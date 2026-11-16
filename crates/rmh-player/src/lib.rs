//! Video player integration using MPV (via IPC)
//! 
//! This module uses the `mpv` executable via JSON IPC to ensure robust playback on all displays (X11, Wayland)
//! without crashing the main application due to FFI/EGL conflicts.
//! 
//! Architecture:
//! - `Player` struct is a handle with a `Sender` to a background task.
//! - The background task (Actor) manages the `mpv` process and IPC socket.
//! - State (position, duration, paused) is shared via `Arc<RwLock>` or `Arc<Mutex>` for synchronous access by UI.

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::{info, error};
use uuid::Uuid;

/// Configuration for MPV player
#[derive(Debug, Clone)]
pub struct PlayerConfig {
    /// Hardware decoding mode (auto, vaapi, nvdec, etc.)
    pub hwdec: String,
    /// Start in fullscreen mode
    pub fullscreen: bool,
    /// Show on-screen controller
    pub osc: bool,
    /// Volume level (0-100)
    pub volume: u8,
    /// Resume from last position
    pub save_position: bool,
    /// Window ID to embed into (not used with IPC currently, but kept for API compat)
    pub wid: Option<i64>,
    /// Additional custom arguments
    pub extra_args: Vec<String>,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            hwdec: "auto".to_string(),
            fullscreen: true,
            osc: true,
            volume: 100,
            save_position: true,
            wid: None,
            extra_args: Vec::new(),
        }
    }
}

/// Shared state accessible by UI synchronously
struct SharedState {
    position: RwLock<f64>,
    duration: RwLock<Option<f64>>,
    paused: RwLock<bool>,
    active: RwLock<bool>,
}

/// Messages sent to the background actor
enum PlayerMessage {
    PlayFile(String, Option<u64>), // path, start_time
    PlayUrl(String),
    Command(String, Vec<Value>),
    SetConfig(PlayerConfig),
    Stop,
}

/// Video player handle
#[derive(Clone)]
pub struct Player {
    sender: mpsc::UnboundedSender<PlayerMessage>,
    state: Arc<SharedState>,
    // We store config locally too for get/modify, but send updates to actor
    config: Arc<RwLock<PlayerConfig>>,
}

unsafe impl Send for Player {}
unsafe impl Sync for Player {}

impl Player {
    /// Create a new player with the given configuration
    pub fn new(config: PlayerConfig) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let state = Arc::new(SharedState {
            position: RwLock::new(0.0),
            duration: RwLock::new(None),
            paused: RwLock::new(false),
            active: RwLock::new(false),
        });
        
        // Clone for listener
        let state_clone = state.clone();
        let config_clone = config.clone();

        // Spawn background task
        // We use tokio::spawn, so this must be called within a runtime context or one must exist.
        // If compilation fails or runtime panic happens, we might need a dedicated thread, 
        // but `iced` usually provides the runtime.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                run_player_actor(receiver, state_clone, config_clone).await;
            });
        } else {
            // Fallback: spawn a thread that builds a runtime? 
            // Better to assume we are in a runtime for now.
            // If try_current fails, we'll log it.
            // Actually, we can use std::thread::spawn and create a basic runtime if needed,
            // but let's assume valid environment.
            error!("Could not find Tokio runtime. Player actor not spawned!");
        }

        Self {
            sender,
            state,
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Create a player with default configuration
    pub fn with_defaults() -> Self {
        Self::new(PlayerConfig::default())
    }

    /// Get current configuration
    pub fn get_config(&self) -> PlayerConfig {
        self.config.read().unwrap().clone()
    }

    /// Update the player configuration
    pub fn set_config(&mut self, config: PlayerConfig) {
        if let Ok(mut c) = self.config.write() {
            *c = config.clone();
        }
        let _ = self.sender.send(PlayerMessage::SetConfig(config));
    }

    /// Play a video file
    pub fn play(&mut self, file_path: &Path) -> Result<()> {
        self.play_at_position(file_path, None)
    }

    /// Play a video file starting at specific position (seconds)
    pub fn play_at_position(&mut self, file_path: &Path, start_time: Option<u64>) -> Result<()> {
        let path_str = file_path.to_str().context("Invalid path")?.to_string();
        self.sender.send(PlayerMessage::PlayFile(path_str, start_time))
            .map_err(|_| anyhow::anyhow!("Player actor dead"))
    }

    /// Play a URL
    pub fn play_url(&mut self, url: &str) -> Result<()> {
        self.sender.send(PlayerMessage::PlayUrl(url.to_string()))
             .map_err(|_| anyhow::anyhow!("Player actor dead"))
    }
    
    pub fn get_time_pos(&self) -> Option<f64> {
         self.state.position.read().ok().map(|v| *v)
    }

    pub fn get_duration(&self) -> Option<f64> {
         self.state.duration.read().ok().and_then(|v| *v)
    }

    pub fn is_paused(&self) -> bool {
         self.state.paused.read().ok().map(|v| *v).unwrap_or(false)
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        let cmd = PlayerMessage::Command("set_property".to_string(), vec![Value::String("pause".to_string()), Value::Bool(paused)]);
        self.sender.send(cmd).map_err(|_| anyhow::anyhow!("Player actor dead"))
    }

    pub fn seek(&self, position: f64) -> Result<()> {
        let val = serde_json::Number::from_f64(position)
            .ok_or_else(|| anyhow::anyhow!("Invalid seek position"))?;
        let cmd = PlayerMessage::Command("set_property".to_string(), vec![Value::String("time-pos".to_string()), Value::Number(val)]);
        self.sender.send(cmd).map_err(|_| anyhow::anyhow!("Player actor dead"))
    }

    pub fn is_active(&self) -> bool {
         self.state.active.read().ok().map(|v| *v).unwrap_or(false)
    }

    /// Stop playback
    pub fn stop(&mut self) -> Result<()> {
         self.sender.send(PlayerMessage::Stop).map_err(|_| anyhow::anyhow!("Player actor dead"))
    }

    // Dummy method for API compatibility
    pub fn get_video_dimensions(&self) -> Option<(i64, i64)> {
        None 
    }
    
    // Dummy method
    pub fn render_to_buffer(&self, _buffer: &mut [u8], _width: i32, _height: i32, _stride: usize) -> Result<()> {
        Err(anyhow::anyhow!("Software rendering not supported in IPC mode"))
    }
}

/// The Actor Loop
async fn run_player_actor(
    mut receiver: mpsc::UnboundedReceiver<PlayerMessage>,
    state: Arc<SharedState>,
    mut config: PlayerConfig
) {
    let mut mpv_process: Option<Child> = None;
    let mut ipc_stream: Option<UnixStream> = None;
    let mut ipc_reader: Option<BufReader<tokio::net::unix::OwnedReadHalf>> = None;
    let mut ipc_writer: Option<tokio::net::unix::OwnedWriteHalf> = None;

    let unique_id = Uuid::new_v4();
    let socket_path = std::env::temp_dir().join(format!("rmh-mpv-{}.sock", unique_id));

    // loop over messages (and ipc events if connected)
    loop {
        // Prepare futures
        // We need to poll the receiver AND the ipc_reader (if active)
        
        let mut line_buffer = String::new();
        
        tokio::select! {
            msg = receiver.recv() => {
                match msg {
                    Some(m) => {
                        // Handle command
                        match m {
                            PlayerMessage::PlayFile(path, start) => {
                                // Ensure Running
                                if mpv_process.is_none() {
                                    start_mpv(&mut mpv_process, &config, &socket_path).await;
                                    connect_ipc(&mut ipc_stream, &mut ipc_reader, &mut ipc_writer, &socket_path, &state).await;
                                }
                                
                                let mut args = vec![Value::String(path), Value::String("replace".to_string())];
                                if let Some(t) = start {
                                    args.push(Value::String(format!("start={}", t)));
                                }
                                send_ipc("loadfile", args, &mut ipc_writer).await;
                            },
                             PlayerMessage::PlayUrl(url) => {
                                if mpv_process.is_none() {
                                    start_mpv(&mut mpv_process, &config, &socket_path).await;
                                    connect_ipc(&mut ipc_stream, &mut ipc_reader, &mut ipc_writer, &socket_path, &state).await;
                                }
                                send_ipc("loadfile", vec![Value::String(url), Value::String("replace".to_string())], &mut ipc_writer).await;
                             },
                             PlayerMessage::Command(cmd, args) => {
                                 if mpv_process.is_some() {
                                     send_ipc(&cmd, args, &mut ipc_writer).await;
                                 }
                             },
                             PlayerMessage::SetConfig(new_config) => {
                                 config = new_config;
                                 ipc_writer = None;
                                 ipc_reader = None;
                                 ipc_stream = None;
                                 if let Some(mut child) = mpv_process.take() {
                                     let _ = child.kill().await;
                                 }
                                 if let Ok(mut w) = state.active.write() { *w = false; }
                                 // Next play will restart with new config
                             },
                             PlayerMessage::Stop => {
                                 if mpv_process.is_some() {
                                     send_ipc("quit", vec![], &mut ipc_writer).await;
                                     // wait for it to die?
                                     // we rely on shutdown event or wait
                                     // Reset
                                     ipc_writer = None;
                                     ipc_reader = None;
                                     ipc_stream = None;
                                     mpv_process = None; // Assume it quits
                                     if let Ok(mut w) = state.active.write() { *w = false; }
                                 }
                             }
                        }
                    },
                    None => break, // Channel closed
                }
            }
            
            // Read from IPC if connected
            res = async {
                 if let Some(reader) = ipc_reader.as_mut() {
                     reader.read_line(&mut line_buffer).await
                 } else {
                     std::future::pending().await
                 }
            } => {
                 match res {
                     Ok(0) => {
                         // EOF
                         ipc_reader = None;
                         ipc_writer = None;
                         ipc_stream = None;
                         mpv_process = None;
                         if let Ok(mut w) = state.active.write() { *w = false; }
                     },
                     Ok(_) => {
                         // Parse JSON
                         if let Ok(val) = serde_json::from_str::<Value>(&line_buffer) {
                             handle_ipc_event(&val, &state);
                         }
                         line_buffer.clear();
                     },
                     Err(_) => {
                         // Error
                         ipc_reader = None;
                     }
                 }
            }
        }
    }
    
    // Cleanup
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
}

async fn start_mpv(process: &mut Option<Child>, config: &PlayerConfig, socket_path: &Path) {
    // cleanup old socket
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }

    let mut args = vec![
            format!("--input-ipc-server={}", socket_path.to_string_lossy()),
            "--idle=yes".to_string(), 
            format!("--hwdec={}", config.hwdec),
            format!("--volume={}", config.volume),
            format!("--osc={}", if config.osc { "yes" } else { "no" }),
            "--keep-open=yes".to_string(),
            format!("--force-window=yes"),
    ];
    
    if let Some(wid) = config.wid {
        args.push(format!("--wid={}", wid));
        // When embedding, we might want to disable OSC if we draw our own, 
        // but for now keep it or let user config decide.
        // We also want to bind 'q', 'ESC', 'BACKSPACE' to quit
        args.push("--input-conf=/dev/null".to_string()); // reset bindings
        args.push("--input-commands=q quit;ESC quit;BS quit".to_string());
    }

    if config.fullscreen {
        // If embedded, fullscreen applies to the embedded viewport, which is what we want.
        // But if floating, it makes the window FS.
        // If embedding, usually we resize the player to fill the area.
        args.push("--fs".to_string());
    }
    args.extend(config.extra_args.clone());

    info!("Starting MPV: {:?}", args);
    match Command::new("mpv")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null()) 
            .stderr(Stdio::piped()) 
            .spawn() {
        Ok(mut child) => {
            if let Some(stderr) = child.stderr.take() {
                tokio::spawn(async move {
                     let reader = BufReader::new(stderr);
                     let mut lines = reader.lines();
                     while let Ok(Some(line)) = lines.next_line().await {
                         info!("MPV: {}", line);
                     }
                });
            }
            *process = Some(child);
        },
        Err(e) => {
            error!("Failed to spawn mpv: {}", e);
        }
    }
}

async fn connect_ipc(
    stream_store: &mut Option<UnixStream>,
    reader_store: &mut Option<BufReader<tokio::net::unix::OwnedReadHalf>>,
    writer_store: &mut Option<tokio::net::unix::OwnedWriteHalf>,
    socket_path: &Path,
    state: &Arc<SharedState>
) {
    // Retry loop
    for _ in 0..50 {
        if socket_path.exists() {
            if let Ok(stream) = UnixStream::connect(socket_path).await {
                let (r, w) = stream.into_split();
                *stream_store = None; // We split it, so we don't keep the original stream in this field if we use owned halves
                                      // Actually we don't need stream_store if we have r/w
                *reader_store = Some(BufReader::new(r));
                *writer_store = Some(w);
                
                if let Ok(mut w) = state.active.write() { *w = true; }
                
                // Init observations
                send_ipc("observe_property", vec![Value::Number(1.into()), Value::String("time-pos".into())], writer_store).await;
                send_ipc("observe_property", vec![Value::Number(2.into()), Value::String("duration".into())], writer_store).await;
                send_ipc("observe_property", vec![Value::Number(3.into()), Value::String("pause".into())], writer_store).await;
                return;
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
    error!("Failed to connect to MPV IPC");
}

async fn send_ipc(command: &str, args: Vec<Value>, writer: &mut Option<tokio::net::unix::OwnedWriteHalf>) {
    if let Some(w) = writer {
        let mut cmd_vec = vec![Value::String(command.to_string())];
        cmd_vec.extend(args);
        let json_cmd = serde_json::json!({ "command": cmd_vec });
        let mut str_cmd = json_cmd.to_string();
        str_cmd.push('\n');
        let _ = w.write_all(str_cmd.as_bytes()).await;
    }
}

fn handle_ipc_event(val: &Value, state: &Arc<SharedState>) {
    // Parse "event": "property-change"
    if let Some(event) = val.get("event").and_then(|v| v.as_str()) {
        if event == "property-change" {
             if let Some(name) = val.get("name").and_then(|v| v.as_str()) {
                 match name {
                     "time-pos" => {
                         if let Some(pos) = val.get("data").and_then(|v| v.as_f64()) {
                             if let Ok(mut w) = state.position.write() { *w = pos; }
                         }
                     },
                     "duration" => {
                         if let Some(dur) = val.get("data").and_then(|v| v.as_f64()) {
                             if let Ok(mut w) = state.duration.write() { *w = Some(dur); }
                         }
                     },
                     "pause" => {
                         if let Some(p) = val.get("data").and_then(|v| v.as_bool()) {
                             if let Ok(mut w) = state.paused.write() { *w = p; }
                         }
                     },
                     _ => {}
                 }
             }
        } else if event == "shutdown" {
             if let Ok(mut w) = state.active.write() { *w = false; }
        }
    }
}

