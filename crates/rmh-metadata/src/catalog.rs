//! Richer TMDB catalogue: release-name parsing, unified movie/TV search,
//! TV seasons & episodes, IMDb lookups, trending lists and poster caching.

use crate::{PosterSize, TmdbClient, TMDB_BASE_URL};
use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What a file name looks like: a movie or a TV episode
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedName {
    pub title: String,
    pub year: Option<u16>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

impl ParsedName {
    pub fn is_episode(&self) -> bool {
        self.season.is_some() && self.episode.is_some()
    }
}

/// Parse "The.Expanse.S02E05.1080p.WEB.x264.mkv" / "Dune (2021) 2160p.mkv"
pub fn parse_release(file_name: &str) -> ParsedName {
    let stem = Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
    let norm: String = stem.chars().map(|c| if matches!(c, '.' | '_' | '[' | ']' | '(' | ')' | '{' | '}') { ' ' } else { c }).collect();

    let ep_re = regex_lite::Regex::new(r"(?i)\b(?:s(\d{1,2})[ ._-]?e(\d{1,3})|(\d{1,2})x(\d{2,3}))\b").unwrap();
    let year_re = regex_lite::Regex::new(r"\b(19[0-9]{2}|20[0-9]{2})\b").unwrap();
    let junk_re = regex_lite::Regex::new(
        r"(?i)\b(2160p|1080p|720p|576p|480p|4k|uhd|hdr10?|dv|bluray|blu ray|bdrip|brrip|dvdrip|web ?dl|webrip|web|hdtv|hdrip|remux|x264|x265|h ?264|h ?265|hevc|avc|aac|ac3|eac3|dts|atmos|truehd|ddp?5 1|5 1|7 1|proper|repack|extended|unrated|imax|multi|dual|complete|season|internal|10bit)\b",
    )
    .unwrap();

    let mut cut = norm.len();
    let (mut season, mut episode) = (None, None);
    if let Some(c) = ep_re.captures(&norm) {
        let m = c.get(0).unwrap();
        cut = cut.min(m.start());
        season = c.get(1).or(c.get(3)).and_then(|v| v.as_str().parse().ok());
        episode = c.get(2).or(c.get(4)).and_then(|v| v.as_str().parse().ok());
    }
    let mut year = None;
    // The year is the last 19xx/20xx before the cut that isn't the whole title
    for m in year_re.find_iter(&norm) {
        if m.start() > 0 && m.start() <= cut {
            year = m.as_str().parse().ok();
            cut = cut.min(m.start());
            break;
        }
    }
    if let Some(m) = junk_re.find(&norm) {
        cut = cut.min(m.start());
    }
    let title = norm[..cut].replace('-', " ").split_whitespace().collect::<Vec<_>>().join(" ");
    let title = if title.is_empty() { stem.to_string() } else { title };
    ParsedName { title, year, season, episode }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Movie,
    Tv,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Movie => "movie",
            Kind::Tv => "tv",
        }
    }
}

/// A movie or a show, as shown on cards
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Title {
    pub kind: Kind,
    pub tmdb_id: i32,
    pub name: String,
    pub year: Option<u16>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub rating: Option<f32>,
    #[serde(default)]
    pub genre_ids: Vec<i32>,
    #[serde(default)]
    pub original_language: Option<String>,
    #[serde(default)]
    pub votes: Option<u32>,
}

impl Title {
    pub fn poster_url(&self, size: PosterSize) -> Option<String> {
        self.poster_path.as_deref().map(|p| TmdbClient::poster_url(p, size))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawTitle {
    id: i32,
    media_type: Option<String>,
    title: Option<String>,
    name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    overview: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    vote_average: Option<f32>,
    #[serde(default)]
    genre_ids: Vec<i32>,
    original_language: Option<String>,
    vote_count: Option<u32>,
}

impl RawTitle {
    pub(crate) fn into_title(self, default: Option<Kind>) -> Option<Title> {
        let kind = match self.media_type.as_deref() {
            Some("movie") => Kind::Movie,
            Some("tv") => Kind::Tv,
            Some(_) => return None, // person, collection
            None => default?,
        };
        let date = self.release_date.or(self.first_air_date);
        Some(Title {
            kind,
            tmdb_id: self.id,
            name: self.title.or(self.name).unwrap_or_default(),
            year: date.as_deref().and_then(|d| d.get(..4)).and_then(|y| y.parse().ok()),
            overview: self.overview.filter(|o| !o.is_empty()),
            poster_path: self.poster_path,
            backdrop_path: self.backdrop_path,
            rating: self.vote_average,
            genre_ids: self.genre_ids,
            original_language: self.original_language,
            votes: self.vote_count,
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Page<T> {
    pub(crate) results: Vec<T>,
}

/// Full details for a movie or show
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Details {
    pub title: Title,
    pub imdb_id: Option<String>,
    pub genres: Vec<String>,
    pub runtime_minutes: Option<i32>,
    pub tagline: Option<String>,
    /// Seasons (shows only), specials (season 0) last
    pub seasons: Vec<SeasonSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonSummary {
    pub number: u32,
    pub name: String,
    pub episode_count: u32,
    pub poster_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub season: u32,
    pub number: u32,
    pub name: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub still_path: Option<String>,
    pub runtime: Option<i32>,
    pub rating: Option<f32>,
}

#[derive(Deserialize)]
struct RawDetails {
    id: i32,
    title: Option<String>,
    name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    overview: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    vote_average: Option<f32>,
    vote_count: Option<u32>,
    original_language: Option<String>,
    #[serde(default)]
    genres: Vec<crate::TmdbGenre>,
    runtime: Option<i32>,
    #[serde(default)]
    episode_run_time: Vec<i32>,
    tagline: Option<String>,
    imdb_id: Option<String>,
    external_ids: Option<AppendedIds>,
    #[serde(default)]
    seasons: Vec<RawSeason>,
}

/// `append_to_response=external_ids` (no `id` field, unlike /external_ids)
#[derive(Deserialize)]
struct AppendedIds {
    imdb_id: Option<String>,
}

#[derive(Deserialize)]
struct RawSeason {
    season_number: u32,
    name: Option<String>,
    episode_count: Option<u32>,
    poster_path: Option<String>,
}

#[derive(Deserialize)]
struct RawEpisodes {
    #[serde(default)]
    episodes: Vec<RawEpisode>,
}

#[derive(Deserialize)]
struct RawEpisode {
    season_number: u32,
    episode_number: u32,
    name: Option<String>,
    overview: Option<String>,
    air_date: Option<String>,
    still_path: Option<String>,
    runtime: Option<i32>,
    vote_average: Option<f32>,
}

impl From<RawEpisode> for Episode {
    fn from(e: RawEpisode) -> Self {
        Episode {
            season: e.season_number,
            number: e.episode_number,
            name: e.name.unwrap_or_else(|| format!("Episode {}", e.episode_number)),
            overview: e.overview.filter(|o| !o.is_empty()),
            air_date: e.air_date,
            still_path: e.still_path,
            runtime: e.runtime,
            rating: e.vote_average,
        }
    }
}

#[derive(Deserialize)]
struct FindResponse {
    #[serde(default)]
    movie_results: Vec<RawTitle>,
    #[serde(default)]
    tv_results: Vec<RawTitle>,
}

impl TmdbClient {
    /// Responses are cached on disk when a cache dir is set: lists for a few
    /// hours, details for days. A stale copy is used when TMDB is unreachable.
    pub fn with_cache(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    fn ttl_for(path: &str) -> std::time::Duration {
        let h = |n: u64| std::time::Duration::from_secs(n * 3600);
        if path.starts_with("/search") {
            h(6)
        } else if path.starts_with("/trending") || path.starts_with("/discover") || path.contains("/popular")
            || path.contains("/now_playing") || path.contains("/upcoming") || path.contains("/on_the_air")
            || path.contains("/airing_today") || path.contains("/top_rated")
        {
            h(4)
        } else if path.starts_with("/genre") || path.starts_with("/watch/providers") || path.starts_with("/configuration") {
            h(24 * 7)
        } else {
            h(48)
        }
    }

    fn cache_file(&self, path: &str, params: &[(&str, &str)]) -> Option<std::path::PathBuf> {
        let dir = self.cache_dir.as_ref()?;
        let mut key = String::from(path);
        for (k, v) in params {
            key.push('&');
            key.push_str(k);
            key.push('=');
            key.push_str(v);
        }
        let h = key.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
        Some(dir.join(format!("{h:016x}.json")))
    }

    /// GET a TMDB v3 endpoint. Accepts a v3 API key or a v4 read access token.
    pub(crate) async fn get_json<T: DeserializeOwned>(&self, path: &str, params: &[(&str, &str)]) -> Result<T> {
        let cache = self.cache_file(path, params);
        let fresh = cache.as_ref().and_then(|f| {
            let age = std::fs::metadata(f).ok()?.modified().ok()?.elapsed().ok()?;
            (age < Self::ttl_for(path)).then(|| std::fs::read(f).ok()).flatten()
        });
        if let Some(bytes) = fresh {
            if let Ok(v) = serde_json::from_slice(&bytes) {
                return Ok(v);
            }
        }
        match self.fetch_bytes(path, params).await {
            Ok(bytes) => {
                let v = serde_json::from_slice(&bytes).with_context(|| format!("Bad TMDB response for {path}"))?;
                if let Some(f) = &cache {
                    if let Some(d) = f.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    let tmp = f.with_extension("part");
                    if std::fs::write(&tmp, &bytes).is_ok() {
                        let _ = std::fs::rename(&tmp, f);
                    }
                }
                Ok(v)
            }
            Err(e) => {
                // offline: a stale cached copy beats an error
                if let Some(bytes) = cache.as_ref().and_then(|f| std::fs::read(f).ok()) {
                    if let Ok(v) = serde_json::from_slice(&bytes) {
                        return Ok(v);
                    }
                }
                Err(e)
            }
        }
    }

    async fn fetch_bytes(&self, path: &str, params: &[(&str, &str)]) -> Result<Vec<u8>> {
        let mut req = self.client.get(format!("{TMDB_BASE_URL}{path}")).query(params);
        if !params.iter().any(|(k, _)| *k == "language") {
            req = req.query(&[("language", "en-US")]);
        }
        req = if self.api_key.starts_with("eyJ") {
            req.bearer_auth(&self.api_key)
        } else {
            req.query(&[("api_key", self.api_key.as_str())])
        };
        let resp = req.send().await.context("TMDB request failed")?;
        let status = resp.status();
        if !status.is_success() {
            if status.as_u16() == 401 {
                bail!("TMDB rejected the API key (401). Check it in Settings.");
            }
            bail!("TMDB error {status} for {path}");
        }
        Ok(resp.bytes().await.context("TMDB read failed")?.to_vec())
    }

    /// Movies and shows matching a query, best first
    pub async fn search_titles(&self, query: &str) -> Result<Vec<Title>> {
        let page: Page<RawTitle> = self.get_json("/search/multi", &[("query", query), ("include_adult", "false")]).await?;
        Ok(page.results.into_iter().filter_map(|r| r.into_title(None)).collect())
    }

    pub async fn search_kind(&self, kind: Kind, query: &str, year: Option<u16>) -> Result<Vec<Title>> {
        let y = year.map(|y| y.to_string());
        let mut params = vec![("query", query)];
        if let Some(y) = y.as_deref() {
            params.push((if kind == Kind::Movie { "year" } else { "first_air_date_year" }, y));
        }
        let page: Page<RawTitle> = self.get_json(&format!("/search/{}", kind.as_str()), &params).await?;
        Ok(page.results.into_iter().filter_map(|r| r.into_title(Some(kind))).collect())
    }

    /// Trending this week (`kind` None = movies and shows)
    pub async fn trending(&self, kind: Option<Kind>) -> Result<Vec<Title>> {
        let k = kind.map(|k| k.as_str()).unwrap_or("all");
        let page: Page<RawTitle> = self.get_json(&format!("/trending/{k}/week"), &[]).await?;
        Ok(page.results.into_iter().filter_map(|r| r.into_title(kind)).collect())
    }

    pub async fn details(&self, kind: Kind, tmdb_id: i32) -> Result<Details> {
        let raw: RawDetails = self
            .get_json(&format!("/{}/{tmdb_id}", kind.as_str()), &[("append_to_response", "external_ids")])
            .await?;
        let date = raw.release_date.or(raw.first_air_date);
        let mut seasons: Vec<SeasonSummary> = raw
            .seasons
            .into_iter()
            .map(|s| SeasonSummary {
                number: s.season_number,
                name: s.name.unwrap_or_else(|| format!("Season {}", s.season_number)),
                episode_count: s.episode_count.unwrap_or(0),
                poster_path: s.poster_path,
            })
            .collect();
        seasons.sort_by_key(|s| if s.number == 0 { u32::MAX } else { s.number });
        Ok(Details {
            title: Title {
                kind,
                tmdb_id: raw.id,
                name: raw.title.or(raw.name).unwrap_or_default(),
                year: date.as_deref().and_then(|d| d.get(..4)).and_then(|y| y.parse().ok()),
                overview: raw.overview.filter(|o| !o.is_empty()),
                poster_path: raw.poster_path,
                backdrop_path: raw.backdrop_path,
                rating: raw.vote_average,
                genre_ids: raw.genres.iter().map(|g| g.id).collect(),
                original_language: raw.original_language,
                votes: raw.vote_count,
            },
            imdb_id: raw.imdb_id.or(raw.external_ids.and_then(|e| e.imdb_id)).filter(|i| !i.is_empty()),
            genres: raw.genres.into_iter().map(|g| g.name).collect(),
            runtime_minutes: raw.runtime.or(raw.episode_run_time.first().copied()),
            tagline: raw.tagline.filter(|t| !t.is_empty()),
            seasons,
        })
    }

    pub async fn season(&self, show_id: i32, season: u32) -> Result<Vec<Episode>> {
        let raw: RawEpisodes = self.get_json(&format!("/tv/{show_id}/season/{season}"), &[]).await?;
        Ok(raw.episodes.into_iter().map(Episode::from).collect())
    }

    pub async fn episode(&self, show_id: i32, season: u32, number: u32) -> Result<Episode> {
        let raw: RawEpisode = self.get_json(&format!("/tv/{show_id}/season/{season}/episode/{number}"), &[]).await?;
        Ok(raw.into())
    }

    /// Look up a movie or show by IMDb id ("tt0133093")
    pub async fn find_imdb(&self, imdb_id: &str) -> Result<Option<Title>> {
        let r: FindResponse = self.get_json(&format!("/find/{imdb_id}"), &[("external_source", "imdb_id")]).await?;
        Ok(r.movie_results
            .into_iter()
            .filter_map(|t| t.into_title(Some(Kind::Movie)))
            .chain(r.tv_results.into_iter().filter_map(|t| t.into_title(Some(Kind::Tv))))
            .next())
    }

    /// Best match for a parsed file name
    pub async fn match_file(&self, parsed: &ParsedName) -> Result<Option<Title>> {
        let kind = if parsed.is_episode() { Kind::Tv } else { Kind::Movie };
        let mut hits = self.search_kind(kind, &parsed.title, parsed.year).await?;
        if hits.is_empty() && parsed.year.is_some() {
            hits = self.search_kind(kind, &parsed.title, None).await?;
        }
        Ok(best_match(hits, &parsed.title, parsed.year))
    }
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// TMDB orders by relevance+popularity, so "Dark" can rank "Dark Shadows"
/// first. Prefer an exact title (and year) match, then TMDB's order.
fn best_match(hits: Vec<Title>, title: &str, year: Option<u16>) -> Option<Title> {
    let want = norm(title);
    let score = |t: &Title| {
        let exact = norm(&t.name) == want;
        let year_ok = year.is_some() && t.year == year;
        (exact as u8) * 2 + year_ok as u8
    };
    let best = hits.iter().enumerate().max_by_key(|(i, t)| (score(t), std::cmp::Reverse(*i))).map(|(i, _)| i)?;
    hits.into_iter().nth(best)
}

/// Download a TMDB image once into `cache_dir`; returns the local file.
pub async fn cache_image(client: &reqwest::Client, url: &str, cache_dir: &Path) -> Result<PathBuf> {
    let name: String = url
        .trim_start_matches("https://image.tmdb.org/t/p/")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' })
        .collect();
    let path = cache_dir.join(name);
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(cache_dir)?;
    let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_exact_title() {
        let t = |id, name: &str, year| Title {
            kind: Kind::Tv,
            tmdb_id: id,
            name: name.into(),
            year,
            overview: None,
            poster_path: None,
            backdrop_path: None,
            rating: None,
            genre_ids: vec![],
            original_language: None,
            votes: None,
        };
        let hits = vec![t(1, "Dark Shadows", Some(1966)), t(2, "Dark", Some(2017)), t(3, "Dark", Some(1990))];
        assert_eq!(best_match(hits.clone(), "Dark", None).unwrap().tmdb_id, 2);
        assert_eq!(best_match(hits.clone(), "Dark", Some(1990)).unwrap().tmdb_id, 3);
        assert_eq!(best_match(hits, "Something", None).unwrap().tmdb_id, 1);
    }

    #[test]
    fn parses_release_names() {
        let p = parse_release("The.Expanse.S02E05.1080p.WEB.x264-GRP.mkv");
        assert_eq!((p.title.as_str(), p.season, p.episode, p.year), ("The Expanse", Some(2), Some(5), None));
        let p = parse_release("Dune (2021) 2160p UHD BluRay.mkv");
        assert_eq!((p.title.as_str(), p.year, p.is_episode()), ("Dune", Some(2021), false));
        let p = parse_release("blade_runner_2049.mp4");
        assert_eq!(p.title, "blade runner");
        assert_eq!(p.year, Some(2049));
        let p = parse_release("Dark 1x03.mkv");
        assert_eq!((p.title.as_str(), p.season, p.episode), ("Dark", Some(1), Some(3)));
        let p = parse_release("1917.mkv");
        assert_eq!(p.title, "1917");
        let p = parse_release("Big Test Movie 2024.mp4");
        assert_eq!((p.title.as_str(), p.year), ("Big Test Movie", Some(2024)));
    }
}
