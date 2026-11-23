//! UI-agnostic video/audio player built on the `mpv` executable and its JSON IPC.
//!
//! The player runs `mpv` as a separate process and talks to it over
//! `--input-ipc-server` (Unix socket on Linux/macOS, named pipe on Windows).
//! This keeps the GUI process isolated from any FFI/EGL issues.
//!
//! Architecture:
//! - [`Player`] is a cheap, cloneable handle. Every method is synchronous and
//!   non-blocking, so it can be driven from any UI toolkit (iced today, GPUI later).
//! - A background actor runs on its own thread with its own Tokio runtime. It owns
//!   the `mpv` process and the IPC socket. The handle never depends on the caller's
//!   async runtime.
//! - Playback state is mirrored into shared memory and exposed via
//!   [`Player::snapshot`] for polling.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Configuration for the mpv player
#[derive(Debug, Clone)]
pub struct PlayerConfig {
    /// Path or name of the mpv executable
    pub mpv_path: String,
    /// Hardware decoding mode (auto, vaapi, nvdec, no, ...)
    pub hwdec: String,
    /// Start in fullscreen mode
    pub fullscreen: bool,
    /// Show mpv's own on-screen controller
    pub osc: bool,
    /// Initial volume (0-100)
    pub volume: u8,
    /// Let mpv remember positions itself (`--save-position-on-quit`)
    pub save_position: bool,
    /// Native window id for `mpv --wid` (X11 window / Win32 HWND). `None` = separate window.
    pub wid: Option<i64>,
    /// Run without any video/audio output (tests, CI)
    pub headless: bool,
    /// Additional custom arguments
    pub extra_args: Vec<String>,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            mpv_path: "mpv".to_string(),
            hwdec: "auto".to_string(),
            fullscreen: false,
            osc: true,
            volume: 100,
            save_position: false,
            wid: None,
            headless: false,
            extra_args: Vec::new(),
        }
    }
}

/// Point-in-time view of the playback state, safe to read from any UI thread.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaybackSnapshot {
    /// mpv process is running and connected
    pub active: bool,
    /// A file is loaded (not idle)
    pub loaded: bool,
    pub position: f64,
    pub duration: Option<f64>,
    pub paused: bool,
    pub volume: f64,
    pub muted: bool,
    pub eof: bool,
    pub title: Option<String>,
    /// Chapters of the current file (`chapter-list`)
    pub chapters: Vec<Chapter>,
    /// Index of the current chapter
    pub chapter: Option<i64>,
    /// Audio / subtitle / video tracks (`track-list`)
    pub tracks: Vec<Track>,
    pub fullscreen: bool,
    /// Playback speed (1.0 = normal)
    pub speed: f64,
    /// Path or URL of the loaded file
    pub path: Option<String>,
    /// Last error reported by mpv or the actor (e.g. file failed to load)
    pub last_error: Option<String>,
    /// Subtitle timing offset in seconds (`sub-delay`; positive = later)
    pub sub_delay: f64,
    /// Secondary subtitle track id (`secondary-sid`), if any
    pub secondary_sid: Option<i64>,
    /// Active hardware decoder (`hwdec-current`), e.g. `vaapi` / `no`
    pub hwdec_current: Option<String>,
    /// Short video pipeline summary from `video-params` (size, format, HDR)
    pub video_info: Option<String>,
}

/// A chapter marker
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub title: String,
    /// Start time in seconds
    pub time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Sub,
    Video,
}

impl TrackKind {
    /// mpv property selecting this kind of track
    pub fn property(self) -> &'static str {
        match self {
            TrackKind::Audio => "aid",
            TrackKind::Sub => "sid",
            TrackKind::Video => "vid",
        }
    }
}

/// An audio, subtitle or video track
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub codec: Option<String>,
    pub selected: bool,
    pub external: bool,
    /// Forced / hearing-impaired style flag from the container
    pub forced: bool,
}

impl Track {
    /// "English (eac3) - Commentary"
    pub fn label(&self) -> String {
        let mut out = match (&self.lang, &self.title) {
            (Some(l), Some(t)) => format!("{} - {t}", lang_name(l)),
            (Some(l), None) => lang_name(l).to_string(),
            (None, Some(t)) => t.clone(),
            (None, None) => format!("Track {}", self.id),
        };
        if let Some(c) = &self.codec {
            out.push_str(&format!(" ({c})"));
        }
        if self.forced {
            out.push_str(" [forced]");
        }
        if self.external {
            out.push_str(" [ext]");
        }
        out
    }
}

fn lang_name(code: &str) -> &str {
    match code {
        "eng" | "en" => "English",
        "cze" | "ces" | "cs" => "Czech",
        "slo" | "slk" | "sk" => "Slovak",
        "ger" | "deu" | "de" => "German",
        "fre" | "fra" | "fr" => "French",
        "spa" | "es" => "Spanish",
        "ita" | "it" => "Italian",
        "pol" | "pl" => "Polish",
        "rus" | "ru" => "Russian",
        "jpn" | "ja" => "Japanese",
        "kor" | "ko" => "Korean",
        "chi" | "zho" | "zh" => "Chinese",
        "por" | "pt" => "Portuguese",
        "hun" | "hu" => "Hungarian",
        "und" => "Unknown",
        other => other,
    }
}

fn parse_chapters(v: Option<&Value>) -> Vec<Chapter> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .enumerate()
                .map(|(i, c)| Chapter {
                    title: c
                        .get("title")
                        .and_then(|t| t.as_str())
                        .filter(|t| !t.is_empty())
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| format!("Chapter {}", i + 1)),
                    time: c.get("time").and_then(|t| t.as_f64()).unwrap_or(0.0),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_tracks(v: Option<&Value>) -> Vec<Track> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let kind = match t.get("type")?.as_str()? {
                        "audio" => TrackKind::Audio,
                        "sub" => TrackKind::Sub,
                        "video" => TrackKind::Video,
                        _ => return None,
                    };
                    let s = |k: &str| t.get(k).and_then(|v| v.as_str()).map(|v| v.to_string());
                    Some(Track {
                        id: t.get("id")?.as_i64()?,
                        kind,
                        title: s("title"),
                        lang: s("lang"),
                        codec: s("codec"),
                        selected: t.get("selected").and_then(|v| v.as_bool()).unwrap_or(false),
                        external: t.get("external").and_then(|v| v.as_bool()).unwrap_or(false),
                        forced: t.get("forced").and_then(|v| v.as_bool()).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_video_info(data: Option<&Value>) -> Option<String> {
    let o = data?.as_object()?;
    let w = o.get("w").and_then(|v| v.as_i64());
    let h = o.get("h").and_then(|v| v.as_i64());
    let pix = o.get("pixelformat").and_then(|v| v.as_str());
    let gamma = o.get("gamma").and_then(|v| v.as_str());
    let prim = o.get("primaries").and_then(|v| v.as_str());
    let mut parts = Vec::new();
    if let (Some(w), Some(h)) = (w, h) {
        parts.push(format!("{w}×{h}"));
    }
    if let Some(p) = pix {
        parts.push(p.to_string());
    }
    let hdr = match gamma {
        Some("pq") | Some("hlg") => true,
        _ => prim.is_some_and(|p| p.contains("2020") || p.contains("bt.2020")),
    };
    if hdr {
        let tag = match gamma {
            Some("pq") => "HDR10/PQ",
            Some("hlg") => "HLG",
            _ => "HDR",
        };
        parts.push(tag.into());
    } else if let Some(g) = gamma {
        parts.push(g.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn parse_sid(data: Option<&Value>) -> Option<i64> {
    match data? {
        Value::Bool(false) => None,
        Value::String(s) if s == "no" || s.is_empty() => None,
        Value::Number(n) => n.as_i64(),
        other => other.as_i64(),
    }
}

/// Messages sent to the background actor
enum PlayerMessage {
    Load { target: String, start: Option<f64> },
    Command(Vec<Value>),
    SetConfig(PlayerConfig),
    Stop,
    Shutdown,
}

/// Player handle. Clone freely; all clones control the same mpv instance.
#[derive(Clone)]
pub struct Player {
    sender: mpsc::UnboundedSender<PlayerMessage>,
    state: Arc<RwLock<PlaybackSnapshot>>,
    config: Arc<RwLock<PlayerConfig>>,
}

impl Player {
    /// Create a new player. Spawns a dedicated background thread; mpv itself is
    /// only started on the first `play*` call.
    pub fn new(config: PlayerConfig) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let state = Arc::new(RwLock::new(PlaybackSnapshot { volume: config.volume as f64, ..Default::default() }));
        let actor_state = state.clone();
        let actor_config = config.clone();

        let spawned = std::thread::Builder::new()
            .name("rmh-player".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(rt) => rt,
                    Err(e) => {
                        error!("Failed to build player runtime: {e}");
                        return;
                    }
                };
                rt.block_on(run_player_actor(receiver, actor_state, actor_config));
            });
        if let Err(e) = spawned {
            error!("Failed to spawn player thread: {e}");
        }

        Self { sender, state, config: Arc::new(RwLock::new(config)) }
    }

    /// Create a player with default configuration
    pub fn with_defaults() -> Self {
        Self::new(PlayerConfig::default())
    }

    /// Check whether the configured mpv binary can be executed. Returns its version line.
    pub fn mpv_version(mpv_path: &str) -> Option<String> {
        let out = std::process::Command::new(mpv_path).arg("--version").output().ok()?;
        if !out.status.success() {
            return None;
        }
        // "mpv 0.37.0 Copyright ..." -> "mpv 0.37.0"
        let first = String::from_utf8_lossy(&out.stdout).lines().next()?.to_string();
        Some(first.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
    }

    pub fn get_config(&self) -> PlayerConfig {
        self.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    /// Update configuration. A running mpv instance is restarted on next play.
    pub fn set_config(&mut self, config: PlayerConfig) {
        if let Ok(mut c) = self.config.write() {
            *c = config.clone();
        }
        let _ = self.sender.send(PlayerMessage::SetConfig(config));
    }

    /// Play a local file
    pub fn play(&mut self, file_path: &Path) -> Result<()> {
        self.play_at_position(file_path, None)
    }

    /// Play a local file starting at a position (seconds)
    pub fn play_at_position(&mut self, file_path: &Path, start_time: Option<u64>) -> Result<()> {
        if !file_path.exists() {
            anyhow::bail!("File not found: {}", file_path.display());
        }
        let path_str = file_path.to_str().context("Path is not valid UTF-8")?.to_string();
        self.send(PlayerMessage::Load { target: path_str, start: start_time.filter(|t| *t > 0).map(|t| t as f64) })
    }

    /// Play a URL (http stream, torrent stream server, ...)
    pub fn play_url(&mut self, url: &str) -> Result<()> {
        self.send(PlayerMessage::Load { target: url.to_string(), start: None })
    }

    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.state.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn get_time_pos(&self) -> Option<f64> {
        Some(self.snapshot().position)
    }

    pub fn get_duration(&self) -> Option<f64> {
        self.snapshot().duration
    }

    pub fn is_paused(&self) -> bool {
        self.snapshot().paused
    }

    pub fn is_active(&self) -> bool {
        self.snapshot().active
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        self.command(vec![json!("set_property"), json!("pause"), json!(paused)])
    }

    pub fn toggle_pause(&self) -> Result<()> {
        self.command(vec![json!("cycle"), json!("pause")])
    }

    /// Seek to an absolute position in seconds
    pub fn seek(&self, position: f64) -> Result<()> {
        if !position.is_finite() {
            anyhow::bail!("Invalid seek position");
        }
        self.command(vec![json!("seek"), json!(position.max(0.0)), json!("absolute")])
    }

    /// Seek relative to the current position (seconds, may be negative)
    pub fn seek_relative(&self, offset: f64) -> Result<()> {
        self.command(vec![json!("seek"), json!(offset), json!("relative")])
    }

    /// Set volume (0-130, mpv's default max)
    pub fn set_volume(&self, volume: f64) -> Result<()> {
        let v = volume.clamp(0.0, 130.0);
        if let Ok(mut s) = self.state.write() {
            s.volume = v;
        }
        if let Ok(mut c) = self.config.write() {
            c.volume = v.min(100.0) as u8;
        }
        self.command(vec![json!("set_property"), json!("volume"), json!(v)])
    }

    /// Jump to chapter `index` (0-based)
    pub fn set_chapter(&self, index: i64) -> Result<()> {
        self.command(vec![json!("set_property"), json!("chapter"), json!(index.max(0))])
    }

    /// Next (+1) / previous (-1) chapter
    pub fn step_chapter(&self, delta: i64) -> Result<()> {
        self.command(vec![json!("add"), json!("chapter"), json!(delta)])
    }

    /// Select a track by id, or disable that kind with `None` (e.g. subtitles off)
    pub fn set_track(&self, kind: TrackKind, id: Option<i64>) -> Result<()> {
        let value = id.map(|i| json!(i)).unwrap_or(json!("no"));
        self.command(vec![json!("set_property"), json!(kind.property()), value])
    }

    /// Secondary subtitle track (`secondary-sid`), or off with `None`
    pub fn set_secondary_sub(&self, id: Option<i64>) -> Result<()> {
        let value = id.map(|i| json!(i)).unwrap_or(json!("no"));
        self.command(vec![json!("set_property"), json!("secondary-sid"), value])
    }

    /// Subtitle timing offset in seconds (positive = show later)
    pub fn set_sub_delay(&self, seconds: f64) -> Result<()> {
        self.command(vec![json!("set_property"), json!("sub-delay"), json!(seconds)])
    }

    /// Only show forced subtitle events (`sub-forced-only`)
    pub fn set_sub_forced_only(&self, on: bool) -> Result<()> {
        self.command(vec![json!("set_property"), json!("sub-forced-only"), json!(if on { "yes" } else { "no" })])
    }

    /// Load an external subtitle file
    pub fn add_subtitle(&self, path: &str) -> Result<()> {
        self.command(vec![json!("sub-add"), json!(path), json!("select")])
    }

    pub fn set_fullscreen(&self, on: bool) -> Result<()> {
        self.command(vec![json!("set_property"), json!("fullscreen"), json!(on)])
    }

    pub fn toggle_fullscreen(&self) -> Result<()> {
        self.command(vec![json!("cycle"), json!("fullscreen")])
    }

    /// Set playback speed (1.0 = normal)
    pub fn set_speed(&self, speed: f64) -> Result<()> {
        self.command(vec![json!("set_property"), json!("speed"), json!(speed.clamp(0.25, 4.0))])
    }

    /// Step one frame forward (`true`) or back; pauses playback
    pub fn frame_step(&self, forward: bool) -> Result<()> {
        self.command(vec![json!(if forward { "frame-step" } else { "frame-back-step" })])
    }

    /// Show text on mpv's OSD
    pub fn show_text(&self, text: &str) -> Result<()> {
        self.command(vec![json!("show-text"), json!(text), json!(1500)])
    }

    pub fn set_muted(&self, muted: bool) -> Result<()> {
        self.command(vec![json!("set_property"), json!("mute"), json!(muted)])
    }

    /// Stop playback and close the mpv window
    pub fn stop(&mut self) -> Result<()> {
        self.send(PlayerMessage::Stop)
    }

    /// Raw mpv IPC command, e.g. `["cycle", "sub"]`
    pub fn command(&self, args: Vec<Value>) -> Result<()> {
        self.send(PlayerMessage::Command(args))
    }

    // Kept for API compatibility with the old libmpv render path
    pub fn get_video_dimensions(&self) -> Option<(i64, i64)> {
        None
    }

    pub fn render_to_buffer(&self, _buffer: &mut [u8], _width: i32, _height: i32, _stride: usize) -> Result<()> {
        Err(anyhow::anyhow!("Software rendering not supported in IPC mode"))
    }

    fn send(&self, msg: PlayerMessage) -> Result<()> {
        self.sender.send(msg).map_err(|_| anyhow::anyhow!("Player actor is not running"))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Last handle gone (the actor holds no Player): shut mpv down.
        if Arc::strong_count(&self.state) <= 2 {
            let _ = self.sender.send(PlayerMessage::Shutdown);
        }
    }
}

type IpcReader = Box<dyn AsyncRead + Unpin + Send>;
type IpcWriter = Box<dyn AsyncWrite + Unpin + Send>;

/// A live mpv process + IPC connection
struct MpvSession {
    child: Child,
    lines: Lines<BufReader<IpcReader>>,
    writer: IpcWriter,
    /// Unix socket path to delete on shutdown; unused on Windows named pipes
    #[cfg_attr(windows, allow(dead_code))]
    ipc_path: Option<PathBuf>,
}

impl MpvSession {
    fn ipc_server_arg() -> (String, Option<PathBuf>) {
        #[cfg(unix)]
        {
            let socket_path = std::env::temp_dir().join(format!("rmh-mpv-{}.sock", Uuid::new_v4()));
            let _ = std::fs::remove_file(&socket_path);
            (format!("--input-ipc-server={}", socket_path.display()), Some(socket_path))
        }
        #[cfg(windows)]
        {
            let name = format!(r"\\.\pipe\aurivon-mpv-{}", Uuid::new_v4());
            (format!("--input-ipc-server={name}"), None)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let socket_path = std::env::temp_dir().join(format!("rmh-mpv-{}.sock", Uuid::new_v4()));
            (format!("--input-ipc-server={}", socket_path.display()), Some(socket_path))
        }
    }

    async fn connect_ipc(ipc_arg: &str, ipc_path: &Option<PathBuf>, child: &mut Child) -> Result<(IpcReader, IpcWriter)> {
        for _ in 0..100 {
            if let Ok(Some(status)) = child.try_wait() {
                anyhow::bail!("mpv exited immediately ({status})");
            }
            #[cfg(unix)]
            {
                let _ = ipc_arg;
                if let Some(path) = ipc_path {
                    if let Ok(s) = tokio::net::UnixStream::connect(path).await {
                        let (r, w) = s.into_split();
                        return Ok((Box::new(r), Box::new(w)));
                    }
                }
            }
            #[cfg(windows)]
            {
                let _ = ipc_path;
                // `--input-ipc-server=\\.\pipe\name` → open the same pipe as client
                let pipe = ipc_arg.strip_prefix("--input-ipc-server=").unwrap_or(ipc_arg);
                match tokio::net::windows::named_pipe::ClientOptions::new().open(pipe) {
                    Ok(s) => {
                        let (r, w) = tokio::io::split(s);
                        return Ok((Box::new(r), Box::new(w)));
                    }
                    Err(_) => {}
                }
            }
            #[cfg(not(any(unix, windows)))]
            {
                let _ = (ipc_arg, ipc_path);
            }
            sleep(Duration::from_millis(50)).await;
        }
        anyhow::bail!("Timed out connecting to mpv IPC")
    }

    async fn start(config: &PlayerConfig) -> Result<Self> {
        let (ipc_arg, ipc_path) = Self::ipc_server_arg();

        let mut args = vec![
            ipc_arg.clone(),
            "--idle=yes".to_string(),
            format!("--hwdec={}", config.hwdec),
            format!("--volume={}", config.volume),
            format!("--osc={}", if config.osc { "yes" } else { "no" }),
            "--keep-open=yes".to_string(),
            "--cache=yes".to_string(),
            "--cache-secs=120".to_string(),
            "--demuxer-max-bytes=500MiB".to_string(),
            "--script-opts=osc-seekbarstyle=bar,osc-hidetimeout=1200,osc-idlescreen=no,osc-deadzonesize=0.2".to_string(),
            "--input-default-bindings=yes".to_string(),
            "--title=Aurivon Player - ${media-title}".to_string(),
            "--no-terminal".to_string(),
        ];
        if config.headless {
            args.push("--vo=null".into());
            args.push("--ao=null".into());
        } else {
            args.push("--force-window=yes".into());
        }
        if config.save_position {
            args.push("--save-position-on-quit".into());
        }
        if let Some(wid) = config.wid {
            args.push(format!("--wid={wid}"));
            args.push("--input-cursor=yes".into());
            args.push("--input-vo-keyboard=no".into());
            args.push("--cursor-autohide=1000".into());
        }
        if config.fullscreen {
            args.push("--fs".into());
        }
        args.extend(config.extra_args.iter().cloned());

        info!("Starting mpv: {} {:?}", config.mpv_path, args);
        let mut child = Command::new(&config.mpv_path)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("Failed to start '{}'. Is mpv installed?", config.mpv_path))?;

        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!("mpv: {}", line);
                }
            });
        }

        let (r, w) = Self::connect_ipc(&ipc_arg, &ipc_path, &mut child).await?;
        let mut session = Self { child, lines: BufReader::new(r).lines(), writer: w, ipc_path };

        for (id, prop) in [
            "time-pos",
            "duration",
            "pause",
            "volume",
            "mute",
            "eof-reached",
            "idle-active",
            "media-title",
            "chapter-list",
            "chapter",
            "track-list",
            "fullscreen",
            "speed",
            "path",
            "sub-delay",
            "secondary-sid",
            "hwdec-current",
            "video-params",
        ]
        .iter()
        .enumerate()
        {
            session.send(vec![json!("observe_property"), json!(id + 1), json!(prop)]).await?;
        }
        Ok(session)
    }

    async fn send(&mut self, command: Vec<Value>) -> Result<()> {
        let mut line = json!({ "command": command }).to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.context("mpv IPC write failed")
    }

    async fn send_named(&mut self, command: Value) -> Result<()> {
        let mut line = json!({ "command": command }).to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.context("mpv IPC write failed")
    }

    async fn shutdown(mut self) {
        let _ = self.send(vec![json!("quit")]).await;
        if timeout(Duration::from_secs(2), self.child.wait()).await.is_err() {
            let _ = self.child.kill().await;
        }
        if let Some(path) = &self.ipc_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn run_player_actor(
    mut receiver: mpsc::UnboundedReceiver<PlayerMessage>,
    state: Arc<RwLock<PlaybackSnapshot>>,
    mut config: PlayerConfig,
) {
    let mut session: Option<MpvSession> = None;

    let set_inactive = |state: &Arc<RwLock<PlaybackSnapshot>>| {
        if let Ok(mut s) = state.write() {
            let volume = s.volume;
            let last_error = s.last_error.take();
            *s = PlaybackSnapshot { volume, last_error, ..Default::default() };
        }
    };

    loop {
        tokio::select! {
            msg = receiver.recv() => {
                let Some(msg) = msg else { break };
                match msg {
                    PlayerMessage::Load { target, start } => {
                        if session.is_none() {
                            match MpvSession::start(&config).await {
                                Ok(s) => {
                                    session = Some(s);
                                    if let Ok(mut st) = state.write() { st.active = true; st.last_error = None; }
                                }
                                Err(e) => {
                                    error!("{e:#}");
                                    if let Ok(mut st) = state.write() { st.last_error = Some(format!("{e:#}")); }
                                    continue;
                                }
                            }
                        }
                        if let Ok(mut st) = state.write() {
                            st.chapters.clear();
                            st.chapter = None;
                            st.tracks.clear();
                            st.position = 0.0;
                            st.duration = None;
                            st.eof = false;
                            st.loaded = true;
                            st.last_error = None;
                        }
                        let s = session.as_mut().unwrap();
                        // Named arguments: works across mpv versions (0.38 added an
                        // `index` positional arg to loadfile that broke positional options).
                        let mut cmd = json!({ "name": "loadfile", "url": target, "flags": "replace" });
                        if let Some(t) = start {
                            cmd["options"] = json!({ "start": format!("{t}") });
                        }
                        if let Err(e) = s.send_named(cmd).await {
                            warn!("{e:#}");
                        }
                        // Make sure we are not stuck paused from a previous file
                        let _ = s.send(vec![json!("set_property"), json!("pause"), json!(false)]).await;
                    }
                    PlayerMessage::Command(args) => {
                        if let Some(s) = session.as_mut() {
                            if let Err(e) = s.send(args).await { warn!("{e:#}"); }
                        }
                    }
                    PlayerMessage::SetConfig(new_config) => {
                        let restart = session.is_some()
                            && (new_config.wid != config.wid
                                || new_config.mpv_path != config.mpv_path
                                || new_config.headless != config.headless
                                || new_config.hwdec != config.hwdec);
                        config = new_config;
                        if restart {
                            if let Some(s) = session.take() { s.shutdown().await; }
                            set_inactive(&state);
                        }
                    }
                    PlayerMessage::Stop => {
                        if let Some(s) = session.take() { s.shutdown().await; }
                        set_inactive(&state);
                    }
                    PlayerMessage::Shutdown => break,
                }
            }

            // `next_line` is cancel-safe, so partial lines survive select! cancellation.
            line = async {
                match session.as_mut() {
                    Some(s) => s.lines.next_line().await,
                    None => std::future::pending().await,
                }
            } => {
                match line {
                    Ok(Some(line)) => {
                        if let Ok(val) = serde_json::from_str::<Value>(&line) {
                            handle_ipc_event(&val, &state);
                        }
                    }
                    Ok(None) | Err(_) => {
                        info!("mpv closed");
                        if let Some(mut s) = session.take() {
                            let _ = timeout(Duration::from_secs(1), s.child.wait()).await;
                            if let Some(path) = &s.ipc_path {
                                let _ = std::fs::remove_file(path);
                            }
                        }
                        set_inactive(&state);
                    }
                }
            }
        }
    }

    if let Some(s) = session.take() {
        s.shutdown().await;
    }
}

fn handle_ipc_event(val: &Value, state: &Arc<RwLock<PlaybackSnapshot>>) {
    let Some(event) = val.get("event").and_then(|v| v.as_str()) else {
        if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
            if err != "success" {
                debug!("mpv command error: {err}");
            }
        }
        return;
    };
    let Ok(mut s) = state.write() else { return };
    match event {
        "property-change" => {
            let data = val.get("data");
            match val.get("name").and_then(|v| v.as_str()).unwrap_or("") {
                "time-pos" => s.position = data.and_then(|v| v.as_f64()).unwrap_or(s.position),
                "duration" => s.duration = data.and_then(|v| v.as_f64()),
                "pause" => s.paused = data.and_then(|v| v.as_bool()).unwrap_or(false),
                "volume" => s.volume = data.and_then(|v| v.as_f64()).unwrap_or(s.volume),
                "mute" => s.muted = data.and_then(|v| v.as_bool()).unwrap_or(false),
                "eof-reached" => s.eof = data.and_then(|v| v.as_bool()).unwrap_or(false),
                "idle-active" => {
                    if data.and_then(|v| v.as_bool()).unwrap_or(false) {
                        s.loaded = false;
                    }
                }
                "media-title" => s.title = data.and_then(|v| v.as_str()).map(|t| t.to_string()),
                "chapter-list" => s.chapters = parse_chapters(data),
                "chapter" => s.chapter = data.and_then(|v| v.as_i64()).filter(|c| *c >= 0),
                "track-list" => s.tracks = parse_tracks(data),
                "fullscreen" => s.fullscreen = data.and_then(|v| v.as_bool()).unwrap_or(false),
                "speed" => s.speed = data.and_then(|v| v.as_f64()).unwrap_or(1.0),
                "path" => s.path = data.and_then(|v| v.as_str()).map(|t| t.to_string()),
                "sub-delay" => s.sub_delay = data.and_then(|v| v.as_f64()).unwrap_or(0.0),
                "secondary-sid" => s.secondary_sid = parse_sid(data),
                "hwdec-current" => {
                    s.hwdec_current = data.and_then(|v| v.as_str()).map(|t| t.to_string()).filter(|t| !t.is_empty() && t != "no")
                }
                "video-params" => s.video_info = parse_video_info(data),
                _ => {}
            }
        }
        "file-loaded" => {
            s.loaded = true;
            s.last_error = None;
        }
        "end-file" => {
            if val.get("reason").and_then(|v| v.as_str()) == Some("error") {
                let msg = val.get("file_error").and_then(|v| v.as_str()).unwrap_or("unknown error");
                s.last_error = Some(format!("mpv could not play file: {msg}"));
                s.loaded = false;
            }
        }
        "shutdown" => s.active = false,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_property_changes() {
        let state = Arc::new(RwLock::new(PlaybackSnapshot::default()));
        handle_ipc_event(&json!({"event":"property-change","name":"time-pos","data":12.5}), &state);
        handle_ipc_event(&json!({"event":"property-change","name":"duration","data":60.0}), &state);
        handle_ipc_event(&json!({"event":"property-change","name":"pause","data":true}), &state);
        handle_ipc_event(&json!({"event":"property-change","name":"volume","data":42.0}), &state);
        handle_ipc_event(&json!({"event":"end-file","reason":"error","file_error":"unrecognized file format"}), &state);
        let s = state.read().unwrap().clone();
        assert_eq!(s.position, 12.5);
        assert_eq!(s.duration, Some(60.0));
        assert!(s.paused);
        assert_eq!(s.volume, 42.0);
        assert!(s.last_error.unwrap().contains("unrecognized"));
    }

    #[test]
    fn parses_chapters_and_tracks() {
        let state = Arc::new(RwLock::new(PlaybackSnapshot::default()));
        handle_ipc_event(&json!({"event":"property-change","name":"chapter-list","data":[
            {"title":"Opening","time":0.0},{"time":95.5}]}), &state);
        handle_ipc_event(&json!({"event":"property-change","name":"chapter","data":1}), &state);
        handle_ipc_event(&json!({"event":"property-change","name":"track-list","data":[
            {"id":1,"type":"video","codec":"h264","selected":true},
            {"id":1,"type":"audio","lang":"eng","codec":"eac3","selected":true},
            {"id":2,"type":"audio","lang":"cze","title":"Dabing","codec":"ac3","selected":false},
            {"id":1,"type":"sub","lang":"eng","codec":"subrip","external":true,"selected":false}]}), &state);
        let s = state.read().unwrap().clone();
        assert_eq!(s.chapters, vec![Chapter { title: "Opening".into(), time: 0.0 }, Chapter { title: "Chapter 2".into(), time: 95.5 }]);
        assert_eq!(s.chapter, Some(1));
        assert_eq!(s.tracks.len(), 4);
        let cz = s.tracks.iter().find(|t| t.kind == TrackKind::Audio && t.id == 2).unwrap();
        assert_eq!(cz.label(), "Czech - Dabing (ac3)");
        assert!(s.tracks.iter().any(|t| t.kind == TrackKind::Sub && t.external));
    }

    #[test]
    fn missing_file_is_rejected() {
        let mut p = Player::with_defaults();
        assert!(p.play(Path::new("/definitely/not/here.mkv")).is_err());
    }
}
