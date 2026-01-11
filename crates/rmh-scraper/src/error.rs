//! Error types for the scraper

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScraperError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    
    #[error("Failed to parse response: {0}")]
    Parse(String),
    
    #[error("Real-Debrid API error: {0}")]
    RealDebrid(String),
    
    #[error("No results found")]
    NoResults,
    
    #[error("Rate limited, retry after {0} seconds")]
    RateLimited(u64),
    
    #[error("Invalid API key")]
    InvalidApiKey,
    
    #[error("Torrent not cached on Real-Debrid")]
    NotCached,
    
    #[error("Scraper {0} is unavailable")]
    ScraperUnavailable(String),
    
    #[error("Invalid magnet link: {0}")]
    InvalidMagnet(String),
    
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type ScraperResult<T> = Result<T, ScraperError>;
