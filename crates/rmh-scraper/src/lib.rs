//! Scraper addon for Aurivon
//!
//! This crate provides scraping capabilities to find streams from various sources:
//! - Torrent sites (1337x, RARBG-style indexers, etc.)
//! - Real-Debrid for instant cached streaming
//! - Direct hosters
//!
//! ## How it works
//!
//! 1. User searches for a movie/show by IMDB ID or title
//! 2. Scraper searches multiple torrent indexers
//! 3. Found magnets are checked against Real-Debrid cache
//! 4. Cached torrents return instant direct links
//! 5. Uncached torrents can be added to RD or streamed via rmh-torrent

mod error;
mod debrid;
mod scrapers;
mod cache;
mod parser;
mod addon;

pub use error::{ScraperError, ScraperResult};
pub use debrid::{CacheProbe, RealDebrid, RealDebridConfig, DebridStream};
pub use scrapers::{Scraper, ScraperManager, ScrapedResult, SearchQuery, human_readable_size};
pub use cache::ScraperCache;
pub use parser::{TorrentInfo, parse_torrent_title};
pub use addon::{ScraperAddon, ScraperAddonConfig, create_scraper_addon};
