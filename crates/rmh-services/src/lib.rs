//! Third-party services beyond TMDB/Trakt:
//!
//! - [`opensubtitles`]: search by file hash + TMDB id, language preference, download
//! - [`simkl`]: PIN login, scrobble, history, watchlist ("plan to watch")
//! - [`anilist`]: token login, progress / completed / planning for anime
//!
//! Secrets (API keys, passwords, tokens) are never logged; tokens are stored
//! with mode 0600 next to the app config.

pub mod anilist;
pub mod opensubtitles;
pub mod simkl;
mod store;

pub use store::Secret;

pub(crate) fn http() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("Aurivon v", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}
