//! TMDB-backed catalogue: matching local files, posters, details, seasons,
//! episodes, trending and search.

use crate::service::{is_audio, MediaService, ServiceResult};
use rmh_core::MediaItem;
use rmh_database::{MediaLink, MediaRepository};
use rmh_metadata::{parse_release, Details, Episode, Kind, PosterSize, TmdbClient, Title};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use tokio::task::JoinSet;
use tracing::{info, warn};

pub use rmh_metadata::{Kind as TitleKind, SeasonSummary};

/// A library item together with what we know about it from TMDB
#[derive(Debug, Clone)]
pub struct LibraryEntry {
    pub item: MediaItem,
    pub link: Option<MediaLink>,
}

impl LibraryEntry {
    pub fn kind(&self) -> Option<Kind> {
        match self.link.as_ref()?.kind.as_str() {
            "movie" => Some(Kind::Movie),
            "tv" => Some(Kind::Tv),
            _ => None,
        }
    }

    /// Card title: "Dune" / "The Expanse"
    pub fn display_title(&self) -> String {
        self.link
            .as_ref()
            .and_then(|l| l.show_title.clone())
            .unwrap_or_else(|| self.item.title.clone())
    }

    /// Second line: "S02E05 · Static" / "2021 · Sci-Fi, Drama"
    pub fn subtitle(&self) -> Option<String> {
        let l = self.link.as_ref()?;
        match l.kind.as_str() {
            "tv" => Some(match (l.season, l.episode, &l.episode_title) {
                (Some(s), Some(e), Some(t)) => format!("S{s:02}E{e:02} · {t}"),
                (Some(s), Some(e), None) => format!("S{s:02}E{e:02}"),
                _ => "Series".into(),
            }),
            "movie" => {
                let year = self.item.release_date.as_deref().and_then(|d| d.get(..4)).map(str::to_string);
                let genres = l.genres.clone().filter(|g| !g.is_empty());
                match (year, genres) {
                    (Some(y), Some(g)) => Some(format!("{y} · {g}")),
                    (Some(y), None) => Some(y),
                    (None, g) => g,
                }
            }
            _ => None,
        }
    }

    pub fn poster(&self) -> Option<PathBuf> {
        self.link.as_ref()?.poster_path.as_ref().map(PathBuf::from).filter(|p| p.exists())
    }

    /// Trakt item for scrobbling / history
    pub fn trakt_item(&self) -> Option<rmh_trakt::Item> {
        let l = self.link.as_ref()?;
        let id = l.tmdb_id?;
        match l.kind.as_str() {
            "movie" => Some(rmh_trakt::Item::Movie { tmdb: id }),
            "tv" => Some(rmh_trakt::Item::Episode { show_tmdb: id, season: l.season? as u32, number: l.episode? as u32 }),
            _ => None,
        }
    }
}

/// A movie/show card for Discover / Trakt lists, with its poster on disk
#[derive(Debug, Clone)]
pub struct TitleCard {
    pub title: Title,
    pub poster: Option<PathBuf>,
}

impl MediaService {
    pub(crate) fn tmdb(&self) -> ServiceResult<TmdbClient> {
        let key = self.config().tmdb_api_key.filter(|k| !k.trim().is_empty()).ok_or("TMDB API key not set. Add it in Settings.")?;
        Ok(TmdbClient::new(key.trim().to_string()).map_err(|e| format!("{e:#}"))?.with_cache(self.inner.tmdb_cache.clone()))
    }

    pub fn image_dir(&self) -> PathBuf {
        self.inner.image_dir.clone()
    }

    /// Library items joined with their TMDB links
    pub fn library_entries(&self) -> impl Future<Output = ServiceResult<Vec<LibraryEntry>>> + 'static {
        let db = self.db();
        self.run(async move {
            let db = db.await?;
            let repo = MediaRepository::new(db.pool());
            let items = repo.get_all_media().await.map_err(|e| format!("{e:#}"))?;
            let mut links: HashMap<String, MediaLink> =
                repo.get_media_links().await.map_err(|e| format!("{e:#}"))?.into_iter().map(|l| (l.media_id.clone(), l)).collect();
            let mut out: Vec<LibraryEntry> =
                items.into_iter().map(|item| LibraryEntry { link: links.remove(&item.id.to_string()), item }).collect();
            out.sort_by(|a, b| {
                a.display_title()
                    .to_lowercase()
                    .cmp(&b.display_title().to_lowercase())
                    .then(a.link.as_ref().and_then(|l| l.season).cmp(&b.link.as_ref().and_then(|l| l.season)))
                    .then(a.link.as_ref().and_then(|l| l.episode).cmp(&b.link.as_ref().and_then(|l| l.episode)))
            });
            Ok(out)
        })
    }

    /// Match library files against TMDB. `force` re-matches everything.
    /// Returns (matched, unmatched).
    pub fn match_library(&self, force: bool) -> impl Future<Output = ServiceResult<(usize, usize)>> + 'static {
        let db = self.db();
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        self.run(async move {
            let tmdb = tmdb?;
            let db = db.await?;
            let repo = MediaRepository::new(db.pool());
            let items = repo.get_all_media().await.map_err(|e| format!("{e:#}"))?;
            let known: HashMap<String, MediaLink> =
                repo.get_media_links().await.map_err(|e| format!("{e:#}"))?.into_iter().map(|l| (l.media_id.clone(), l)).collect();
            let (mut matched, mut unmatched) = (0, 0);
            let mut details_cache: HashMap<(Kind, i32), Details> = HashMap::new();
            for item in items {
                if is_audio(&item) || (!force && known.contains_key(&item.id.to_string())) {
                    continue;
                }
                let parsed = parse_release(&item.file_name);
                let hit = match tmdb.match_file(&parsed).await {
                    Ok(h) => h,
                    Err(e) => {
                        warn!("TMDB match failed for {}: {e:#}", item.file_name);
                        if e.to_string().contains("401") {
                            return Err(e.to_string());
                        }
                        continue;
                    }
                };
                let Some(title) = hit else {
                    unmatched += 1;
                    let _ = repo
                        .upsert_media_link(&MediaLink { media_id: item.id.to_string(), kind: "none".into(), ..Default::default() })
                        .await;
                    continue;
                };
                let details = match details_cache.get(&(title.kind, title.tmdb_id)) {
                    Some(d) => Some(d.clone()),
                    None => match tmdb.details(title.kind, title.tmdb_id).await {
                        Ok(d) => {
                            details_cache.insert((title.kind, title.tmdb_id), d.clone());
                            Some(d)
                        }
                        Err(e) => {
                            warn!("TMDB details failed: {e:#}");
                            None
                        }
                    },
                };
                let poster = match title.poster_url(PosterSize::W342) {
                    Some(url) => crate::images::fetch_image(&http, &url, &images).await.ok(),
                    None => None,
                };
                let episode = if title.kind == Kind::Tv {
                    match (parsed.season, parsed.episode) {
                        (Some(s), Some(e)) => tmdb.episode(title.tmdb_id, s, e).await.ok(),
                        _ => None,
                    }
                } else {
                    None
                };
                let link = MediaLink {
                    media_id: item.id.to_string(),
                    kind: title.kind.as_str().into(),
                    tmdb_id: Some(title.tmdb_id as i64),
                    imdb_id: details.as_ref().and_then(|d| d.imdb_id.clone()),
                    show_title: Some(title.name.clone()),
                    season: parsed.season.map(i64::from),
                    episode: parsed.episode.map(i64::from),
                    episode_title: episode.as_ref().map(|e| e.name.clone()),
                    poster_path: poster.map(|p| p.to_string_lossy().to_string()),
                    genres: details.as_ref().map(|d| d.genres.iter().take(3).cloned().collect::<Vec<_>>().join(", ")),
                    trakt_synced_at: None,
                };
                repo.upsert_media_link(&link).await.map_err(|e| format!("{e:#}"))?;
                let overview = episode.as_ref().and_then(|e| e.overview.clone()).or(title.overview.clone());
                let release = episode.as_ref().and_then(|e| e.air_date.clone()).or(title.year.map(|y| format!("{y}-01-01")));
                let runtime = episode.as_ref().and_then(|e| e.runtime).or(details.as_ref().and_then(|d| d.runtime_minutes));
                let poster_url = title.poster_url(PosterSize::W342);
                let backdrop = title.backdrop_path.as_deref().map(|b| TmdbClient::backdrop_url(b, rmh_metadata::BackdropSize::W1280));
                let _ = repo
                    .update_media_metadata(
                        item.id,
                        &item.title,
                        poster_url.as_deref(),
                        backdrop.as_deref(),
                        overview.as_deref(),
                        release.as_deref(),
                        title.rating,
                        runtime,
                        Some(title.tmdb_id),
                    )
                    .await;
                matched += 1;
            }
            info!("TMDB matching: {matched} matched, {unmatched} unmatched");
            Ok((matched, unmatched))
        })
    }

    /// Movies and shows for a query; an IMDb id ("tt…") is looked up directly
    pub fn search_titles(&self, query: impl Into<String>) -> impl Future<Output = ServiceResult<Vec<TitleCard>>> + 'static {
        let query: String = query.into();
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        self.run(async move {
            let tmdb = tmdb?;
            let q = query.trim();
            let titles = if q.starts_with("tt") && q[2..].chars().all(|c| c.is_ascii_digit()) {
                tmdb.find_imdb(q).await.map_err(|e| format!("{e:#}"))?.into_iter().collect()
            } else {
                tmdb.search_titles(q).await.map_err(|e| format!("TMDB search failed: {e:#}"))?
            };
            Ok(with_posters(titles, &http, &images).await)
        })
    }

    pub fn trending(&self) -> impl Future<Output = ServiceResult<Vec<TitleCard>>> + 'static {
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        self.run(async move {
            let titles = tmdb?.trending(None).await.map_err(|e| format!("{e:#}"))?;
            Ok(with_posters(titles, &http, &images).await)
        })
    }

    pub fn title_details(&self, kind: Kind, tmdb_id: i32) -> impl Future<Output = ServiceResult<(Details, Option<PathBuf>)>> + 'static {
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        self.run(async move {
            let d = tmdb?.details(kind, tmdb_id).await.map_err(|e| format!("{e:#}"))?;
            let backdrop = match d.title.backdrop_path.as_deref() {
                Some(b) => crate::images::fetch_image(&http, &TmdbClient::backdrop_url(b, rmh_metadata::BackdropSize::W780), &images).await.ok(),
                None => None,
            };
            Ok((d, backdrop))
        })
    }

    pub fn season_episodes(&self, show_id: i32, season: u32) -> impl Future<Output = ServiceResult<Vec<Episode>>> + 'static {
        let tmdb = self.tmdb();
        self.run(async move { tmdb?.season(show_id, season).await.map_err(|e| format!("{e:#}")) })
    }

    /// Cache posters for (kind, tmdb id) pairs, e.g. Trakt lists. Missing ones are skipped.
    pub fn cards_for(&self, ids: Vec<(Kind, i32)>) -> impl Future<Output = ServiceResult<Vec<TitleCard>>> + 'static {
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        self.run(async move {
            let tmdb = tmdb?;
            let mut set = JoinSet::new();
            for (i, (kind, id)) in ids.into_iter().enumerate() {
                let tmdb = tmdb.clone();
                set.spawn(async move { (i, tmdb.details(kind, id).await.ok().map(|d| d.title)) });
            }
            let mut titles: Vec<(usize, Title)> = vec![];
            while let Some(Ok((i, t))) = set.join_next().await {
                if let Some(t) = t {
                    titles.push((i, t));
                }
            }
            titles.sort_by_key(|(i, _)| *i);
            Ok(with_posters(titles.into_iter().map(|(_, t)| t).collect(), &http, &images).await)
        })
    }
}

/// Cards for `titles`, with posters only where they are already on disk.
/// Nothing is downloaded here: the UI fetches posters lazily as cards scroll
/// into view (see [`MediaService::image`]).
pub(crate) async fn with_posters(titles: Vec<Title>, _http: &reqwest::Client, dir: &Path) -> Vec<TitleCard> {
    titles
        .into_iter()
        .map(|title| {
            let poster = crate::images::poster_url(&title)
                .map(|u| rmh_metadata::cached_image_path(&u, dir))
                .filter(|p| p.exists());
            TitleCard { poster, title }
        })
        .collect()
}

impl MediaService {
    /// A small JPEG of `source` (file or URL) at `secs`, for seek-bar previews.
    /// Cached per 5-second bucket; needs `ffmpeg` on PATH.
    pub fn thumbnail(&self, source: String, secs: f64) -> impl Future<Output = ServiceResult<PathBuf>> + 'static {
        let dir = self.inner.image_dir.join("thumbs");
        self.run(async move {
            let bucket = (secs.max(0.0) / 5.0).floor() as u64 * 5;
            let key = source.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
            let out = dir.join(format!("{key:016x}-{bucket}.jpg"));
            if out.exists() {
                return Ok(out);
            }
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let tmp = out.with_extension("tmp.jpg");
            let status = tokio::process::Command::new("ffmpeg")
                .args(["-nostdin", "-loglevel", "error", "-y", "-ss", &bucket.to_string(), "-i", &source])
                .args(["-frames:v", "1", "-vf", "scale=256:-2", "-q:v", "6"])
                .arg(&tmp)
                .kill_on_drop(true)
                .status()
                .await
                .map_err(|e| format!("ffmpeg not available: {e}"))?;
            if !status.success() || !tmp.exists() {
                return Err("thumbnail failed".into());
            }
            std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
            Ok(out)
        })
    }
}

/// Next local episode after `current` (same show, next season/episode)
pub fn next_episode<'a>(entries: &'a [LibraryEntry], current: &LibraryEntry) -> Option<&'a LibraryEntry> {
    let cur = current.link.as_ref().filter(|l| l.kind == "tv")?;
    let key = (cur.season?, cur.episode?);
    entries
        .iter()
        .filter_map(|e| {
            let l = e.link.as_ref()?;
            (l.kind == "tv" && l.tmdb_id == cur.tmdb_id).then_some((l.season?, l.episode?, e))
        })
        .filter(|(s, ep, _)| (*s, *ep) > key)
        .min_by_key(|(s, ep, _)| (*s, *ep))
        .map(|(_, _, e)| e)
}

/// "Up next": for every show with a watched (or in-progress) episode, the first
/// unwatched local episode after the latest one watched.
pub fn up_next(entries: &[LibraryEntry]) -> Vec<&LibraryEntry> {
    let mut latest: HashMap<i64, (i64, i64, chrono_like::Stamp)> = HashMap::new();
    for e in entries {
        let Some(l) = e.link.as_ref().filter(|l| l.kind == "tv") else { continue };
        let (Some(id), Some(s), Some(ep)) = (l.tmdb_id, l.season, l.episode) else { continue };
        if e.item.watched || e.item.watch_progress > 0 {
            let stamp = e.item.updated_at.timestamp();
            let entry = latest.entry(id).or_insert((s, ep, stamp));
            if (s, ep) > (entry.0, entry.1) {
                *entry = (s, ep, stamp.max(entry.2));
            }
        }
    }
    let mut out: Vec<(&LibraryEntry, chrono_like::Stamp)> = latest
        .into_iter()
        .filter_map(|(id, (s, ep, stamp))| {
            entries
                .iter()
                .filter_map(|e| {
                    let l = e.link.as_ref()?;
                    (l.kind == "tv" && l.tmdb_id == Some(id) && !e.item.watched).then_some((l.season?, l.episode?, e))
                })
                .filter(|(es, eep, _)| (*es, *eep) > (s, ep) || ((*es, *eep) == (s, ep)))
                .min_by_key(|(es, eep, _)| (*es, *eep))
                .map(|(_, _, e)| (e, stamp))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out.into_iter().map(|(e, _)| e).collect()
}

mod chrono_like {
    pub type Stamp = i64;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmh_core::MediaItem;

    fn ep(s: i64, e: i64, watched: bool, progress: i64) -> LibraryEntry {
        let mut item = MediaItem::from_scan(format!("/x/S{s}E{e}.mkv"), format!("S{s}E{e}.mkv"), 1);
        item.watched = watched;
        item.watch_progress = progress;
        LibraryEntry {
            link: Some(MediaLink { media_id: item.id.to_string(), kind: "tv".into(), tmdb_id: Some(1), season: Some(s), episode: Some(e), ..Default::default() }),
            item,
        }
    }

    #[test]
    fn next_and_up_next() {
        let list = vec![ep(1, 1, true, 0), ep(1, 2, false, 0), ep(1, 3, false, 0), ep(2, 1, false, 0)];
        assert_eq!(next_episode(&list, &list[1]).unwrap().link.as_ref().unwrap().episode, Some(3));
        assert_eq!(next_episode(&list, &list[2]).unwrap().link.as_ref().unwrap().season, Some(2));
        assert!(next_episode(&list, &list[3]).is_none());
        let up = up_next(&list);
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].link.as_ref().unwrap().episode, Some(2));
        // in-progress episode is itself "up next"
        let list = vec![ep(1, 1, true, 0), ep(1, 2, false, 300), ep(1, 3, false, 0)];
        assert_eq!(up_next(&list)[0].link.as_ref().unwrap().episode, Some(2));
    }
}
