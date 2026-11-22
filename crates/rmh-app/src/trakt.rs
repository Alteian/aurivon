//! Trakt.tv: login, scrobbling and two-way watched sync.

use crate::catalog::LibraryEntry;
use crate::service::{MediaService, ServiceResult};
use rmh_database::MediaRepository;
use rmh_trakt::{Entry, Item, PkceLogin, ScrobbleAction, TraktClient};
use std::future::Future;
use tracing::{info, warn};

pub use rmh_trakt::{Entry as TraktEntry, Item as TraktItem, PkceLogin as TraktPkceLogin, ScrobbleAction as TraktAction};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TraktStatus {
    /// Client id is configured
    pub configured: bool,
    pub logged_in: bool,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    /// Local items newly marked watched from Trakt history
    pub pulled: usize,
    /// Local watched items sent to Trakt
    pub pushed: usize,
}

/// Watchlist + continue-watching + catalogs from Trakt
#[derive(Debug, Clone, Default)]
pub struct TraktLists {
    pub watchlist: Vec<Entry>,
    pub playback: Vec<Entry>,
    pub history: Vec<Entry>,
    pub recommendations: Vec<Entry>,
}

impl MediaService {
    pub(crate) fn trakt_client(&self) -> ServiceResult<TraktClient> {
        self.inner
            .trakt
            .read()
            .ok()
            .and_then(|t| t.clone())
            .ok_or_else(|| "Trakt client failed to start.".to_string())
    }

    pub fn trakt_status(&self) -> impl Future<Output = ServiceResult<TraktStatus>> + 'static {
        let client = self.trakt_client().ok();
        self.run(async move {
            Ok(match client {
                None => TraktStatus::default(),
                Some(c) => TraktStatus { configured: true, logged_in: c.is_logged_in().await, username: c.username().await },
            })
        })
    }

    /// Step 1: Authorization Code + PKCE. Open [`PkceLogin::auth_url`] in a browser.
    pub fn trakt_begin_login(&self) -> impl Future<Output = ServiceResult<PkceLogin>> + 'static {
        let client = self.trakt_client();
        self.run(async move { client?.start_pkce_login().await.map_err(|e| format!("{e:#}")) })
    }

    /// Step 2: wait for the HTTPS callback page to forward the code to the local listener.
    pub fn trakt_finish_login(&self) -> impl Future<Output = ServiceResult<Option<String>>> + 'static {
        let client = self.trakt_client();
        self.run(async move { client?.wait_for_pkce_login().await.map_err(|e| format!("{e:#}")) })
    }

    /// Paste the `code` from the callback page if the automatic forward failed.
    pub fn trakt_complete_login(&self, code: String) -> impl Future<Output = ServiceResult<Option<String>>> + 'static {
        let client = self.trakt_client();
        self.run(async move { client?.complete_pkce_login(&code).await.map_err(|e| format!("{e:#}")) })
    }

    pub fn trakt_logout(&self) -> impl Future<Output = ServiceResult<()>> + 'static {
        let client = self.trakt_client();
        self.run(async move { client?.logout().await.map_err(|e| format!("{e:#}")) })
    }

    /// Fire-and-forget scrobble (no-op when Trakt is off or not logged in)
    pub fn trakt_scrobble(&self, action: ScrobbleAction, item: Item, progress: f64) {
        if !self.config().trakt_scrobble {
            return;
        }
        let Ok(client) = self.trakt_client() else { return };
        self.inner.rt.spawn(async move {
            if !client.is_logged_in().await {
                return;
            }
            match client.scrobble(action, &item, progress).await {
                Ok(()) => info!("Trakt scrobble {action:?} {:.0}%", progress),
                Err(e) => warn!("Trakt scrobble failed: {e:#}"),
            }
        });
    }

    /// Push a single watched item to Trakt history immediately (mark-watched path).
    pub fn trakt_push_watched(&self, item: Item) {
        let Ok(client) = self.trakt_client() else { return };
        self.inner.rt.spawn(async move {
            if !client.is_logged_in().await {
                return;
            }
            match client.add_to_history(&[item]).await {
                Ok(()) => info!("Trakt history push ok"),
                Err(e) => warn!("Trakt history push failed: {e:#}"),
            }
        });
    }

    pub fn trakt_lists(&self) -> impl Future<Output = ServiceResult<TraktLists>> + 'static {
        let client = self.trakt_client();
        let catalogs = self.config().enable_trakt_catalogs;
        self.run(async move {
            let c = client?;
            if !c.is_logged_in().await {
                return Ok(TraktLists::default());
            }
            let watchlist = c.watchlist().await.map_err(|e| format!("{e:#}"))?;
            let playback = c.playback().await.map_err(|e| format!("{e:#}"))?;
            let (history, recommendations) = if catalogs {
                (
                    c.history(40).await.unwrap_or_default(),
                    c.recommendations(20).await.unwrap_or_default(),
                )
            } else {
                (vec![], vec![])
            };
            Ok(TraktLists { watchlist, playback, history, recommendations })
        })
    }

    pub fn trakt_add_to_watchlist(&self, kind: crate::catalog::TitleKind, tmdb_id: i32) -> impl Future<Output = ServiceResult<()>> + 'static {
        let client = self.trakt_client();
        self.run(async move {
            let c = client?;
            let r = match kind {
                rmh_metadata::Kind::Movie => c.add_to_watchlist(&Item::Movie { tmdb: tmdb_id as i64 }).await,
                rmh_metadata::Kind::Tv => c.add_show_to_watchlist(tmdb_id as i64).await,
            };
            r.map_err(|e| format!("{e:#}"))
        })
    }

    pub fn trakt_remove_from_watchlist(&self, kind: crate::catalog::TitleKind, tmdb_id: i32) -> impl Future<Output = ServiceResult<()>> + 'static {
        let client = self.trakt_client();
        self.run(async move {
            let c = client?;
            let r = match kind {
                rmh_metadata::Kind::Movie => c.remove_from_watchlist(&Item::Movie { tmdb: tmdb_id as i64 }).await,
                rmh_metadata::Kind::Tv => c.remove_show_from_watchlist(tmdb_id as i64).await,
            };
            r.map_err(|e| format!("{e:#}"))
        })
    }

    /// Two-way watched sync between the local library and Trakt
    pub fn trakt_sync(&self) -> impl Future<Output = ServiceResult<SyncReport>> + 'static {
        let client = self.trakt_client();
        let entries = self.library_entries();
        let db = self.db();
        let svc = self.clone();
        self.run(async move {
            let c = client?;
            if !c.is_logged_in().await {
                return Err("Log in to Trakt first".into());
            }
            let entries: Vec<LibraryEntry> = entries.await?;
            let db = db.await?;
            let repo = MediaRepository::new(db.pool());
            let watched = c.watched().await.map_err(|e| format!("{e:#}"))?;
            let mut report = SyncReport::default();
            let mut push: Vec<(uuid::Uuid, Item)> = vec![];
            for e in &entries {
                let Some(item) = e.trakt_item() else { continue };
                let on_trakt = match &item {
                    Item::Movie { tmdb } => watched.movies.contains(tmdb),
                    Item::Episode { show_tmdb, season, number } => watched.episodes.contains(&(*show_tmdb, *season, *number)),
                };
                if on_trakt && !e.item.watched {
                    repo.update_watched_status(e.item.id, true).await.map_err(|e| format!("{e:#}"))?;
                    let _ = repo.mark_trakt_synced(e.item.id).await;
                    report.pulled += 1;
                } else if e.item.watched && !on_trakt && e.link.as_ref().is_some_and(|l| l.trakt_synced_at.is_none()) {
                    push.push((e.item.id, item));
                }
            }
            if !push.is_empty() {
                let items: Vec<Item> = push.iter().map(|(_, i)| i.clone()).collect();
                c.add_to_history(&items).await.map_err(|e| format!("{e:#}"))?;
                for (id, _) in &push {
                    let _ = repo.mark_trakt_synced(*id).await;
                }
                report.pushed = push.len();
            }
            let mut cfg = svc.config();
            cfg.trakt_watched_synced_at = Some(chrono::Utc::now().timestamp());
            let _ = svc.save_config(cfg);
            info!("Trakt sync: pulled {}, pushed {}", report.pulled, report.pushed);
            Ok(report)
        })
    }
}
