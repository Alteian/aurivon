//! UI-agnostic video/audio player built on the `mpv` executable and its JSON IPC.
//!
//! The player runs `mpv` as a separate process and talks to it over a Unix
//! socket (`--input-ipc-server`). This keeps the GUI process isolated from any
//! FFI/EGL issues and works on X11 and Wayland alike.
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
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
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
    /// X11 window ID to embed into (`--wid`). `None` = separate mpv window.
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
    /// Last error reported by mpv or the actor (e.g. file failed to load)
    pub last_error: Option<String>,
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

/// A live mpv process + IPC connection
struct MpvSession {
    child: Child,
    lines: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
    socket_path: PathBuf,
}

impl MpvSession {
    async fn start(config: &PlayerConfig) -> Result<Self> {
        let socket_path = std::env::temp_dir().join(format!("rmh-mpv-{}.sock", Uuid::new_v4()));
        let _ = std::fs::remove_file(&socket_path);

        let mut args = vec![
            format!("--input-ipc-server={}", socket_path.display()),
            "--idle=yes".to_string(),
            format!("--hwdec={}", config.hwdec),
            format!("--volume={}", config.volume),
            format!("--osc={}", if config.osc { "yes" } else { "no" }),
            "--keep-open=yes".to_string(),
            "--input-default-bindings=yes".to_string(),
            "--title=RustMediaHub Player - ${media-title}".to_string(),
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
            .with_context(|| format!("Failed to start '{}'. Is mpv installed (sudo apt install mpv)?", config.mpv_path))?;

        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!("mpv: {}", line);
                }
            });
        }

        // Wait for the IPC socket (up to ~5s)
        let mut stream = None;
        for _ in 0..100 {
            if let Ok(Some(status)) = child.try_wait() {
                anyhow::bail!("mpv exited immediately ({status})");
            }
            if let Ok(s) = UnixStream::connect(&socket_path).await {
                stream = Some(s);
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
        let stream = stream.context("Timed out connecting to mpv IPC socket")?;
        let (r, w) = stream.into_split();
        let mut session = Self { child, lines: BufReader::new(r).lines(), writer: w, socket_path };

        for (id, prop) in [
            "time-pos", "duration", "pause", "volume", "mute", "eof-reached", "idle-active", "media-title",
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
        let _ = std::fs::remove_file(&self.socket_path);
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
                            && (new_config.wid != config.wid || new_config.mpv_path != config.mpv_path
                                || new_config.headless != config.headless);
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
                            let _ = std::fs::remove_file(&s.socket_path);
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
    fn missing_file_is_rejected() {
        let mut p = Player::with_defaults();
        assert!(p.play(Path::new("/definitely/not/here.mkv")).is_err());
    }
}
