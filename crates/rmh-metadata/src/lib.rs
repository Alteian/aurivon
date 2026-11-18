//! TMDB metadata fetching for Aurivon

use anyhow::{Context, Result};
use reqwest::Client;
use rmh_database::{Database, MediaRepository};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

pub mod browse;
pub mod catalog;
pub use browse::{
    Airing, CastMember, Credit, DiscoverQuery, Release, Extras, Genre, ListKind, Person, PersonCard, Provider, ResultPage, SearchResults, Sort, Video,
};
pub use catalog::{cache_image, cached_image_path, parse_release, Details, Episode, Kind, ParsedName, SeasonSummary, Title};

pub(crate) const TMDB_BASE_URL: &str = "https://api.themoviedb.org/3";
const TMDB_IMAGE_BASE: &str = "https://image.tmdb.org/t/p";

/// TMDB API client
#[derive(Clone)]
pub struct TmdbClient {
    client: Client,
    api_key: String,
    /// Optional on-disk cache for JSON responses (see `with_cache`)
    pub(crate) cache_dir: Option<std::path::PathBuf>,
}

impl TmdbClient {
    /// Create a new TMDB client with the given API key
    pub fn new(api_key: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self { client, api_key, cache_dir: None })
    }

    /// Search for a movie by title
    pub async fn search_movie(&self, query: &str) -> Result<Vec<TmdbMovieResult>> {
        info!("TMDB: Searching for movie: {}", query);
        let url = format!("{}/search/movie", TMDB_BASE_URL);
        
        let response = self
            .client
            .get(&url)
            .query(&[
                ("api_key", self.api_key.as_str()),
                ("query", query),
                ("language", "en-US"),
                ("page", "1"),
            ])
            .send()
            .await
            .context("Failed to search TMDB")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            warn!("TMDB API error: {} - {}", status, text);
            anyhow::bail!("TMDB API error: {} - {}", status, text);
        }

        let result: TmdbSearchResponse = response
            .json()
            .await
            .context("Failed to parse TMDB response")?;

        debug!("Found {} results for query: {}", result.results.len(), query);
        Ok(result.results)
    }

    /// Search for a TV show by title
    pub async fn search_tv(&self, query: &str) -> Result<Vec<TmdbTvResult>> {
        let url = format!("{}/search/tv", TMDB_BASE_URL);
        
        let response = self
            .client
            .get(&url)
            .query(&[
                ("api_key", self.api_key.as_str()),
                ("query", query),
                ("language", "en-US"),
                ("page", "1"),
            ])
            .send()
            .await
            .context("Failed to search TMDB for TV")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("TMDB API error: {} - {}", status, text);
        }

        let result: TmdbTvSearchResponse = response
            .json()
            .await
            .context("Failed to parse TMDB TV response")?;

        debug!("Found {} TV results for query: {}", result.results.len(), query);
        Ok(result.results)
    }

    /// Get detailed movie information by TMDB ID
    pub async fn get_movie_details(&self, tmdb_id: i32) -> Result<TmdbMovieDetails> {
        let url = format!("{}/movie/{}", TMDB_BASE_URL, tmdb_id);

        let response = self
            .client
            .get(&url)
            .query(&[
                ("api_key", self.api_key.as_str()),
                ("language", "en-US"),
            ])
            .send()
            .await
            .context("Failed to get movie details")?;

        if !response.status().is_success() {
            let status = response.status();
            anyhow::bail!("TMDB API error: {}", status);
        }

        let details: TmdbMovieDetails = response
            .json()
            .await
            .context("Failed to parse movie details")?;

        Ok(details)
    }

    /// Get external IDs for a movie (including IMDB ID)
    pub async fn get_movie_external_ids(&self, tmdb_id: i32) -> Result<ExternalIds> {
        let url = format!("{}/movie/{}/external_ids", TMDB_BASE_URL, tmdb_id);

        let response = self
            .client
            .get(&url)
            .query(&[("api_key", self.api_key.as_str())])
            .send()
            .await
            .context("Failed to get external IDs")?;

        if !response.status().is_success() {
            let status = response.status();
            anyhow::bail!("TMDB API error: {}", status);
        }

        let ids: ExternalIds = response
            .json()
            .await
            .context("Failed to parse external IDs")?;

        Ok(ids)
    }

    /// Get external IDs for a TV show (including IMDB ID)
    pub async fn get_tv_external_ids(&self, tmdb_id: i32) -> Result<ExternalIds> {
        let url = format!("{}/tv/{}/external_ids", TMDB_BASE_URL, tmdb_id);

        let response = self
            .client
            .get(&url)
            .query(&[("api_key", self.api_key.as_str())])
            .send()
            .await
            .context("Failed to get TV external IDs")?;

        if !response.status().is_success() {
            let status = response.status();
            anyhow::bail!("TMDB API error: {}", status);
        }

        let ids: ExternalIds = response
            .json()
            .await
            .context("Failed to parse TV external IDs")?;

        Ok(ids)
    }

    /// Search for a movie and get its IMDB ID in one call
    pub async fn search_movie_with_imdb(&self, query: &str) -> Result<Vec<MovieSearchResult>> {
        let movies = self.search_movie(query).await?;
        let mut results = Vec::new();

        for movie in movies.into_iter().take(10) {
            // Get IMDB ID for each result
            match self.get_movie_external_ids(movie.id).await {
                Ok(ids) => {
                    let year = movie.release_date.as_ref()
                        .and_then(|d| d.split('-').next())
                        .and_then(|y| y.parse().ok());
                    results.push(MovieSearchResult {
                        tmdb_id: movie.id,
                        imdb_id: ids.imdb_id,
                        title: movie.title,
                        original_title: movie.original_title,
                        overview: movie.overview,
                        poster_path: movie.poster_path,
                        backdrop_path: movie.backdrop_path,
                        release_date: movie.release_date,
                        vote_average: movie.vote_average,
                        year,
                    });
                }
                Err(e) => {
                    debug!("Failed to get IMDB ID for {}: {}", movie.title, e);
                    // Include without IMDB ID
                    results.push(MovieSearchResult {
                        tmdb_id: movie.id,
                        imdb_id: None,
                        title: movie.title,
                        original_title: movie.original_title,
                        overview: movie.overview,
                        poster_path: movie.poster_path,
                        backdrop_path: movie.backdrop_path,
                        release_date: movie.release_date,
                        vote_average: movie.vote_average,
                        year: None,
                    });
                }
            }
        }

        Ok(results)
    }

    /// Get the full poster URL for a given path
    pub fn poster_url(poster_path: &str, size: PosterSize) -> String {
        format!("{}/{}{}", TMDB_IMAGE_BASE, size.as_str(), poster_path)
    }

    /// Get the full backdrop URL for a given path
    pub fn backdrop_url(backdrop_path: &str, size: BackdropSize) -> String {
        format!("{}/{}{}", TMDB_IMAGE_BASE, size.as_str(), backdrop_path)
    }
}

/// Poster size options
#[derive(Debug, Clone, Copy)]
pub enum PosterSize {
    W92,
    W154,
    W185,
    W342,
    W500,
    W780,
    Original,
}

impl PosterSize {
    fn as_str(&self) -> &'static str {
        match self {
            PosterSize::W92 => "w92",
            PosterSize::W154 => "w154",
            PosterSize::W185 => "w185",
            PosterSize::W342 => "w342",
            PosterSize::W500 => "w500",
            PosterSize::W780 => "w780",
            PosterSize::Original => "original",
        }
    }
}

/// Backdrop size options
#[derive(Debug, Clone, Copy)]
pub enum BackdropSize {
    W300,
    W780,
    W1280,
    Original,
}

impl BackdropSize {
    fn as_str(&self) -> &'static str {
        match self {
            BackdropSize::W300 => "w300",
            BackdropSize::W780 => "w780",
            BackdropSize::W1280 => "w1280",
            BackdropSize::Original => "original",
        }
    }
}

// ==================== TMDB API RESPONSE TYPES ====================

#[derive(Debug, Deserialize)]
struct TmdbSearchResponse {
    results: Vec<TmdbMovieResult>,
    #[allow(dead_code)]
    total_results: i32,
    #[allow(dead_code)]
    total_pages: i32,
}

#[derive(Debug, Deserialize)]
struct TmdbTvSearchResponse {
    results: Vec<TmdbTvResult>,
    #[allow(dead_code)]
    total_results: i32,
    #[allow(dead_code)]
    total_pages: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbMovieResult {
    pub id: i32,
    pub title: String,
    pub original_title: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub release_date: Option<String>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>,
    pub popularity: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbTvResult {
    pub id: i32,
    pub name: String,
    pub original_name: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub first_air_date: Option<String>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>,
    pub popularity: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbMovieDetails {
    pub id: i32,
    pub title: String,
    pub original_title: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub release_date: Option<String>,
    pub vote_average: Option<f32>,
    pub runtime: Option<i32>,
    pub genres: Vec<TmdbGenre>,
    pub status: String,
    pub tagline: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbGenre {
    pub id: i32,
    pub name: String,
}

/// External IDs from TMDB (IMDB, Facebook, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalIds {
    pub id: i32,
    pub imdb_id: Option<String>,
    pub facebook_id: Option<String>,
    pub instagram_id: Option<String>,
    pub twitter_id: Option<String>,
}

/// Combined movie search result with IMDB ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieSearchResult {
    pub tmdb_id: i32,
    pub imdb_id: Option<String>,
    pub title: String,
    pub original_title: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub release_date: Option<String>,
    pub vote_average: Option<f32>,
    pub year: Option<u16>,
}

impl MovieSearchResult {
    /// Get the full poster URL
    pub fn poster_url(&self, size: PosterSize) -> Option<String> {
        self.poster_path.as_ref().map(|p| TmdbClient::poster_url(p, size))
    }

    /// Get the full backdrop URL
    pub fn backdrop_url(&self, size: BackdropSize) -> Option<String> {
        self.backdrop_path.as_ref().map(|p| TmdbClient::backdrop_url(p, size))
    }
}

// ==================== METADATA FETCHER ====================

/// High-level metadata fetcher that integrates with the database
pub struct MetadataFetcher {
    tmdb: TmdbClient,
    database: Database,
}

impl MetadataFetcher {
    pub fn new(api_key: String, database: Database) -> Result<Self> {
        let tmdb = TmdbClient::new(api_key)?;
        Ok(Self { tmdb, database })
    }

    /// Fetch and update metadata for a single media item
    pub async fn fetch_for_item(&self, item_id: Uuid, search_query: &str) -> Result<()> {
        info!("Fetching metadata for: {}", search_query);

        // Search TMDB
        let results = self.tmdb.search_movie(search_query).await?;

        if results.is_empty() {
            warn!("No TMDB results found for: {}", search_query);
            return Ok(());
        }

        // Take the first (most relevant) result
        let movie = &results[0];

        // Get full poster and backdrop URLs
        let poster_url = movie
            .poster_path
            .as_ref()
            .map(|p| TmdbClient::poster_url(p, PosterSize::W500));

        let backdrop_url = movie
            .backdrop_path
            .as_ref()
            .map(|p| TmdbClient::backdrop_url(p, BackdropSize::W1280));

        // Update database
        let repo = MediaRepository::new(self.database.pool());
        repo.update_media_metadata(
            item_id,
            &movie.title,
            poster_url.as_deref(),
            backdrop_url.as_deref(),
            movie.overview.as_deref(),
            movie.release_date.as_deref(),
            movie.vote_average,
            None, // Runtime requires additional API call
            Some(movie.id),
        )
        .await?;

        info!("Updated metadata for: {} (TMDB ID: {})", movie.title, movie.id);
        Ok(())
    }

    /// Fetch metadata for all items without TMDB ID
    pub async fn fetch_missing_metadata(&self) -> Result<usize> {
        let repo = MediaRepository::new(self.database.pool());
        let all_items = repo.get_all_media().await?;

        let missing: Vec<_> = all_items
            .into_iter()
            .filter(|item| item.tmdb_id.is_none())
            .collect();

        info!("Found {} items without metadata", missing.len());

        let mut updated = 0;
        for item in missing {
            // Use the extracted title for search
            if let Err(e) = self.fetch_for_item(item.id, &item.title).await {
                warn!("Failed to fetch metadata for {}: {}", item.title, e);
            } else {
                updated += 1;
            }

            // Rate limiting - TMDB allows ~40 requests per 10 seconds
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        Ok(updated)
    }

    /// Get the TMDB client for direct API access
    pub fn client(&self) -> &TmdbClient {
        &self.tmdb
    }
}

/// Extract a clean search query from a filename
pub fn clean_title_for_search(filename: &str) -> String {
    // Remove extension
    let name = filename
        .rsplit_once('.')
        .map(|(n, _)| n)
        .unwrap_or(filename);

    // Replace separators
    let mut title = name
        .replace('.', " ")
        .replace('_', " ")
        .replace('-', " ");

    // Remove common quality/format indicators
    let patterns = [
        "1080p", "720p", "480p", "2160p", "4k", "uhd",
        "bluray", "bdrip", "brrip", "dvdrip", "webrip", "web-dl", "webdl",
        "hdtv", "hdrip", "x264", "x265", "h264", "h265", "hevc", "avc",
        "aac", "ac3", "dts", "5.1", "7.1",
        "proper", "repack", "extended", "unrated", "directors cut",
    ];

    for pattern in patterns {
        title = title.to_lowercase().replace(pattern, "");
    }

    // Remove year patterns like (2023) or [2023]
    let re_year = regex_lite::Regex::new(r"[\(\[\{]?\d{4}[\)\]\}]?").unwrap();
    title = re_year.replace_all(&title, "").to_string();

    // Clean up extra spaces
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

