//! Core domain models for RustMediaHub

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a media item in the library
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: Uuid,
    pub title: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: i64,
    pub media_type: MediaType,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub rating: Option<f32>,
    pub runtime_minutes: Option<i32>,
    pub watched: bool,
    pub watch_progress: i64, // Progress in seconds
    pub tmdb_id: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MediaItem {
    /// Create a new media item from a scanned file
    pub fn from_scan(file_path: String, file_name: String, file_size: i64) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            title: Self::extract_title(&file_name),
            file_path,
            file_name,
            file_size,
            media_type: MediaType::Movie, // Default, can be refined later
            poster_url: None,
            backdrop_url: None,
            overview: None,
            release_date: None,
            rating: None,
            runtime_minutes: None,
            watched: false,
            watch_progress: 0,
            tmdb_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Extract a human-readable title from filename
    fn extract_title(file_name: &str) -> String {
        // Remove extension
        let name = file_name
            .rsplit_once('.')
            .map(|(name, _)| name)
            .unwrap_or(file_name);

        // Replace common separators with spaces
        let title = name
            .replace('.', " ")
            .replace('_', " ")
            .replace('-', " ");

        // Remove common quality indicators and year patterns for cleaner title
        // This is a simple heuristic; metadata fetch will get the real title
        title.trim().to_string()
    }
}

/// Type of media content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    Movie,
    TvShow,
    Episode,
    Unknown,
}

impl std::fmt::Display for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaType::Movie => write!(f, "movie"),
            MediaType::TvShow => write!(f, "tvshow"),
            MediaType::Episode => write!(f, "episode"),
            MediaType::Unknown => write!(f, "unknown"),
        }
    }
}

impl From<String> for MediaType {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "movie" => MediaType::Movie,
            "tvshow" => MediaType::TvShow,
            "episode" => MediaType::Episode,
            _ => MediaType::Unknown,
        }
    }
}

/// Represents a library/folder being scanned
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub media_type: MediaType,
    pub last_scanned: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Library {
    pub fn new(name: String, path: String, media_type: MediaType) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            path,
            media_type,
            last_scanned: None,
            created_at: Utc::now(),
        }
    }
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub tmdb_api_key: Option<String>,
    pub mpv_path: String,
    pub mpv_hwdec: String,
    pub theme: Theme,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tmdb_api_key: None,
            mpv_path: "mpv".to_string(), // Assumes mpv is in PATH
            mpv_hwdec: "auto".to_string(),
            theme: Theme::Dark,
        }
    }
}

/// UI Theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// Scan progress event for UI updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub total_files: usize,
    pub scanned_files: usize,
    pub current_file: String,
    pub is_complete: bool,
}

/// Video file extensions we support
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "m2ts",
];

/// Audio file extensions we support
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "wma", "alac", "aiff",
];

/// Check if a file extension is a supported video format
pub fn is_video_file(extension: &str) -> bool {
    VIDEO_EXTENSIONS.contains(&extension.to_lowercase().as_str())
}

/// Check if a file extension is a supported audio format
pub fn is_audio_file(extension: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&extension.to_lowercase().as_str())
}

/// Check if a file extension is any playable media (video or audio)
pub fn is_media_file(extension: &str) -> bool {
    is_video_file(extension) || is_audio_file(extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_extensions() {
        assert!(is_video_file("MKV"));
        assert!(is_audio_file("flac"));
        assert!(is_media_file("mp3"));
        assert!(!is_media_file("pdf"));
    }

    #[test]
    fn title_from_filename() {
        let item = MediaItem::from_scan("/x/The_Matrix.1999.mkv".into(), "The_Matrix.1999.mkv".into(), 1);
        assert_eq!(item.title, "The Matrix 1999");
    }
}
