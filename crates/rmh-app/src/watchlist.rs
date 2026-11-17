//! Local watchlist (works offline / without accounts); additions are also
//! pushed to Trakt, Simkl and AniList when those are connected.

use crate::catalog::{with_posters, TitleCard};
use crate::service::{MediaService, ServiceResult};
use rmh_metadata::{Kind, Title};
use serde::{Deserialize, Serialize};
use std::future::Future;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchItem {
    pub title: Title,
    /// Unix seconds
    pub added: i64,
}

impl MediaService {
    fn watchlist_path(&self) -> std::path::PathBuf {
        self.inner.data_dir.join("watchlist.json")
    }

    pub fn watchlist(&self) -> Vec<WatchItem> {
        std::fs::read_to_string(self.watchlist_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
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
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            list.insert(0, WatchItem { title: title.clone(), added: now });
        }
        if let Some(d) = self.watchlist_path().parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(self.watchlist_path(), serde_json::to_vec_pretty(&list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if added {
            // mirror to connected services (best effort)
            let fut = self.trakt_add_to_watchlist(title.kind, title.tmdb_id);
            self.inner.rt.spawn(async move {
                let _ = fut.await;
            });
            self.social_watchlist_add(title.kind, title.tmdb_id);
        }
        Ok(added)
    }

    pub fn watchlist_cards(&self) -> impl Future<Output = ServiceResult<Vec<TitleCard>>> + 'static {
        let titles: Vec<Title> = self.watchlist().into_iter().map(|w| w.title).collect();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        self.run(async move { Ok(with_posters(titles, &http, &images).await) })
    }
}
