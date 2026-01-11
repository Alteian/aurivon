//! Torrent title parser
//! 
//! Extracts quality, resolution, codec, and other info from torrent names

use regex::Regex;
use std::sync::LazyLock;

/// Parsed information from a torrent title
#[derive(Debug, Clone, Default)]
pub struct TorrentInfo {
    /// Original title string
    pub raw_title: String,
    /// Cleaned title (movie/show name)
    pub title: String,
    /// Year if found
    pub year: Option<u16>,
    /// Season number for series
    pub season: Option<u32>,
    /// Episode number for series
    pub episode: Option<u32>,
    /// Video resolution (480p, 720p, 1080p, 2160p/4K)
    pub resolution: Option<String>,
    /// Video quality (BluRay, WEB-DL, HDRip, etc.)
    pub quality: Option<String>,
    /// Video codec (x264, x265/HEVC, AV1, etc.)
    pub codec: Option<String>,
    /// Audio codec (AAC, AC3, DTS, Atmos, etc.)
    pub audio: Option<String>,
    /// HDR format if present
    pub hdr: Option<String>,
    /// Release group
    pub group: Option<String>,
    /// File size as string
    pub size: Option<String>,
    /// Number of seeders (if known)
    pub seeders: Option<u32>,
}

impl TorrentInfo {
    /// Generate a quality string like "1080p BluRay x265 HDR"
    pub fn quality_string(&self) -> String {
        let mut parts = Vec::new();
        
        if let Some(ref res) = self.resolution {
            parts.push(res.clone());
        }
        if let Some(ref quality) = self.quality {
            parts.push(quality.clone());
        }
        if let Some(ref codec) = self.codec {
            parts.push(codec.clone());
        }
        if let Some(ref hdr) = self.hdr {
            parts.push(hdr.clone());
        }
        if let Some(ref audio) = self.audio {
            parts.push(audio.clone());
        }
        
        if parts.is_empty() {
            "Unknown Quality".to_string()
        } else {
            parts.join(" ")
        }
    }

    /// Quality score for sorting (higher = better)
    pub fn quality_score(&self) -> u32 {
        let mut score = 0u32;

        // Resolution scoring
        if let Some(ref res) = self.resolution {
            score += match res.as_str() {
                "2160p" | "4K" => 4000,
                "1080p" => 3000,
                "720p" => 2000,
                "480p" => 1000,
                _ => 500,
            };
        }

        // Quality source scoring
        if let Some(ref quality) = self.quality {
            let q = quality.to_uppercase();
            score += if q.contains("REMUX") {
                500
            } else if q.contains("BLURAY") || q.contains("BLU-RAY") {
                400
            } else if q.contains("WEB-DL") || q.contains("WEBDL") {
                350
            } else if q.contains("WEBRIP") || q.contains("WEB-RIP") {
                300
            } else if q.contains("HDTV") {
                200
            } else if q.contains("HDCAM") || q.contains("HDRIP") {
                100
            } else if q.contains("CAM") || q.contains("TS") || q.contains("TELESYNC") {
                10
            } else {
                50
            };
        }

        // HDR bonus
        if self.hdr.is_some() {
            score += 100;
        }

        // Codec bonus (HEVC/x265 is more efficient)
        if let Some(ref codec) = self.codec {
            let c = codec.to_uppercase();
            if c.contains("HEVC") || c.contains("X265") || c.contains("H265") {
                score += 50;
            } else if c.contains("AV1") {
                score += 60;
            }
        }

        // Seeders bonus
        if let Some(seeders) = self.seeders {
            score += seeders.min(100);
        }

        score
    }
}

/// Parse a torrent title to extract metadata
pub fn parse_torrent_title(title: &str) -> TorrentInfo {
    let mut info = TorrentInfo {
        raw_title: title.to_string(),
        ..Default::default()
    };

    // Resolution
    static RESOLUTION_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(2160p|4k|1080p|720p|480p|1080i|720i)\b").unwrap()
    });
    if let Some(cap) = RESOLUTION_RE.captures(title) {
        let res = cap[1].to_uppercase();
        info.resolution = Some(if res == "4K" { "2160p".to_string() } else { res });
    }

    // Quality/Source
    static QUALITY_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(REMUX|Blu-?Ray|BDRip|BRRip|WEB-?DL|WEBRip|HDRip|HDTV|DVDRip|DVDSCR|CAM|TS|TELESYNC|HDCAM|WEB)\b").unwrap()
    });
    if let Some(cap) = QUALITY_RE.captures(title) {
        info.quality = Some(cap[1].to_uppercase().replace("-", ""));
    }

    // Video Codec
    static CODEC_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(x264|x265|h\.?264|h\.?265|HEVC|AVC|AV1|VP9|MPEG-?2)\b").unwrap()
    });
    if let Some(cap) = CODEC_RE.captures(title) {
        let codec = cap[1].to_uppercase().replace(".", "");
        info.codec = Some(match codec.as_str() {
            "H264" | "AVC" => "x264".to_string(),
            "H265" | "HEVC" => "x265".to_string(),
            _ => codec,
        });
    }

    // Audio
    static AUDIO_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(Atmos|TrueHD|DTS-?HD(?:\s*MA)?|DTS|DD\+?5\.1|AC3|AAC(?:2\.0)?|FLAC|EAC3|Dolby\s*Digital)\b").unwrap()
    });
    if let Some(cap) = AUDIO_RE.captures(title) {
        info.audio = Some(cap[1].to_uppercase());
    }

    // HDR
    static HDR_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(HDR10\+?|Dolby\s*Vision|DV|HDR|HLG|SDR)\b").unwrap()
    });
    if let Some(cap) = HDR_RE.captures(title) {
        let hdr = cap[1].to_uppercase().replace(" ", "");
        if hdr != "SDR" {
            info.hdr = Some(hdr);
        }
    }

    // Season/Episode (S01E02 format)
    static EPISODE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)[Ss](\d{1,2})[Ee](\d{1,3})").unwrap()
    });
    if let Some(cap) = EPISODE_RE.captures(title) {
        info.season = cap[1].parse().ok();
        info.episode = cap[2].parse().ok();
    } else {
        // Try season only (S01, Season 1)
        static SEASON_RE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?i)(?:Season|[Ss])[\s.]?(\d{1,2})").unwrap()
        });
        if let Some(cap) = SEASON_RE.captures(title) {
            info.season = cap[1].parse().ok();
        }
    }

    // Year
    static YEAR_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\b((?:19|20)\d{2})\b").unwrap()
    });
    if let Some(cap) = YEAR_RE.captures(title) {
        info.year = cap[1].parse().ok();
    }

    // Release group (usually at the end, after dash)
    static GROUP_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"-([A-Za-z0-9]+)(?:\[.*?\])?$").unwrap()
    });
    if let Some(cap) = GROUP_RE.captures(title) {
        info.group = Some(cap[1].to_string());
    }

    // Clean title: take everything before year/resolution/quality markers
    let clean_title = title
        .split(|c: char| c == '.' || c == '_')
        .take_while(|part| {
            let p = part.to_uppercase();
            !p.parse::<u16>().map(|y| y >= 1900 && y <= 2100).unwrap_or(false)
                && !RESOLUTION_RE.is_match(part)
                && !QUALITY_RE.is_match(part)
                && !p.starts_with("S0")
                && !p.starts_with("S1")
        })
        .collect::<Vec<_>>()
        .join(" ");
    
    info.title = clean_title.trim().to_string();

    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_movie() {
        let info = parse_torrent_title("The.Matrix.1999.2160p.UHD.BluRay.x265.HDR.DTS-HD.MA-GROUP");
        assert_eq!(info.title, "The Matrix");
        assert_eq!(info.year, Some(1999));
        assert_eq!(info.resolution, Some("2160p".to_string()));
        assert_eq!(info.quality, Some("BLURAY".to_string()));
        assert_eq!(info.codec, Some("x265".to_string()));
        assert!(info.hdr.is_some());
    }

    #[test]
    fn test_parse_series() {
        let info = parse_torrent_title("Breaking.Bad.S05E16.Felina.1080p.BluRay.x264-DEMAND");
        assert!(info.title.contains("Breaking"));
        assert_eq!(info.season, Some(5));
        assert_eq!(info.episode, Some(16));
        assert_eq!(info.resolution, Some("1080p".to_string()));
    }

    #[test]
    fn test_quality_score() {
        let low = parse_torrent_title("Movie.2024.480p.WEBRip.x264");
        let high = parse_torrent_title("Movie.2024.2160p.UHD.BluRay.REMUX.HDR");
        
        assert!(high.quality_score() > low.quality_score());
    }
}
