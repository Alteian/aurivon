//! Local watchlist synced both ways with Trakt (and mirrored to Simkl / AniList on add).

use crate::catalog::{with_posters, TitleCard};
use crate::config::TraktWatchlistPolicy;
use crate::service::{MediaService, ServiceResult};
use rmh_metadata::{Kind, Title};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::future::Future;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchItem {
    pub title: Title,
    /// Unix seconds
    pub added: i64,
}

#[derive(Debug, Clone, Default)]
pub struct WatchlistSyncReport {
    pub added_local: usize,
    pub removed_local: usize,
    pub pushed: usize,
    pub removed_remote: usize,
}

impl MediaService {
    fn watchlist_path(&self) -> std::path::PathBuf {
        self.inner.data_dir.join("watchlist.json")
    }

    pub fn watchlist(&self) -> Vec<WatchItem> {
        std::fs::read_to_string(self.watchlist_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_watchlist(&self, list: &[WatchItem]) -> ServiceResult<()> {
        if let Some(d) = self.watchlist_path().parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(self.watchlist_path(), serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }

    pub fn in_watchlist(&self, kind: Kind, tmdb_id: i32) -> bool {
        self.watchlist().iter().any(|w| w.title.kind == kind && w.title.tmdb_id == tmdb_id)
    }

    /// Add or remove; returns true when the title is now on the list
    pub fn toggle_watchlist(&self, title: &Title) -> ServiceResult<bool> {
        let mut list = self.watchlist();
        let before = list.len();
        list.retain(|w| !(w.title.kind == title.kind && w.title.tmdb_id == title.tmdb_id));
        let added = list.len() == before;
        if added {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            list.insert(0, WatchItem { title: title.clone(), added: now });
        }
        self.save_watchlist(&list)?;
        if added {
            let fut = self.trakt_add_to_watchlist(title.kind, title.tmdb_id);
            self.inner.rt.spawn(async move {
                let _ = fut.await;
            });
            self.social_watchlist_add(title.kind, title.tmdb_id);
        } else {
            let fut = self.trakt_remove_from_watchlist(title.kind, title.tmdb_id);
            self.inner.rt.spawn(async move {
                let _ = fut.await;
            });
        }
        Ok(added)
    }

    pub fn remove_from_watchlist(&self, kind: Kind, tmdb_id: i32) -> ServiceResult<()> {
        let mut list = self.watchlist();
        let before = list.len();
        list.retain(|w| !(w.title.kind == kind && w.title.tmdb_id == tmdb_id));
        if list.len() == before {
            return Ok(());
        }
        self.save_watchlist(&list)?;
        let fut = self.trakt_remove_from_watchlist(kind, tmdb_id);
        self.inner.rt.spawn(async move {
            let _ = fut.await;
        });
        Ok(())
    }

    pub fn watchlist_cards(&self) -> impl Future<Output = ServiceResult<Vec<TitleCard>>> + 'static {
        let titles: Vec<Title> = self.watchlist().into_iter().map(|w| w.title).collect();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        self.run(async move { Ok(with_posters(titles, &http, &images).await) })
    }

    /// Merge local watchlist with Trakt according to the configured policy
    pub fn sync_watchlist(&self) -> impl Future<Output = ServiceResult<WatchlistSyncReport>> + 'static {
        let client = self.trakt_client();
        let policy = self.config().trakt_watchlist_policy;
        let local = self.watchlist();
        let tmdb = self.tmdb();
        let svc = self.clone();
        self.run(async move {
            let c = client?;
            if !c.is_logged_in().await {
                return Err("Log in to Trakt first".into());
            }
            let remote = c.watchlist().await.map_err(|e| format!("{e:#}"))?;
            let mut report = WatchlistSyncReport::default();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let local_keys: HashSet<(Kind, i32)> = local.iter().map(|w| (w.title.kind, w.title.tmdb_id)).collect();
            let mut remote_keys: HashSet<(Kind, i32)> = HashSet::new();
            let mut remote_titles: Vec<Title> = vec![];

            for e in &remote {
                let (kind, id, year, poster) = match (&e.movie, &e.show) {
                    (Some(m), _) => (
                        Kind::Movie,
                        m.ids.tmdb.unwrap_or(0) as i32,
                        m.year.map(|y| y as u16),
                        None::<String>,
                    ),
                    (_, Some(s)) => (Kind::Tv, s.ids.tmdb.unwrap_or(0) as i32, s.year.map(|y| y as u16), None),
                    _ => continue,
                };
                if id == 0 {
                    continue;
                }
                remote_keys.insert((kind, id));
                let mut title = Title {
                    kind,
                    tmdb_id: id,
                    name: e.label(),
                    year,
                    overview: None,
                    poster_path: poster,
                    backdrop_path: None,
                    rating: None,
                    genre_ids: vec![],
                    original_language: None,
                    votes: None,
                };
                if let Ok(client) = &tmdb {
                    if let Ok(d) = client.details(kind, id).await {
                        title = d.title;
                    }
                }
                remote_titles.push(title);
            }

            let mut next = local.clone();

            match policy {
                TraktWatchlistPolicy::TraktWins => {
                    report.removed_local = next.len();
                    next.clear();
                    for t in remote_titles {
                        next.push(WatchItem { title: t, added: now });
                        report.added_local += 1;
                    }
                }
                TraktWatchlistPolicy::LocalWins => {
                    // Push local-only to Trakt; do not pull
                    for w in &local {
                        if !remote_keys.contains(&(w.title.kind, w.title.tmdb_id)) {
                            let _ = match w.title.kind {
                                Kind::Movie => c.add_to_watchlist(&rmh_trakt::Item::Movie { tmdb: w.title.tmdb_id as i64 }).await,
                                Kind::Tv => c.add_show_to_watchlist(w.title.tmdb_id as i64).await,
                            };
                            report.pushed += 1;
                        }
                    }
                }
                TraktWatchlistPolicy::Merge => {
                    for t in remote_titles {
                        let key = (t.kind, t.tmdb_id);
                        if !local_keys.contains(&key) {
                            next.insert(0, WatchItem { title: t, added: now });
                            report.added_local += 1;
                        }
                    }
                    for w in &local {
                        if !remote_keys.contains(&(w.title.kind, w.title.tmdb_id)) {
                            let _ = match w.title.kind {
                                Kind::Movie => c.add_to_watchlist(&rmh_trakt::Item::Movie { tmdb: w.title.tmdb_id as i64 }).await,
                                Kind::Tv => c.add_show_to_watchlist(w.title.tmdb_id as i64).await,
                            };
                            report.pushed += 1;
                        }
                    }
                }
            }

            // Dedup
            let mut seen = HashSet::new();
            next.retain(|w| seen.insert((w.title.kind, w.title.tmdb_id)));
            svc.save_watchlist(&next)?;

            let mut cfg = svc.config();
            cfg.trakt_watchlist_synced_at = Some(now);
            let _ = svc.save_config(cfg);

            info!(
                "Watchlist sync: +{} local, −{} local, {} pushed",
                report.added_local, report.removed_local, report.pushed
            );
            Ok(report)
        })
    }
}
