//! Find & browse: instant search (library + TMDB, typo tolerant), curated
//! rows, the Browse filter engine, details extras and person pages.

use crate::catalog::{with_posters, LibraryEntry, TitleCard};
use crate::fuzzy;
use crate::service::{is_audio, MediaService, ServiceResult};
use rmh_metadata::{Kind, TmdbClient, Title};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use tokio::task::JoinSet;

pub use rmh_metadata::{
    CastMember, Credit, DiscoverQuery, Extras, Genre, ListKind, Person, PersonCard, Provider, Sort as BrowseSort, Video,
};

/// A person search hit with a cached photo
#[derive(Debug, Clone)]
pub struct PersonHit {
    pub card: PersonCard,
    pub photo: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchHits {
    pub titles: Vec<TitleCard>,
    pub people: Vec<PersonHit>,
    /// Set when nothing matched literally and we searched a corrected spelling
    pub corrected: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BrowseResults {
    pub cards: Vec<TitleCard>,
    pub page: u32,
    pub total_pages: u32,
    pub total_results: u32,
}

/// Filter options for the Browse page
#[derive(Debug, Clone, Default)]
pub struct Facets {
    pub movie_genres: Vec<Genre>,
    pub tv_genres: Vec<Genre>,
    pub movie_providers: Vec<(Provider, Option<PathBuf>)>,
    pub tv_providers: Vec<(Provider, Option<PathBuf>)>,
}

#[derive(Debug, Clone, Default)]
pub struct TitleExtras {
    pub extras: Extras,
    pub photos: HashMap<i32, PathBuf>,
    pub provider_logos: HashMap<i32, PathBuf>,
    pub recommended: Vec<TitleCard>,
    pub similar: Vec<TitleCard>,
}

#[derive(Debug, Clone)]
pub struct PersonPage {
    pub person: Person,
    pub photo: Option<PathBuf>,
    /// Known-for (most voted) first
    pub known_for: Vec<TitleCard>,
    /// Full filmography, newest first: (card, role, department)
    pub credits: Vec<(TitleCard, String, String)>,
}

/// Library search: best match per movie / show, fuzzy
pub fn search_library<'a>(entries: &'a [LibraryEntry], query: &str) -> Vec<(&'a LibraryEntry, f32)> {
    let mut best: HashMap<String, (&LibraryEntry, f32)> = HashMap::new();
    for e in entries.iter().filter(|e| !is_audio(&e.item)) {
        let title = e.display_title();
        let s = fuzzy::score(query, &title).max(fuzzy::score(query, &e.item.title) * 0.95);
        if s < 0.7 {
            continue;
        }
        let key = e.link.as_ref().and_then(|l| l.tmdb_id.map(|t| format!("{}{t}", l.kind))).unwrap_or_else(|| title.to_lowercase());
        let cur = best.entry(key).or_insert((e, s));
        // prefer the in-progress / earliest unwatched episode as the representative
        if s > cur.1 || (s == cur.1 && e.item.watch_progress > cur.0.item.watch_progress) {
            *cur = (e, s);
        }
    }
    let mut v: Vec<(&LibraryEntry, f32)> = best.into_values().collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.display_title().cmp(&b.0.display_title())));
    v
}

/// (kind, tmdb id) of everything in the library, for "in library" badges
pub fn library_ids(entries: &[LibraryEntry]) -> HashSet<(Kind, i32)> {
    entries.iter().filter_map(|e| Some((e.kind()?, e.link.as_ref()?.tmdb_id? as i32))).collect()
}

async fn cache_many(http: &reqwest::Client, dir: &Path, items: Vec<(i32, String)>) -> HashMap<i32, PathBuf> {
    let mut set = JoinSet::new();
    for (id, url) in items {
        let (http, dir) = (http.clone(), dir.to_path_buf());
        set.spawn(async move { (id, crate::images::fetch_image(&http, &url, &dir).await.ok()) });
    }
    let mut out = HashMap::new();
    while let Some(Ok((id, p))) = set.join_next().await {
        if let Some(p) = p {
            out.insert(id, p);
        }
    }
    out
}

impl MediaService {
    pub(crate) fn remember(&self, titles: &[Title]) {
        if let Ok(mut seen) = self.inner.seen.write() {
            for t in titles {
                seen.entry((t.kind, t.tmdb_id)).or_insert_with(|| t.clone());
            }
        }
    }

    fn seen_matches(&self, query: &str, min: f32) -> Vec<Title> {
        let Ok(seen) = self.inner.seen.read() else { return vec![] };
        let mut v: Vec<(f32, &Title)> =
            seen.values().map(|t| (fuzzy::score(query, &t.name), t)).filter(|(s, _)| *s >= min).collect();
        v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.votes.cmp(&a.1.votes)));
        v.into_iter().take(8).map(|(_, t)| t.clone()).collect()
    }

    /// As-you-type search over TMDB movies, shows and people, tolerant to typos
    pub fn search_everything(&self, query: impl Into<String>) -> impl Future<Output = ServiceResult<SearchHits>> + 'static {
        let query: String = query.into();
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        let svc = self.clone();
        self.run(async move {
            let tmdb = tmdb?;
            let q = query.trim().to_string();
            if q.is_empty() {
                return Ok(SearchHits::default());
            }
            if q.starts_with("tt") && q.len() > 3 && q[2..].chars().all(|c| c.is_ascii_digit()) {
                let titles: Vec<Title> = tmdb.find_imdb(&q).await.map_err(|e| format!("{e:#}"))?.into_iter().collect();
                return Ok(SearchHits { titles: with_posters(titles, &http, &images).await, ..Default::default() });
            }
            let mut res = tmdb.search_all(&q).await.map_err(|e| format!("TMDB search failed: {e:#}"))?;
            let mut corrected = None;
            // Weak results (few hits, or only obscure titles): a typo is likely
            let popular = res.titles.iter().any(|t| t.votes.unwrap_or(0) >= 300 && fuzzy::score(&q, &t.name) >= 0.85);
            let mut fallback: HashSet<(Kind, i32)> = HashSet::new();
            if (res.titles.len() < 3 || !popular) && q.chars().count() >= 4 {
                // 1) titles we've already seen (trending, browse, library) by fuzzy match
                let mut extra = svc.seen_matches(&q, 0.72);
                // 2) TMDB matches prefixes, so a typo near the end disappears when trimmed
                let chars: Vec<char> = q.chars().collect();
                for cut in 1..=3usize {
                    if chars.len() <= cut + 3 {
                        break;
                    }
                    let shorter: String = chars[..chars.len() - cut].iter().collect();
                    let r = tmdb.search_all(&shorter).await.unwrap_or_default();
                    let good: Vec<Title> = r.titles.into_iter().filter(|t| fuzzy::score(&q, &t.name) >= 0.6).collect();
                    if good.iter().any(|t| t.votes.unwrap_or(0) >= 300) || (!good.is_empty() && res.titles.is_empty()) {
                        extra.extend(good);
                        break;
                    }
                }
                // 3) multi-word queries: try the longest word alone
                if res.titles.is_empty() && extra.is_empty() && q.contains(' ') {
                    if let Some(w) = q.split_whitespace().max_by_key(|w| w.len()).filter(|w| w.len() >= 4) {
                        let r = tmdb.search_all(w).await.unwrap_or_default();
                        extra = r.titles.into_iter().filter(|t| fuzzy::score(&q, &t.name) >= 0.6).collect();
                    }
                }
                let have: HashSet<(Kind, i32)> = res.titles.iter().map(|t| (t.kind, t.tmdb_id)).collect();
                for t in extra {
                    if !have.contains(&(t.kind, t.tmdb_id)) && fallback.insert((t.kind, t.tmdb_id)) {
                        res.titles.push(t);
                    }
                }
            }
            // How well the name matches, plus a popularity nudge so a famous title
            // with one typo beats an obscure exact match
            let rank = |t: &Title| fuzzy::score(&q, &t.name) + 0.05 * ((t.votes.unwrap_or(0) as f32) + 1.0).log10();
            let mut ranked: Vec<(usize, Title)> = res.titles.into_iter().enumerate().collect();
            ranked.sort_by(|(ia, a), (ib, b)| rank(b).partial_cmp(&rank(a)).unwrap_or(std::cmp::Ordering::Equal).then(ia.cmp(ib)));
            if let Some((_, top)) = ranked.first() {
                if fallback.contains(&(top.kind, top.tmdb_id)) && fuzzy::score(&q, &top.name) < 0.95 {
                    corrected = Some(top.name.clone());
                }
            }
            let titles: Vec<Title> = ranked.into_iter().map(|(_, t)| t).take(16).collect();
            svc.remember(&titles);
            let people: Vec<PersonCard> = res.people.into_iter().take(6).collect();
            let photos = cache_many(
                &http,
                &images,
                people.iter().filter_map(|p| Some((p.id, TmdbClient::profile_url(p.profile_path.as_deref()?)))).collect(),
            )
            .await;
            Ok(SearchHits {
                titles: with_posters(titles, &http, &images).await,
                people: people.into_iter().map(|p| PersonHit { photo: photos.get(&p.id).cloned(), card: p }).collect(),
                corrected,
            })
        })
    }

    /// Genres and streaming services for the filter bar
    pub fn browse_facets(&self) -> impl Future<Output = ServiceResult<Facets>> + 'static {
        let tmdb = self.tmdb();
        let region = self.config().region;
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        self.run(async move {
            let tmdb = tmdb?;
            let err = |e: anyhow::Error| format!("{e:#}");
            let movie_genres = tmdb.genres(Kind::Movie).await.map_err(err)?;
            let tv_genres = tmdb.genres(Kind::Tv).await.map_err(err)?;
            let mp: Vec<Provider> = tmdb.providers(Kind::Movie, &region).await.unwrap_or_default().into_iter().take(14).collect();
            let tp: Vec<Provider> = tmdb.providers(Kind::Tv, &region).await.unwrap_or_default().into_iter().take(14).collect();
            let logos = cache_many(
                &http,
                &images,
                mp.iter().chain(tp.iter()).filter_map(|p| Some((p.id, TmdbClient::logo_url(p.logo_path.as_deref()?)))).collect(),
            )
            .await;
            let with = |v: Vec<Provider>| v.into_iter().map(|p| (p.clone(), logos.get(&p.id).cloned())).collect();
            Ok(Facets { movie_genres, tv_genres, movie_providers: with(mp), tv_providers: with(tp) })
        })
    }

    /// A curated row (trending, popular, top rated, …)
    pub fn browse_list(&self, kind: Kind, list: ListKind, page: u32) -> impl Future<Output = ServiceResult<BrowseResults>> + 'static {
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        let svc = self.clone();
        self.run(async move {
            let p = tmdb?.list(kind, list, page).await.map_err(|e| format!("{e:#}"))?;
            svc.remember(&p.results);
            Ok(BrowseResults { cards: with_posters(p.results, &http, &images).await, page: p.page, total_pages: p.total_pages, total_results: p.total_results })
        })
    }

    /// Filtered + sorted results (TMDB /discover)
    pub fn browse_discover(&self, mut q: DiscoverQuery) -> impl Future<Output = ServiceResult<BrowseResults>> + 'static {
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        q.region = self.config().region;
        let svc = self.clone();
        self.run(async move {
            let p = tmdb?.discover(&q).await.map_err(|e| format!("{e:#}"))?;
            svc.remember(&p.results);
            Ok(BrowseResults { cards: with_posters(p.results, &http, &images).await, page: p.page, total_pages: p.total_pages, total_results: p.total_results })
        })
    }

    /// Cast, crew, trailers, where to stream, similar & recommended
    pub fn title_extras(&self, kind: Kind, tmdb_id: i32) -> impl Future<Output = ServiceResult<TitleExtras>> + 'static {
        let tmdb = self.tmdb();
        let region = self.config().region;
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        let svc = self.clone();
        self.run(async move {
            let extras = tmdb?.extras(kind, tmdb_id, &region).await.map_err(|e| format!("{e:#}"))?;
            let people: Vec<(i32, String)> = extras
                .cast
                .iter()
                .take(14)
                .chain(extras.crew.iter().take(4))
                .filter_map(|c| Some((c.id, TmdbClient::profile_url(c.profile_path.as_deref()?))))
                .collect();
            let photos = cache_many(&http, &images, people).await;
            let provider_logos = cache_many(
                &http,
                &images,
                extras.providers.iter().filter_map(|p| Some((p.id, TmdbClient::logo_url(p.logo_path.as_deref()?)))).collect(),
            )
            .await;
            svc.remember(&extras.recommendations);
            svc.remember(&extras.similar);
            let recommended = with_posters(extras.recommendations.iter().take(16).cloned().collect(), &http, &images).await;
            // TMDB's "similar" list is noisy: keep titles people actually watched and rated
            let rec_ids: HashSet<i32> = extras.recommendations.iter().map(|t| t.tmdb_id).collect();
            let mut sim: Vec<Title> = extras
                .similar
                .iter()
                .filter(|t| t.votes.unwrap_or(0) >= 150 && t.rating.unwrap_or(0.0) >= 6.0 && !rec_ids.contains(&t.tmdb_id))
                .cloned()
                .collect();
            sim.sort_by(|a, b| b.votes.unwrap_or(0).cmp(&a.votes.unwrap_or(0)));
            let similar = with_posters(sim.into_iter().take(16).collect(), &http, &images).await;
            Ok(TitleExtras { extras, photos, provider_logos, recommended, similar })
        })
    }

    pub fn person_page(&self, id: i32) -> impl Future<Output = ServiceResult<PersonPage>> + 'static {
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        self.run(async move {
            let person = tmdb?.person(id).await.map_err(|e| format!("{e:#}"))?;
            let photo = match person.profile_path.as_deref() {
                Some(p) => crate::images::fetch_image(&http, &TmdbClient::profile_url(p).replace("/w185", "/h632"), &images).await.ok(),
                None => None,
            };
            let mut known: Vec<&Credit> = person.credits.iter().filter(|c| c.title.poster_path.is_some()).collect();
            known.sort_by(|a, b| b.title.votes.unwrap_or(0).cmp(&a.title.votes.unwrap_or(0)));
            let known_titles: Vec<Title> = known.iter().take(12).map(|c| c.title.clone()).collect();
            // skip "Self", "Thanks" and archive-footage appearances
            let noise = |role: &str| {
                role.split(" · ").all(|r| {
                    let r = r.to_lowercase();
                    r.is_empty() || r.starts_with("self") || r.starts_with("himself") || r.starts_with("herself") || r == "thanks" || r.contains("archive")
                })
            };
            let all: Vec<Credit> = person.credits.iter().filter(|c| !noise(&c.role) || c.department != "Acting" && c.department != "Crew").take(80).cloned().collect();
            let known_for = with_posters(known_titles, &http, &images).await;
            let cards = with_posters(all.iter().map(|c| c.title.clone()).collect(), &http, &images).await;
            let credits = cards.into_iter().zip(all).map(|(card, c)| (card, c.role, c.department)).collect();
            Ok(PersonPage { person, photo, known_for, credits })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmh_core::MediaItem;
    use rmh_database::MediaLink;

    fn entry(title: &str, show: Option<(&str, i64)>) -> LibraryEntry {
        let mut item = MediaItem::from_scan(format!("/m/{title}.mkv"), format!("{title}.mkv"), 1);
        item.title = title.to_string();
        LibraryEntry {
            item,
            link: show.map(|(name, id)| MediaLink {
                kind: "tv".into(),
                tmdb_id: Some(id),
                show_title: Some(name.into()),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn library_search_is_fuzzy_and_collapses_episodes() {
        let e = vec![
            entry("The Expanse S01E01", Some(("The Expanse", 1))),
            entry("The Expanse S01E02", Some(("The Expanse", 1))),
            entry("Pelíšky", None),
            entry("Dune", None),
        ];
        let r = search_library(&e, "expanse");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0.display_title(), "The Expanse");
        assert_eq!(search_library(&e, "pelisky")[0].0.item.title, "Pelíšky");
        assert_eq!(search_library(&e, "dnue")[0].0.item.title, "Dune");
        assert!(search_library(&e, "matrix").is_empty());
    }
}
