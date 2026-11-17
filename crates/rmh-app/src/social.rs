//! Simkl and AniList: login, scrobbling / progress, history and watchlist.

use crate::service::{MediaService, ServiceResult};
use rmh_metadata::Kind;
use rmh_services::anilist::{self, AniListClient};
use rmh_services::simkl::{self, Pin, SimklClient};
use std::future::Future;
use tracing::{info, warn};

pub use rmh_services::simkl::Pin as SimklPin;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServiceStatus {
    pub configured: bool,
    pub logged_in: bool,
    pub username: Option<String>,
}

/// What finished / is playing, for Simkl & AniList
#[derive(Debug, Clone)]
pub struct WatchEvent {
    pub kind: Kind,
    pub tmdb_id: i32,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

impl WatchEvent {
    fn simkl_item(&self) -> simkl::Item {
        match (self.kind, self.season, self.episode) {
            (Kind::Tv, Some(s), Some(e)) => simkl::Item::Episode { show_tmdb: self.tmdb_id, season: s, number: e },
            (Kind::Tv, _, _) => simkl::Item::Show { tmdb: self.tmdb_id },
            (Kind::Movie, _, _) => simkl::Item::Movie { tmdb: self.tmdb_id },
        }
    }
}

impl MediaService {
    pub(crate) fn simkl(&self) -> ServiceResult<SimklClient> {
        let id = self.config().simkl_id().ok_or("Simkl is not set up: add your Simkl client id in Settings.")?;
        SimklClient::new(id, self.inner.data_dir.join("simkl.json")).map_err(|e| format!("{e:#}"))
    }

    pub(crate) fn anilist(&self) -> ServiceResult<AniListClient> {
        AniListClient::new(self.inner.data_dir.join("anilist.json")).map_err(|e| format!("{e:#}"))
    }

    pub fn simkl_status(&self) -> impl Future<Output = ServiceResult<ServiceStatus>> + 'static {
        let c = self.simkl().ok();
        self.run(async move {
            Ok(match c {
                None => ServiceStatus::default(),
                Some(c) if !c.is_logged_in() => ServiceStatus { configured: true, ..Default::default() },
                Some(c) => ServiceStatus { configured: true, logged_in: true, username: c.username().await.ok() },
            })
        })
    }

    pub fn simkl_begin_login(&self) -> impl Future<Output = ServiceResult<Pin>> + 'static {
        let c = self.simkl();
        self.run(async move { c?.start_login().await.map_err(|e| format!("{e:#}")) })
    }

    pub fn simkl_finish_login(&self, pin: Pin) -> impl Future<Output = ServiceResult<()>> + 'static {
        let c = self.simkl();
        self.run(async move { c?.wait_for_login(&pin).await.map_err(|e| format!("{e:#}")) })
    }

    pub fn simkl_logout(&self) {
        if let Ok(c) = self.simkl() {
            c.logout();
        }
    }

    pub fn anilist_status(&self) -> impl Future<Output = ServiceResult<ServiceStatus>> + 'static {
        let configured = self.config().anilist_id().is_some();
        let c = self.anilist();
        self.run(async move {
            let c = c?;
            if !c.is_logged_in() {
                return Ok(ServiceStatus { configured, ..Default::default() });
            }
            Ok(ServiceStatus { configured: true, logged_in: true, username: c.username().await.ok() })
        })
    }

    /// Page to open in the browser to get a token (needs the AniList client id)
    pub fn anilist_auth_url(&self) -> ServiceResult<String> {
        let id = self.config().anilist_id().ok_or("Add your AniList client id in Settings first.")?;
        Ok(anilist::authorize_url(&id))
    }

    pub fn anilist_set_token(&self, token: String) -> impl Future<Output = ServiceResult<String>> + 'static {
        let c = self.anilist();
        self.run(async move { c?.set_token(&token).await.map_err(|e| format!("{e:#}")) })
    }

    pub fn anilist_logout(&self) {
        if let Ok(c) = self.anilist() {
            c.logout();
        }
    }

    /// Fire-and-forget: Simkl scrobble + AniList progress (anime only)
    pub fn social_scrobble(&self, action: simkl::Scrobble, ev: WatchEvent, progress: f64) {
        let cfg = self.config();
        if cfg.simkl_scrobble {
            if let Ok(c) = self.simkl() {
                if c.is_logged_in() {
                    let item = ev.simkl_item();
                    self.inner.rt.spawn(async move {
                        match c.scrobble(action, item, progress).await {
                            Ok(()) => info!("Simkl scrobble {action:?} {progress:.0}%"),
                            Err(e) => warn!("Simkl scrobble failed: {e:#}"),
                        }
                    });
                }
            }
        }
        let finished = matches!(action, simkl::Scrobble::Stop) && progress >= 80.0;
        if finished && cfg.anilist_sync {
            self.anilist_progress(ev, anilist::Status::Completed);
        }
    }

    fn anilist_progress(&self, ev: WatchEvent, movie_status: anilist::Status) {
        let Ok(c) = self.anilist() else { return };
        if !c.is_logged_in() {
            return;
        }
        let Ok(tmdb) = self.tmdb() else { return };
        self.inner.rt.spawn(async move {
            let Ok(d) = tmdb.details(ev.kind, ev.tmdb_id).await else { return };
            if !anilist::looks_like_anime(d.title.original_language.as_deref(), &d.title.genre_ids) {
                return;
            }
            let Ok(Some(anime)) = c.find(&d.title.name, d.title.year).await else {
                warn!("AniList: no match for {}", d.title.name);
                return;
            };
            let r = match (ev.kind, ev.episode) {
                (Kind::Tv, Some(n)) if movie_status == anilist::Status::Completed => c.watched_episode(&anime, n).await,
                _ => c.save(anime.id, movie_status, None).await,
            };
            match r {
                Ok(()) => info!("AniList updated: {}", anime.title),
                Err(e) => warn!("AniList update failed: {e:#}"),
            }
        });
    }

    /// Add to Simkl "plan to watch" and AniList "planning" (anime), if connected
    pub(crate) fn social_watchlist_add(&self, kind: Kind, tmdb_id: i32) {
        if let Ok(c) = self.simkl() {
            if c.is_logged_in() {
                let item = if kind == Kind::Movie { simkl::Item::Movie { tmdb: tmdb_id } } else { simkl::Item::Show { tmdb: tmdb_id } };
                self.inner.rt.spawn(async move {
                    if let Err(e) = c.add_to_watchlist(&[item]).await {
                        warn!("Simkl watchlist failed: {e:#}");
                    }
                });
            }
        }
        if self.config().anilist_sync {
            self.anilist_progress(WatchEvent { kind, tmdb_id, season: None, episode: None }, anilist::Status::Planning);
        }
    }
}
