//! Release calendar: Trakt calendars when connected, TMDB otherwise.
//! Times are converted to the configured time zone (Europe/Prague by default).

use crate::catalog::with_posters;
use crate::service::{MediaService, ServiceResult};
use chrono::{Datelike, NaiveDate, TimeZone};
use rmh_metadata::{Kind, Title, TmdbClient};
use rmh_trakt::CalendarKind;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use tokio::task::JoinSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReleaseKind {
    /// A new episode
    Episode,
    /// Series or season premiere
    Premiere,
    /// In cinemas
    Theatrical,
    /// Streaming / digital / disc
    Digital,
}

impl ReleaseKind {
    pub fn label(self) -> &'static str {
        match self {
            ReleaseKind::Episode => "Episode",
            ReleaseKind::Premiere => "Premiere",
            ReleaseKind::Theatrical => "Cinema",
            ReleaseKind::Digital => "Digital",
        }
    }
    pub fn is_movie(self) -> bool {
        matches!(self, ReleaseKind::Theatrical | ReleaseKind::Digital)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarItem {
    /// Local date
    pub date: NaiveDate,
    /// Local "21:00" when known (episodes from Trakt)
    pub time: Option<String>,
    pub kind: ReleaseKind,
    pub title_kind: Kind,
    pub tmdb_id: Option<i32>,
    pub name: String,
    /// "S02E05 · Name" / "Cinema" / "Digital"
    pub detail: String,
    /// On the user's lists (Trakt "my", watchlist or library)
    pub mine: bool,
    pub poster: Option<PathBuf>,
    #[serde(default)]
    pub title: Option<Title>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CalendarMonth {
    pub year: i32,
    pub month: u32,
    /// First day of the 6-week grid (a Monday)
    pub grid_start: Option<NaiveDate>,
    pub items: Vec<CalendarItem>,
    /// "Trakt" or "TMDB"
    pub source: String,
}

/// Monday on or before the 1st of the month, and the 42-day grid end
pub fn month_grid(year: i32, month: u32) -> (NaiveDate, NaiveDate) {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default();
    let start = first - chrono::Duration::days(first.weekday().num_days_from_monday() as i64);
    (start, start + chrono::Duration::days(41))
}

/// UTC timestamp or plain date → local (date, time)
/// First and last day of a month
pub fn month_range(year: i32, month: u32) -> (NaiveDate, NaiveDate) {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default();
    let next = if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1) } else { NaiveDate::from_ymd_opt(year, month + 1, 1) };
    (first, next.map(|n| n - chrono::Duration::days(1)).unwrap_or(first))
}

/// Parallel TMDB metadata lookups while building a calendar month
const CALENDAR_PARALLEL: usize = 8;

pub fn to_local(when: &str, tz: chrono_tz::Tz) -> Option<(NaiveDate, Option<String>)> {
    if when.len() == 10 {
        return NaiveDate::parse_from_str(when, "%Y-%m-%d").ok().map(|d| (d, None));
    }
    let utc = chrono::DateTime::parse_from_rfc3339(when).ok()?.with_timezone(&chrono::Utc);
    let local = tz.from_utc_datetime(&utc.naive_utc());
    Some((local.date_naive(), Some(local.format("%H:%M").to_string())))
}

impl MediaService {
    fn calendar_cache(&self, key: &str) -> PathBuf {
        self.inner.tmdb_cache.parent().map(|p| p.to_path_buf()).unwrap_or_default().join("calendar").join(format!("{key}.json"))
    }

    /// One month (6-week grid). Cached for 6 hours; `force` refetches.
    pub fn calendar_month(&self, year: i32, month: u32, force: bool) -> impl Future<Output = ServiceResult<CalendarMonth>> + 'static {
        let cfg = self.config();
        let tz: chrono_tz::Tz = cfg.timezone.parse().unwrap_or(chrono_tz::Europe::Prague);
        let trakt = self.inner.trakt.read().ok().and_then(|t| t.clone());
        let tmdb = self.tmdb();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        let region = cfg.region.clone();
        let watch: HashSet<(Kind, i32)> = self.watchlist().iter().map(|w| (w.title.kind, w.title.tmdb_id)).collect();
        let lib = self.library_tmdb_ids();
        let svc = self.clone();
        self.run(async move {
            let lib = lib.await;
            let mut mine_ids: HashSet<(Kind, i32)> = watch.clone();
            mine_ids.extend(lib.iter().copied());
            let use_trakt = match &trakt {
                Some(t) => t.is_logged_in().await,
                None => false,
            };
            let source = if use_trakt { "Trakt" } else { "TMDB" };
            let cache = svc.calendar_cache(&format!("{source}-{year}-{month:02}-{region}"));
            if !force {
                let fresh = std::fs::metadata(&cache)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age < std::time::Duration::from_secs(6 * 3600));
                if fresh {
                    if let Some(m) = std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice::<CalendarMonth>(&b).ok()) {
                        return Ok(m);
                    }
                }
            }
            // Only the month itself is fetched; the grid's leading/trailing days
            // from neighbouring months stay empty (fewer TMDB/Trakt requests).
            let (start, end) = month_range(year, month);
            let tmdb = tmdb?;
            let mut items = if use_trakt {
                trakt_items(trakt.as_ref().unwrap(), start, end, tz).await?
            } else {
                tmdb_items(&tmdb, &region, start, end, &mine_ids).await?
            };
            for it in items.iter_mut() {
                if let Some(id) = it.tmdb_id {
                    it.mine |= mine_ids.contains(&(it.title_kind, id));
                }
            }
            items.retain(|i| i.date >= start && i.date <= end);
            attach_posters(&tmdb, &http, &images, &mut items).await;
            items.sort_by(|a, b| a.date.cmp(&b.date).then(b.mine.cmp(&a.mine)).then(a.time.cmp(&b.time)).then(a.name.cmp(&b.name)));
            let month = CalendarMonth { year, month, grid_start: Some(month_grid(year, month).0), items, source: source.into() };
            if let Some(d) = cache.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&cache, serde_json::to_vec(&month).unwrap_or_default());
            Ok(month)
        })
    }

    fn library_tmdb_ids(&self) -> impl Future<Output = HashSet<(Kind, i32)>> + 'static {
        let f = self.library_entries();
        async move { f.await.map(|e| crate::discovery::library_ids(&e)).unwrap_or_default() }
    }
}

async fn trakt_items(t: &rmh_trakt::TraktClient, start: NaiveDate, end: NaiveDate, tz: chrono_tz::Tz) -> ServiceResult<Vec<CalendarItem>> {
    let mut out: Vec<CalendarItem> = vec![];
    let mut seen = HashSet::new();
    // a day of slack on both ends for time-zone shifts; Trakt allows ≤ 33 days per call
    let from = start - chrono::Duration::days(1);
    let jobs: Vec<(bool, CalendarKind)> = vec![
        (true, CalendarKind::Shows),
        (true, CalendarKind::Movies),
        (true, CalendarKind::Dvd),
        (false, CalendarKind::Premieres),
        (false, CalendarKind::NewShows),
        (false, CalendarKind::Movies),
        (false, CalendarKind::Dvd),
    ];
    for (my, kind) in jobs {
        let mut chunk = from;
        while chunk <= end {
            let days = 22;
            let res = t.calendar(my, kind, &chunk.format("%Y-%m-%d").to_string(), days).await;
            chunk += chrono::Duration::days(days as i64);
            let list = match res {
                Ok(l) => l,
                Err(e) if my => return Err(format!("{e:#}")),
                Err(e) => {
                    tracing::warn!("Trakt calendar: {e:#}");
                    continue;
                }
            };
            for e in list {
                let Some((date, time)) = to_local(&e.when, tz) else { continue };
                let rk = match (e.kind, &e.episode) {
                    (CalendarKind::Movies, _) => ReleaseKind::Theatrical,
                    (CalendarKind::Dvd, _) => ReleaseKind::Digital,
                    (CalendarKind::Premieres | CalendarKind::NewShows, _) => ReleaseKind::Premiere,
                    (_, Some((_, 1, _))) => ReleaseKind::Premiere,
                    _ => ReleaseKind::Episode,
                };
                let key = (e.tmdb_id, e.title.clone(), e.episode.as_ref().map(|x| (x.0, x.1)), rk.is_movie(), date);
                if !seen.insert(key) {
                    // already listed from "my" calendars
                    continue;
                }
                let detail = match &e.episode {
                    Some((s, n, Some(name))) => format!("S{s:02}E{n:02} · {name}"),
                    Some((s, n, None)) => format!("S{s:02}E{n:02}"),
                    None => rk.label().to_string(),
                };
                out.push(CalendarItem {
                    date,
                    time: time.filter(|_| !rk.is_movie()),
                    kind: rk,
                    title_kind: if rk.is_movie() { Kind::Movie } else { Kind::Tv },
                    tmdb_id: e.tmdb_id.map(|i| i as i32),
                    name: e.title,
                    detail,
                    mine: my,
                    poster: None,
                    title: None,
                });
            }
        }
    }
    Ok(out)
}

async fn tmdb_items(
    tmdb: &TmdbClient,
    region: &str,
    start: NaiveDate,
    end: NaiveDate,
    mine: &HashSet<(Kind, i32)>,
) -> ServiceResult<Vec<CalendarItem>> {
    let (from, to) = (start.format("%Y-%m-%d").to_string(), end.format("%Y-%m-%d").to_string());
    let mut out = vec![];
    // Movies: theatrical and digital releases in the region, with the regional date
    let mut movies: Vec<(Title, ReleaseKind)> = vec![];
    for (types, rk, pages) in [("2|3", ReleaseKind::Theatrical, 2u32), ("4|5", ReleaseKind::Digital, 2)] {
        for page in 1..=pages {
            match tmdb.movie_releases(region, &from, &to, types, page).await {
                Ok(p) => {
                    let last = p.page >= p.total_pages;
                    movies.extend(p.results.into_iter().map(|t| (t, rk)));
                    if last {
                        break;
                    }
                }
                Err(e) => return Err(format!("{e:#}")),
            }
        }
    }
    let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(CALENDAR_PARALLEL));
    let mut set = JoinSet::new();
    for (t, rk) in movies {
        let tmdb = tmdb.clone();
        let region = region.to_string();
        let permits = permits.clone();
        set.spawn(async move {
            let _p = permits.acquire_owned().await;
            let dates = tmdb.release_dates(t.tmdb_id, &region).await.unwrap_or_default();
            (t, rk, dates)
        });
    }
    let mut seen = HashSet::new();
    while let Some(Ok((t, rk, dates))) = set.join_next().await {
        let wanted: &[u8] = if rk == ReleaseKind::Theatrical { &[3, 2] } else { &[4, 5] };
        let date = dates
            .iter()
            .filter(|r| wanted.contains(&r.kind))
            .filter_map(|r| NaiveDate::parse_from_str(&r.date, "%Y-%m-%d").ok())
            .find(|d| *d >= start && *d <= end);
        let Some(date) = date else { continue };
        if !seen.insert((t.tmdb_id, rk)) {
            continue;
        }
        out.push(CalendarItem {
            date,
            time: None,
            kind: rk,
            title_kind: Kind::Movie,
            tmdb_id: Some(t.tmdb_id),
            name: t.name.clone(),
            detail: rk.label().into(),
            mine: false,
            poster: None,
            title: Some(t),
        });
    }
    // Series: everything on the user's lists, plus the most popular shows airing
    let mut shows: Vec<i32> = mine.iter().filter(|(k, _)| *k == Kind::Tv).map(|(_, id)| *id).collect();
    if let Ok(p) = tmdb.shows_airing(&from, &to, 1).await {
        shows.extend(p.results.iter().take(14).map(|t| t.tmdb_id));
    }
    shows.sort();
    shows.dedup();
    let mut set = JoinSet::new();
    for id in shows {
        let tmdb = tmdb.clone();
        let (from, to) = (from.clone(), to.clone());
        let permits = permits.clone();
        set.spawn(async move {
            let _p = permits.acquire_owned().await;
            let eps = tmdb.episodes_airing(id, &from, &to).await.unwrap_or_default();
            let title = if eps.is_empty() { None } else { tmdb.details(Kind::Tv, id).await.ok().map(|d| d.title) };
            (id, title, eps)
        });
    }
    while let Some(Ok((id, title, eps))) = set.join_next().await {
        let Some(title) = title else { continue };
        for e in eps {
            let Ok(date) = NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") else { continue };
            out.push(CalendarItem {
                date,
                time: None,
                kind: if e.episode == 1 { ReleaseKind::Premiere } else { ReleaseKind::Episode },
                title_kind: Kind::Tv,
                tmdb_id: Some(id),
                name: title.name.clone(),
                detail: format!("S{:02}E{:02} · {}", e.season, e.episode, e.name),
                mine: false,
                poster: None,
                title: Some(title.clone()),
            });
        }
    }
    Ok(out)
}

async fn attach_posters(tmdb: &TmdbClient, http: &reqwest::Client, images: &std::path::Path, items: &mut [CalendarItem]) {
    // titles we don't have yet (Trakt items) – details are cached on disk
    let missing: Vec<(Kind, i32)> = {
        let mut s: Vec<(Kind, i32)> =
            items.iter().filter(|i| i.title.is_none()).filter_map(|i| Some((i.title_kind, i.tmdb_id?))).collect();
        s.sort_by_key(|(k, id)| (k.as_str(), *id));
        s.dedup();
        s.into_iter().take(160).collect()
    };
    let mut titles: HashMap<(Kind, i32), Title> = HashMap::new();
    for chunk in missing.chunks(CALENDAR_PARALLEL) {
        let mut set = JoinSet::new();
        for (k, id) in chunk.iter().copied() {
            let tmdb = tmdb.clone();
            set.spawn(async move { tmdb.details(k, id).await.ok().map(|d| ((k, id), d.title)) });
        }
        while let Some(Ok(r)) = set.join_next().await {
            if let Some((key, t)) = r {
                titles.insert(key, t);
            }
        }
    }
    for it in items.iter_mut().filter(|i| i.title.is_none()) {
        if let Some(id) = it.tmdb_id {
            it.title = titles.get(&(it.title_kind, id)).cloned();
        }
    }
    let mut uniq: Vec<Title> = vec![];
    let mut seen = HashSet::new();
    for t in items.iter().filter_map(|i| i.title.clone()) {
        if seen.insert((t.kind, t.tmdb_id)) {
            uniq.push(t);
        }
    }
    let cards = with_posters(uniq, http, images).await;
    let posters: HashMap<(Kind, i32), PathBuf> =
        cards.into_iter().filter_map(|c| Some(((c.title.kind, c.title.tmdb_id), c.poster?))).collect();
    for it in items.iter_mut() {
        if let Some(t) = &it.title {
            it.poster = posters.get(&(t.kind, t.tmdb_id)).cloned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_and_local_time() {
        let (s, e) = month_grid(2026, 11);
        assert_eq!(s, NaiveDate::from_ymd_opt(2026, 10, 26).unwrap()); // Monday
        assert_eq!((e - s).num_days(), 41);
        assert_eq!(month_range(2026, 11), (NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(), NaiveDate::from_ymd_opt(2026, 11, 30).unwrap()));
        assert_eq!(month_range(2026, 12).1, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
        assert_eq!(month_range(2028, 2).1, NaiveDate::from_ymd_opt(2028, 2, 29).unwrap());
        // 02:00 UTC in November = 03:00 in Prague (CET)
        let (d, t) = to_local("2026-11-20T02:00:00.000Z", chrono_tz::Europe::Prague).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 11, 20).unwrap());
        assert_eq!(t.as_deref(), Some("03:00"));
        // 23:30 UTC in July = next day 01:30 CEST
        let (d, t) = to_local("2026-07-01T23:30:00Z", chrono_tz::Europe::Prague).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 7, 2).unwrap());
        assert_eq!(t.as_deref(), Some("01:30"));
        assert_eq!(to_local("2026-11-21", chrono_tz::Europe::Prague).unwrap().1, None);
    }
}
