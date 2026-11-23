//! Trakt custom lists + local cache for the Lists / Watchlist hub.

use crate::catalog::{with_posters, TitleCard};
use crate::service::{MediaService, ServiceResult};
use rmh_metadata::{Kind, Title};
use rmh_trakt::{Entry, UserList};
use serde::{Deserialize, Serialize};
use std::future::Future;
use tracing::info;

/// Which list chip is selected on the Lists page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListScope {
    /// Union of watchlist + every custom list (deduped by TMDB id)
    All,
    /// Sync watchlist only
    Watchlist,
    /// Custom list by Trakt list id (slug or numeric id string)
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListTypeFilter {
    All,
    Movies,
    Shows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSort {
    Added,
    Title,
    Year,
    Rank,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListsCache {
    pub lists: Vec<UserList>,
    /// list_id → entries
    pub items: std::collections::HashMap<String, Vec<Entry>>,
    pub fetched_at: i64,
}

impl MediaService {
    fn lists_cache_path(&self) -> std::path::PathBuf {
        self.inner.data_dir.join("trakt_lists.json")
    }

    pub fn lists_cache(&self) -> ListsCache {
        std::fs::read_to_string(self.lists_cache_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_lists_cache(&self, cache: &ListsCache) -> ServiceResult<()> {
        if let Some(d) = self.lists_cache_path().parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            self.lists_cache_path(),
            serde_json::to_vec_pretty(cache).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    /// Refresh custom list metadata + items from Trakt.
    pub fn refresh_trakt_lists(&self) -> impl Future<Output = ServiceResult<ListsCache>> + 'static {
        let client = self.trakt_client();
        let svc = self.clone();
        self.run(async move {
            let c = client?;
            if !c.is_logged_in().await {
                return Err("Log in to Trakt first".into());
            }
            let lists = c.user_lists().await.map_err(|e| format!("{e:#}"))?;
            let mut items = std::collections::HashMap::new();
            for list in &lists {
                let id = list
                    .ids
                    .slug
                    .clone()
                    .or_else(|| list.ids.trakt.map(|t| t.to_string()))
                    .unwrap_or_else(|| list.name.clone());
                match c.list_items(&id).await {
                    Ok(entries) => {
                        items.insert(id, entries);
                    }
                    Err(e) => tracing::warn!("list items {id}: {e:#}"),
                }
            }
            let cache = ListsCache {
                lists,
                items,
                fetched_at: chrono::Utc::now().timestamp(),
            };
            svc.save_lists_cache(&cache)?;
            info!("Trakt lists: {} lists cached", cache.lists.len());
            Ok(cache)
        })
    }

    pub fn create_trakt_list(&self, name: String, description: Option<String>) -> impl Future<Output = ServiceResult<UserList>> + 'static {
        let client = self.trakt_client();
        let svc = self.clone();
        self.run(async move {
            let c = client?;
            let list = c
                .create_list(&name, description.as_deref())
                .await
                .map_err(|e| format!("{e:#}"))?;
            let _ = svc.refresh_trakt_lists().await;
            Ok(list)
        })
    }

    pub fn rename_trakt_list(
        &self,
        list_id: String,
        name: String,
        description: Option<String>,
    ) -> impl Future<Output = ServiceResult<UserList>> + 'static {
        let client = self.trakt_client();
        let svc = self.clone();
        self.run(async move {
            let c = client?;
            let list = c
                .update_list(&list_id, &name, description.as_deref())
                .await
                .map_err(|e| format!("{e:#}"))?;
            let _ = svc.refresh_trakt_lists().await;
            Ok(list)
        })
    }

    pub fn delete_trakt_list(&self, list_id: String) -> impl Future<Output = ServiceResult<()>> + 'static {
        let client = self.trakt_client();
        let svc = self.clone();
        self.run(async move {
            let c = client?;
            c.delete_list(&list_id).await.map_err(|e| format!("{e:#}"))?;
            let _ = svc.refresh_trakt_lists().await;
            Ok(())
        })
    }

    pub fn add_to_trakt_list(
        &self,
        list_id: String,
        kind: Kind,
        tmdb_id: i32,
    ) -> impl Future<Output = ServiceResult<()>> + 'static {
        let client = self.trakt_client();
        let svc = self.clone();
        self.run(async move {
            let c = client?;
            let (movies, shows) = match kind {
                Kind::Movie => (vec![tmdb_id as i64], vec![]),
                Kind::Tv => (vec![], vec![tmdb_id as i64]),
            };
            c.add_to_list(&list_id, &movies, &shows)
                .await
                .map_err(|e| format!("{e:#}"))?;
            let _ = svc.refresh_trakt_lists().await;
            Ok(())
        })
    }

    pub fn remove_from_trakt_list(
        &self,
        list_id: String,
        kind: Kind,
        tmdb_id: i32,
    ) -> impl Future<Output = ServiceResult<()>> + 'static {
        let client = self.trakt_client();
        let svc = self.clone();
        self.run(async move {
            let c = client?;
            let (movies, shows) = match kind {
                Kind::Movie => (vec![tmdb_id as i64], vec![]),
                Kind::Tv => (vec![], vec![tmdb_id as i64]),
            };
            c.remove_from_list(&list_id, &movies, &shows)
                .await
                .map_err(|e| format!("{e:#}"))?;
            let _ = svc.refresh_trakt_lists().await;
            Ok(())
        })
    }

    /// Cards for the Lists hub given scope / filters.
    pub fn list_hub_cards(
        &self,
        scope: ListScope,
        type_filter: ListTypeFilter,
        sort: ListSort,
        query: String,
    ) -> impl Future<Output = ServiceResult<Vec<TitleCard>>> + 'static {
        let cache = self.lists_cache();
        let watch = self.watchlist();
        let http = self.inner.http.clone();
        let images = self.inner.image_dir.clone();
        let q = query.trim().to_lowercase();
        self.run(async move {
            let mut titles: Vec<(Title, Option<i32>, Option<String>)> = Vec::new();

            let push_entry = |titles: &mut Vec<_>, e: &Entry| {
                let Some(tmdb) = e.tmdb_id() else { return };
                let kind = if e.movie.is_some() { Kind::Movie } else { Kind::Tv };
                let title = Title {
                    kind,
                    tmdb_id: tmdb as i32,
                    name: e
                        .movie
                        .as_ref()
                        .or(e.show.as_ref())
                        .and_then(|m| m.title.clone())
                        .unwrap_or_default(),
                    year: e
                        .movie
                        .as_ref()
                        .or(e.show.as_ref())
                        .and_then(|m| m.year)
                        .map(|y| y as u16),
                    overview: None,
                    poster_path: None,
                    backdrop_path: None,
                    rating: None,
                    genre_ids: Vec::new(),
                    original_language: None,
                    votes: None,
                };
                titles.push((title, e.rank, e.listed_at.clone()));
            };

            match &scope {
                ListScope::Watchlist => {
                    for w in &watch {
                        titles.push((w.title.clone(), None, Some(w.added.to_string())));
                    }
                }
                ListScope::Custom(id) => {
                    if let Some(entries) = cache.items.get(id) {
                        for e in entries {
                            push_entry(&mut titles, e);
                        }
                    }
                }
                ListScope::All => {
                    for w in &watch {
                        titles.push((w.title.clone(), None, Some(w.added.to_string())));
                    }
                    for entries in cache.items.values() {
                        for e in entries {
                            push_entry(&mut titles, e);
                        }
                    }
                    // dedupe by (kind, tmdb)
                    let mut seen = std::collections::HashSet::new();
                    titles.retain(|(t, _, _)| seen.insert((t.kind, t.tmdb_id)));
                }
            }

            titles.retain(|(t, _, _)| match type_filter {
                ListTypeFilter::All => true,
                ListTypeFilter::Movies => t.kind == Kind::Movie,
                ListTypeFilter::Shows => t.kind == Kind::Tv,
            });
            if !q.is_empty() {
                titles.retain(|(t, _, _)| t.name.to_lowercase().contains(&q));
            }
            match sort {
                ListSort::Title => titles.sort_by(|a, b| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase())),
                ListSort::Year => titles.sort_by(|a, b| b.0.year.cmp(&a.0.year)),
                ListSort::Rank => titles.sort_by(|a, b| a.1.cmp(&b.1)),
                ListSort::Added => titles.sort_by(|a, b| b.2.cmp(&a.2)),
            }

            let only: Vec<Title> = titles.into_iter().map(|(t, _, _)| t).collect();
            Ok(with_posters(only, &http, &images).await)
        })
    }
}
