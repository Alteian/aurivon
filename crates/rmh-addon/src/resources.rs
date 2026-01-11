//! Resource types returned by addons

use serde::{Deserialize, Serialize};

/// A content item in a catalog (movie, series, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaItem {
    /// Unique identifier (format: "prefix:id", e.g., "tt1234567" for IMDB)
    pub id: String,
    
    /// Content type
    #[serde(rename = "type")]
    pub content_type: String,
    
    /// Title/name
    pub name: String,
    
    /// Poster image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    
    /// Background/fanart image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    
    /// Logo image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    
    /// Description/overview
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    
    /// Release year or date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_info: Option<String>,
    
    /// IMDB ID (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,
    
    /// Runtime in format "1h 30m" or minutes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    
    /// Genres
    #[serde(default)]
    pub genres: Vec<String>,
    
    /// Director(s)
    #[serde(default)]
    pub director: Vec<String>,
    
    /// Cast members
    #[serde(default)]
    pub cast: Vec<String>,
    
    /// IMDB rating (0-10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imdb_rating: Option<f32>,
    
    /// Trailer URL (YouTube or direct)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailer: Option<String>,
    
    /// For series: list of videos/episodes
    #[serde(default)]
    pub videos: Vec<Video>,
    
    /// External links
    #[serde(default)]
    pub links: Vec<Link>,
    
    /// Behavior hints for UI
    #[serde(default)]
    pub behavior_hints: MetaBehaviorHints,
}

impl MetaItem {
    pub fn new(id: impl Into<String>, content_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content_type: content_type.into(),
            name: name.into(),
            poster: None,
            background: None,
            logo: None,
            description: None,
            release_info: None,
            imdb_id: None,
            runtime: None,
            genres: Vec::new(),
            director: Vec::new(),
            cast: Vec::new(),
            imdb_rating: None,
            trailer: None,
            videos: Vec::new(),
            links: Vec::new(),
            behavior_hints: MetaBehaviorHints::default(),
        }
    }

    pub fn with_poster(mut self, url: impl Into<String>) -> Self {
        self.poster = Some(url.into());
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_year(mut self, year: u16) -> Self {
        self.release_info = Some(year.to_string());
        self
    }
}

/// Video entry (for series episodes or movie versions)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    /// Video ID (e.g., "tt1234567:1:1" for S01E01)
    pub id: String,
    
    /// Episode/video title
    pub title: String,
    
    /// Season number (for series)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub season: Option<u32>,
    
    /// Episode number (for series)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episode: Option<u32>,
    
    /// Release date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub released: Option<String>,
    
    /// Thumbnail URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    
    /// Episode overview
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
}

/// External link (trailers, IMDB, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub name: String,
    pub category: String,
    pub url: String,
}

/// Behavior hints for meta items
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaBehaviorHints {
    /// Default video ID to play
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_video_id: Option<String>,
    
    /// Has scheduled release (upcoming)
    #[serde(default)]
    pub has_scheduled_videos: bool,
}

/// A playable stream source
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    /// Stream URL (HTTP, magnet, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    
    /// YouTube video ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yt_id: Option<String>,
    
    /// Magnet/torrent info hash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info_hash: Option<String>,
    
    /// File index within torrent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_idx: Option<u32>,
    
    /// External player URL (opens in browser)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    
    /// Stream title/name (e.g., "1080p BluRay")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    
    /// Stream name/source (e.g., addon name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    
    /// Stream description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    
    /// Subtitles bundled with this stream
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
    
    /// Behavior hints
    #[serde(default)]
    pub behavior_hints: StreamBehaviorHints,
}

impl Stream {
    /// Create a direct HTTP stream
    pub fn http(url: impl Into<String>) -> Self {
        Self {
            url: Some(url.into()),
            yt_id: None,
            info_hash: None,
            file_idx: None,
            external_url: None,
            title: None,
            name: None,
            description: None,
            subtitles: Vec::new(),
            behavior_hints: StreamBehaviorHints::default(),
        }
    }

    /// Create a torrent/magnet stream
    pub fn torrent(info_hash: impl Into<String>, file_idx: Option<u32>) -> Self {
        Self {
            url: None,
            yt_id: None,
            info_hash: Some(info_hash.into()),
            file_idx,
            external_url: None,
            title: None,
            name: None,
            description: None,
            subtitles: Vec::new(),
            behavior_hints: StreamBehaviorHints::default(),
        }
    }

    /// Create a local file stream
    pub fn local_file(path: impl Into<String>) -> Self {
        Self::http(format!("file://{}", path.into()))
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_quality(mut self, quality: impl Into<String>) -> Self {
        self.title = Some(quality.into());
        self
    }
}

/// Behavior hints for streams
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamBehaviorHints {
    /// Estimated size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_size: Option<u64>,
    
    /// Video resolution (e.g., "1920x1080")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_resolution: Option<String>,
    
    /// Filename for display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    
    /// Whether to not auto-play
    #[serde(default)]
    pub not_web_ready: bool,
    
    /// Binary content type
    #[serde(default)]
    pub binge_group: Option<String>,
}

/// Subtitle track
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtitle {
    /// Subtitle ID
    pub id: String,
    
    /// Subtitle URL (SRT, VTT, etc.)
    pub url: String,
    
    /// Language code (e.g., "en", "es")
    pub lang: String,
    
    /// Optional label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Subtitle {
    pub fn new(id: impl Into<String>, url: impl Into<String>, lang: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            url: url.into(),
            lang: lang.into(),
            label: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// Response from a catalog request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogResponse {
    /// List of meta items
    pub metas: Vec<MetaItem>,
    
    /// Pagination: has more pages
    #[serde(default)]
    pub has_more: bool,
    
    /// Cache settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_max_age: Option<u32>,
}

impl CatalogResponse {
    pub fn new(metas: Vec<MetaItem>) -> Self {
        Self {
            metas,
            has_more: false,
            cache_max_age: None,
        }
    }

    pub fn with_pagination(mut self, has_more: bool) -> Self {
        self.has_more = has_more;
        self
    }
}

/// Response from a meta request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaResponse {
    pub meta: MetaItem,
}

/// Response from a stream request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamResponse {
    pub streams: Vec<Stream>,
}

/// Response from a subtitle request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleResponse {
    pub subtitles: Vec<Subtitle>,
}
