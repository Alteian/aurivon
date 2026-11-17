//! OpenSubtitles search & download for what's playing.

use crate::service::{MediaService, ServiceResult};
use rmh_services::opensubtitles::{SubQuery, Subtitle};
use std::future::Future;
use std::path::PathBuf;

pub use rmh_services::opensubtitles::Subtitle as SubtitleHit;

/// What is playing, as far as subtitle search is concerned
#[derive(Debug, Clone, Default)]
pub struct SubContext {
    pub file: Option<PathBuf>,
    pub tmdb_id: Option<i32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub query: Option<String>,
}

impl MediaService {
    pub fn has_opensubtitles(&self) -> bool {
        self.inner.opensubtitles.read().map(|o| o.is_some()).unwrap_or(false)
    }

    /// Search, best first (preferred languages, exact file hash, downloads)
    pub fn subtitle_search(&self, ctx: SubContext) -> impl Future<Output = ServiceResult<Vec<Subtitle>>> + 'static {
        let os = self.inner.opensubtitles.read().ok().and_then(|o| o.clone());
        let languages = self.config().subtitle_languages();
        self.run(async move {
            let os = os.ok_or("OpenSubtitles is not set up: add an API key in Settings → Services.")?;
            let q = SubQuery {
                file: ctx.file.filter(|f| f.exists()),
                tmdb_id: ctx.tmdb_id,
                season: ctx.season,
                episode: ctx.episode,
                query: ctx.query,
                languages,
            };
            os.search(&q).await.map_err(|e| format!("{e:#}"))
        })
    }

    pub fn subtitle_download(&self, sub: Subtitle) -> impl Future<Output = ServiceResult<PathBuf>> + 'static {
        let os = self.inner.opensubtitles.read().ok().and_then(|o| o.clone());
        let dir = self.inner.subs_dir.clone();
        self.run(async move {
            let os = os.ok_or("OpenSubtitles is not set up")?;
            os.download(&sub, &dir).await.map_err(|e| format!("{e:#}"))
        })
    }
}
