//! Stream picker filters: what a release name says about quality (resolution,
//! HDR / Dolby Vision, codec, audio, source, size) and the user's filter + sort.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Resolution {
    #[default]
    Unknown,
    Sd,
    Hd720,
    Hd1080,
    Uhd,
}

impl Resolution {
    pub const ALL: [Resolution; 4] = [Resolution::Uhd, Resolution::Hd1080, Resolution::Hd720, Resolution::Sd];
    pub fn label(self) -> &'static str {
        match self {
            Resolution::Uhd => "4K",
            Resolution::Hd1080 => "1080p",
            Resolution::Hd720 => "720p",
            Resolution::Sd => "SD",
            Resolution::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    Hevc,
    Av1,
    H264,
    #[default]
    Other,
}

impl Codec {
    pub const ALL: [Codec; 3] = [Codec::Hevc, Codec::Av1, Codec::H264];
    pub fn label(self) -> &'static str {
        match self {
            Codec::Hevc => "HEVC",
            Codec::Av1 => "AV1",
            Codec::H264 => "H.264",
            Codec::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Remux,
    BluRay,
    Web,
    Hdtv,
    Dvd,
    /// CAM, TS, telesync, screeners
    Cam,
    #[default]
    Other,
}

impl Source {
    pub const ALL: [Source; 4] = [Source::Remux, Source::BluRay, Source::Web, Source::Hdtv];
    pub fn label(self) -> &'static str {
        match self {
            Source::Remux => "Remux",
            Source::BluRay => "BluRay",
            Source::Web => "WEB",
            Source::Hdtv => "HDTV",
            Source::Dvd => "DVD",
            Source::Cam => "CAM/TS",
            Source::Other => "Other",
        }
    }
}

/// "7.88 GB", "700 MB", "1.4 GiB" → bytes (sizes as scrapers print them)
pub fn parse_size(s: &str) -> Option<u64> {
    let mut it = s.split_whitespace();
    let n: f64 = it.next()?.replace(',', ".").parse().ok()?;
    let unit = it.next()?.to_uppercase();
    let mult = match unit.trim_end_matches("IB").trim_end_matches('B') {
        "T" => 1024f64.powi(4),
        "G" => 1024f64.powi(3),
        "M" => 1024f64.powi(2),
        "K" => 1024.0,
        "" => 1.0,
        _ => return None,
    };
    Some((n * mult) as u64)
}

/// What a release name tells us
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamTraits {
    pub resolution: Resolution,
    pub hdr: bool,
    pub dolby_vision: bool,
    pub codec: Codec,
    pub atmos: bool,
    /// 5.1 / 7.1 (or a lossless multichannel track)
    pub surround: bool,
    pub source: Source,
    pub size_bytes: Option<u64>,
}

fn tokens(title: &str) -> Vec<String> {
    title
        .to_uppercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '+'))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

impl StreamTraits {
    pub fn parse(title: &str, size_bytes: Option<u64>) -> Self {
        let t = tokens(title);
        let has = |w: &str| t.iter().any(|x| x == w);
        let up = title.to_uppercase();
        let resolution = if has("2160P") || has("4K") || has("UHD") {
            Resolution::Uhd
        } else if has("1080P") || has("1080I") {
            Resolution::Hd1080
        } else if has("720P") {
            Resolution::Hd720
        } else if has("480P") || has("576P") || has("360P") || has("DVDRIP") || has("XVID") {
            Resolution::Sd
        } else {
            Resolution::Unknown
        };
        let dolby_vision = has("DV") || has("DOVI") || up.contains("DOLBY VISION") || up.contains("DOLBY.VISION") || has("DOLBYVISION");
        let hdr = dolby_vision || t.iter().any(|x| x.starts_with("HDR") || x == "HLG");
        let codec = if has("AV1") {
            Codec::Av1
        } else if has("X265") || has("H265") || has("HEVC") || up.contains("H.265") {
            Codec::Hevc
        } else if has("X264") || has("H264") || has("AVC") || up.contains("H.264") {
            Codec::H264
        } else {
            Codec::Other
        };
        let atmos = has("ATMOS");
        let surround = atmos
            || up.contains("5.1")
            || up.contains("7.1")
            || has("DD5")
            || has("DDP5")
            || has("TRUEHD")
            || t.iter().any(|x| x.starts_with("DTS"));
        let source = if has("REMUX") || has("BDREMUX") {
            Source::Remux
        } else if has("CAM") || has("CAMRIP") || has("HDCAM") || has("TS") || has("HDTS") || has("TELESYNC") || has("TC") || has("TELECINE")
            || has("SCR") || has("DVDSCR") || has("SCREENER") || has("HDTC")
        {
            Source::Cam
        } else if has("BLURAY") || has("BLU") || has("BDRIP") || has("BRRIP") || has("BD") {
            Source::BluRay
        } else if has("WEB") || has("WEBDL") || has("WEBRIP") || up.contains("WEB-DL") || has("AMZN") || has("NF") || has("DSNP") || has("ATVP") || has("HMAX") {
            Source::Web
        } else if has("HDTV") || has("PDTV") {
            Source::Hdtv
        } else if has("DVDRIP") || has("DVD") || has("DVD5") || has("DVD9") {
            Source::Dvd
        } else {
            Source::Other
        };
        Self { resolution, hdr, dolby_vision, codec, atmos, surround, source, size_bytes }
    }

    /// "4K · DV · HEVC · Atmos · Remux"
    pub fn badges(&self) -> Vec<&'static str> {
        let mut v = vec![];
        if self.resolution != Resolution::Unknown {
            v.push(self.resolution.label());
        }
        if self.dolby_vision {
            v.push("DV");
        } else if self.hdr {
            v.push("HDR");
        }
        if self.codec != Codec::Other {
            v.push(self.codec.label());
        }
        if self.atmos {
            v.push("Atmos");
        } else if self.surround {
            v.push("5.1");
        }
        if !matches!(self.source, Source::Other) {
            v.push(self.source.label());
        }
        v
    }
}

/// HDR requirement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HdrFilter {
    #[default]
    Any,
    /// HDR10 / HDR10+ / HLG / Dolby Vision
    Hdr,
    DolbyVision,
    /// SDR only (for displays without HDR)
    NoHdr,
}

impl HdrFilter {
    pub const ALL: [HdrFilter; 4] = [HdrFilter::Any, HdrFilter::Hdr, HdrFilter::DolbyVision, HdrFilter::NoHdr];
    pub fn label(self) -> &'static str {
        match self {
            HdrFilter::Any => "Any",
            HdrFilter::Hdr => "HDR",
            HdrFilter::DolbyVision => "Dolby Vision",
            HdrFilter::NoHdr => "SDR only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFilter {
    #[default]
    Any,
    Surround,
    Atmos,
}

impl AudioFilter {
    pub const ALL: [AudioFilter; 3] = [AudioFilter::Any, AudioFilter::Surround, AudioFilter::Atmos];
    pub fn label(self) -> &'static str {
        match self {
            AudioFilter::Any => "Any",
            AudioFilter::Surround => "5.1+",
            AudioFilter::Atmos => "Atmos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamSort {
    /// Real-Debrid cached first, then quality score, then seeders
    #[default]
    Best,
    Resolution,
    SizeDesc,
    SizeAsc,
    Seeders,
    /// Fast-start order: cached first, then torrent health, then quality
    Hottest,
}

impl StreamSort {
    pub const ALL: [StreamSort; 6] =
        [StreamSort::Best, StreamSort::Hottest, StreamSort::Resolution, StreamSort::SizeDesc, StreamSort::SizeAsc, StreamSort::Seeders];
    pub fn label(self) -> &'static str {
        match self {
            StreamSort::Best => "Best match",
            StreamSort::Resolution => "Resolution",
            StreamSort::SizeDesc => "Largest",
            StreamSort::SizeAsc => "Smallest",
            StreamSort::Seeders => "Seeders",
            StreamSort::Hottest => "Starts fastest",
        }
    }
}

/// Size presets for the min / max chips (GB)
pub const MAX_SIZE_GB: [Option<u32>; 5] = [None, Some(2), Some(5), Some(10), Some(25)];
pub const MIN_SIZE_GB: [Option<u32>; 4] = [None, Some(1), Some(4), Some(10)];

/// Stream picker filter + sort (the default is saved in Settings)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamFilter {
    /// Allowed resolutions; empty = any
    pub resolutions: Vec<Resolution>,
    pub hdr: HdrFilter,
    /// Allowed codecs; empty = any
    pub codecs: Vec<Codec>,
    pub audio: AudioFilter,
    /// Allowed sources; empty = any
    pub sources: Vec<Source>,
    pub hide_cam: bool,
    pub cached_only: bool,
    pub min_gb: Option<u32>,
    pub max_gb: Option<u32>,
    pub sort: StreamSort,
}

impl Default for StreamFilter {
    fn default() -> Self {
        Self {
            resolutions: vec![],
            hdr: HdrFilter::Any,
            codecs: vec![],
            audio: AudioFilter::Any,
            sources: vec![],
            hide_cam: true,
            cached_only: false,
            min_gb: None,
            max_gb: None,
            sort: StreamSort::Best,
        }
    }
}

pub fn toggle<T: PartialEq + Copy>(v: &mut Vec<T>, x: T) {
    if let Some(i) = v.iter().position(|y| *y == x) {
        v.remove(i);
    } else {
        v.push(x);
    }
}

impl StreamFilter {
    /// Anything narrower than "show everything except CAM"
    pub fn is_active(&self) -> bool {
        let base = StreamFilter { sort: self.sort, ..Default::default() };
        *self != base
    }

    pub fn matches(&self, t: &StreamTraits, rd_cached: bool) -> bool {
        if self.hide_cam && t.source == Source::Cam {
            return false;
        }
        if self.cached_only && !rd_cached {
            return false;
        }
        if !self.resolutions.is_empty() && !self.resolutions.contains(&t.resolution) {
            return false;
        }
        let hdr_ok = match self.hdr {
            HdrFilter::Any => true,
            HdrFilter::Hdr => t.hdr,
            HdrFilter::DolbyVision => t.dolby_vision,
            HdrFilter::NoHdr => !t.hdr,
        };
        if !hdr_ok {
            return false;
        }
        if !self.codecs.is_empty() && !self.codecs.contains(&t.codec) {
            return false;
        }
        let audio_ok = match self.audio {
            AudioFilter::Any => true,
            AudioFilter::Surround => t.surround,
            AudioFilter::Atmos => t.atmos,
        };
        if !audio_ok {
            return false;
        }
        if !self.sources.is_empty() && !self.sources.contains(&t.source) {
            return false;
        }
        // GiB, like the sizes scrapers print
        let gb = |n: u32| n as u64 * 1_073_741_824;
        // unknown sizes pass size limits (many cached results don't report one)
        if let Some(size) = t.size_bytes {
            if self.min_gb.is_some_and(|m| size < gb(m)) || self.max_gb.is_some_and(|m| size > gb(m)) {
                return false;
            }
        }
        true
    }
}

/// Indices of `hits` that pass `f`, in the chosen order
pub fn apply(hits: &[crate::StreamHit], f: &StreamFilter) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..hits.len()).filter(|i| f.matches(&hits[*i].traits, hits[*i].rd_cached)).collect();
    let size = |i: &usize| hits[*i].traits.size_bytes.unwrap_or(0);
    match f.sort {
        // search results already come in "best" order
        StreamSort::Best => {}
        StreamSort::Resolution => idx.sort_by(|a, b| {
            hits[*b].traits.resolution.cmp(&hits[*a].traits.resolution).then(hits[*b].rd_cached.cmp(&hits[*a].rd_cached)).then(hits[*b].score.cmp(&hits[*a].score))
        }),
        StreamSort::SizeDesc => idx.sort_by_key(|i| std::cmp::Reverse(size(i))),
        StreamSort::SizeAsc => idx.sort_by_key(|i| if hits[*i].traits.size_bytes.is_some() { size(i) } else { u64::MAX }),
        StreamSort::Seeders => idx.sort_by_key(|i| std::cmp::Reverse(hits[*i].seeders.unwrap_or(0))),
        StreamSort::Hottest => sort_hottest(hits, &mut idx),
    }
    idx
}

/// Which candidates to ask Real-Debrid about (each check is one request
/// when RD refuses the torrent, four when it accepts it): the first few of
/// the fast-start order plus the best-quality ones, since a cached link
/// starts instantly whatever its swarm looks like
pub fn rd_probe_set(hits: &[crate::StreamHit], order: &[usize], n: usize) -> Vec<usize> {
    let mut by_quality = order.to_vec();
    by_quality.sort_by(|a, b| hits[*b].score.cmp(&hits[*a].score));
    let mut out: Vec<usize> = Vec::with_capacity(n * 2);
    for i in order.iter().take(n).chain(by_quality.iter().take(n)) {
        if !out.contains(i) {
            out.push(*i);
        }
    }
    out
}

/// "Best match" order: Real-Debrid cached first, then quality, then seeders
pub fn sort_best(hits: &mut [crate::StreamHit]) {
    hits.sort_by(|a, b| {
        b.rd_cached
            .cmp(&a.rd_cached)
            .then(b.score.cmp(&a.score))
            .then(b.seeders.unwrap_or(0).cmp(&a.seeders.unwrap_or(0)))
    });
}

/// How alive a torrent looks, from its seeder count
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    Dead,
    Weak,
    Good,
    Hot,
}

impl Health {
    pub fn of(seeders: Option<u32>) -> Self {
        match seeders.unwrap_or(0) {
            0..=4 => Health::Dead,
            5..=24 => Health::Weak,
            25..=99 => Health::Good,
            _ => Health::Hot,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Health::Hot => "🔥",
            Health::Good => "▲",
            Health::Weak => "▽",
            Health::Dead => "✕",
        }
    }

    fn lower(self) -> Self {
        match self {
            Health::Hot => Health::Good,
            Health::Good => Health::Weak,
            _ => Health::Dead,
        }
    }
}

/// Files above this need more bandwidth than most swarms deliver
const P2P_HEAVY_BYTES: u64 = 30 * 1024 * 1024 * 1024;

/// Health used for the fast-start pick: a cached Real-Debrid link is always
/// "hot", a huge P2P file counts one step colder (remux bitrates stall)
pub fn start_health(h: &crate::StreamHit) -> Health {
    if h.rd_cached {
        return Health::Hot;
    }
    let base = Health::of(h.seeders);
    if h.traits.size_bytes.is_some_and(|b| b > P2P_HEAVY_BYTES) {
        base.lower()
    } else {
        base
    }
}

/// How quickly a stream should start: a bigger swarm counts most (log of the
/// seeders), quality breaks near-ties, huge P2P files are penalised
pub fn start_score(h: &crate::StreamHit) -> f64 {
    let swarm = (h.seeders.unwrap_or(0) as f64 + 1.0).log2();
    let quality = h.score as f64 / 1000.0;
    let heavy = if !h.rd_cached && h.traits.size_bytes.is_some_and(|b| b > P2P_HEAVY_BYTES) { 1.5 } else { 0.0 };
    swarm + 0.6 * quality - heavy
}

fn sort_hottest(hits: &[crate::StreamHit], idx: &mut [usize]) {
    idx.sort_by(|a, b| {
        let (a, b) = (&hits[*a], &hits[*b]);
        b.rd_cached
            .cmp(&a.rd_cached)
            .then(if a.rd_cached { b.score.cmp(&a.score) } else { std::cmp::Ordering::Equal })
            .then(start_score(b).total_cmp(&start_score(a)))
    });
}

/// Candidates for "Play best", in the order they should be tried: streams
/// that pass the filter, Real-Debrid cached first (best quality first), then
/// by [`start_score`]. Dead torrents (under 5 seeders) are left out unless nothing
/// else matches.
pub fn fast_start_order(hits: &[crate::StreamHit], f: &StreamFilter) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..hits.len()).filter(|i| f.matches(&hits[*i].traits, hits[*i].rd_cached)).collect();
    sort_hottest(hits, &mut idx);
    if idx.iter().any(|i| start_health(&hits[*i]) > Health::Dead) {
        idx.retain(|i| start_health(&hits[*i]) > Health::Dead);
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(title: &str, gb: u64, seeders: u32, rd: bool) -> crate::StreamHit {
        crate::StreamHit {
            title: title.into(),
            quality: String::new(),
            size: None,
            seeders: Some(seeders),
            source: "t".into(),
            rd_cached: rd,
            magnet: String::new(),
            info_hash: title.into(),
            file_idx: None,
            traits: StreamTraits::parse(title, Some(gb * 1024 * 1024 * 1024)),
            score: rmh_scraper::parse_torrent_title(title).quality_score(),
        }
    }

    #[test]
    fn fast_start_prefers_cached_then_hot() {
        let hits = vec![
            hit("Movie.2021.2160p.BluRay.REMUX.HEVC-FGT", 80, 300, false),
            hit("Movie.2021.2160p.WEB-DL.HEVC-EVO", 20, 40, false),
            hit("Movie.2021.1080p.WEB-DL.x264-EVO", 6, 400, false),
            hit("Movie.2021.1080p.BluRay.x264-OLD", 8, 2, false),
            hit("Movie.2021.720p.WEB-DL.x264-RD", 3, 1, true),
        ];
        let order = fast_start_order(&hits, &StreamFilter::default());
        assert_eq!(order[0], 4, "cached Real-Debrid link first");
        assert_eq!(order[1], 2, "then the hot 1080p");
        assert!(!order.contains(&3), "dead torrent dropped");
        // 80 GB remux with 300 seeders counts as merely "good" for P2P
        assert_eq!(start_health(&hits[0]), Health::Good);
        let only_4k = StreamFilter { resolutions: vec![Resolution::Uhd], ..Default::default() };
        let order = fast_start_order(&hits, &only_4k);
        assert_eq!(order.len(), 2);
        assert_eq!(Health::of(Some(150)).icon(), "🔥");
        let all = fast_start_order(&hits, &StreamFilter::default());
        let probe = rd_probe_set(&hits, &all, 1);
        assert_eq!(probe, vec![4, 0], "fastest plus the best-quality remux");
    }

    #[test]
    fn parses_release_names() {
        let t = StreamTraits::parse("Dune.Part.Two.2024.2160p.UHD.BluRay.REMUX.DV.HDR10.HEVC.TrueHD.7.1.Atmos-FGT", Some(70_000_000_000));
        assert_eq!(t.resolution, Resolution::Uhd);
        assert!(t.dolby_vision && t.hdr && t.atmos && t.surround);
        assert_eq!(t.codec, Codec::Hevc);
        assert_eq!(t.source, Source::Remux);
        assert_eq!(t.badges(), vec!["4K", "DV", "HEVC", "Atmos", "Remux"]);

        let t = StreamTraits::parse("Show.S01E02.1080p.AMZN.WEB-DL.DDP5.1.H.264-NTb", None);
        assert_eq!((t.resolution, t.codec, t.source), (Resolution::Hd1080, Codec::H264, Source::Web));
        assert!(t.surround && !t.hdr && !t.atmos);

        let t = StreamTraits::parse("Movie 2025 HDCAM x264", None);
        assert_eq!(t.source, Source::Cam);
        let t = StreamTraits::parse("Movie.2025.720p.HDTS.x264", None);
        assert_eq!(t.source, Source::Cam);
        let t = StreamTraits::parse("Movie.2023.1080p.WEBRip.AV1.Opus", None);
        assert_eq!((t.codec, t.source), (Codec::Av1, Source::Web));
        // "TS" inside a word is not a telesync
        let t = StreamTraits::parse("The.Tsunami.Effects.1080p.BluRay.x265", None);
        assert_eq!(t.source, Source::BluRay);
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("2.5 GB"), Some(2_684_354_560));
        assert_eq!(parse_size("700 MB"), Some(734_003_200));
        assert_eq!(parse_size("1.5 GiB"), Some(1_610_612_736));
        assert_eq!(parse_size("junk"), None);
    }

    #[test]
    fn filters() {
        let uhd = StreamTraits::parse("A.2160p.WEB-DL.DV.HDR.H265.Atmos", Some(20_000_000_000));
        let fhd = StreamTraits::parse("A.1080p.BluRay.x264.DTS", Some(9_000_000_000));
        let cam = StreamTraits::parse("A.CAM.x264", Some(1_000_000_000));
        let f = StreamFilter::default();
        assert!(f.matches(&uhd, false) && f.matches(&fhd, false) && !f.matches(&cam, false));
        assert!(!f.is_active());

        let f = StreamFilter { resolutions: vec![Resolution::Hd1080], ..Default::default() };
        assert!(!f.matches(&uhd, true) && f.matches(&fhd, true));
        assert!(f.is_active());
        let f = StreamFilter { hdr: HdrFilter::DolbyVision, ..Default::default() };
        assert!(f.matches(&uhd, false) && !f.matches(&fhd, false));
        let f = StreamFilter { hdr: HdrFilter::NoHdr, ..Default::default() };
        assert!(!f.matches(&uhd, false) && f.matches(&fhd, false));
        let f = StreamFilter { codecs: vec![Codec::Hevc, Codec::Av1], ..Default::default() };
        assert!(f.matches(&uhd, false) && !f.matches(&fhd, false));
        let f = StreamFilter { audio: AudioFilter::Atmos, ..Default::default() };
        assert!(f.matches(&uhd, false) && !f.matches(&fhd, false));
        let f = StreamFilter { audio: AudioFilter::Surround, ..Default::default() };
        assert!(f.matches(&uhd, false) && f.matches(&fhd, false));
        let f = StreamFilter { max_gb: Some(10), ..Default::default() };
        assert!(!f.matches(&uhd, false) && f.matches(&fhd, false));
        let f = StreamFilter { min_gb: Some(10), ..Default::default() };
        assert!(f.matches(&uhd, false) && !f.matches(&fhd, false));
        let f = StreamFilter { sources: vec![Source::BluRay], cached_only: true, ..Default::default() };
        assert!(!f.matches(&fhd, false) && f.matches(&fhd, true) && !f.matches(&uhd, true));
        let f = StreamFilter { hide_cam: false, ..Default::default() };
        assert!(f.matches(&cam, false));
    }

    #[test]
    fn filter_serde_defaults() {
        let f: StreamFilter = serde_json::from_str("{\"resolutions\":[\"uhd\"]}").unwrap();
        assert_eq!(f.resolutions, vec![Resolution::Uhd]);
        assert!(f.hide_cam);
        let mut v = vec![Codec::Hevc];
        toggle(&mut v, Codec::Av1);
        toggle(&mut v, Codec::Hevc);
        assert_eq!(v, vec![Codec::Av1]);
    }
}
