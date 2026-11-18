//! Torrent streaming engine using librqbit
//!
//! Provides Stremio-like sequential downloading for instant video playback.

use crate::error::{TorrentError, TorrentResult};
use crate::server::{StreamServer, StreamableFile};
use crate::stream::{StreamInfo, StreamStatus, TorrentHandle, TorrentStream};
use crate::find_best_video_file;
use librqbit::api::TorrentIdOrHash;
use librqbit::{AddTorrent, AddTorrentOptions, ManagedTorrent, Session, SessionOptions};
use std::collections::{HashMap, HashSet};
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Well-known public trackers, added to every torrent so peers show up in a
/// second or two instead of waiting for the DHT to warm up.
pub const DEFAULT_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
];

/// Lowercase hex info hash of a magnet link
fn magnet_hash(magnet: &str) -> Option<String> {
    let lower = magnet.to_lowercase();
    let start = lower.find("xt=urn:btih:")? + "xt=urn:btih:".len();
    let hash: String = lower[start..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (hash.len() == 40).then_some(hash)
}

/// `magnet` plus [`DEFAULT_TRACKERS`] it does not list yet
pub fn with_default_trackers(magnet: &str) -> String {
    let mut out = magnet.to_string();
    if !magnet.starts_with("magnet:") || std::env::var("AURIVON_BARE_MAGNET").is_ok() {
        return out;
    }
    for t in DEFAULT_TRACKERS {
        let enc: String = t
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect();
        if !magnet.contains(&enc) && !magnet.contains(t) {
            out.push_str("&tr=");
            out.push_str(&enc);
        }
    }
    out
}

/// Progress of [`TorrentEngine::prebuffer`]
#[derive(Debug, Clone, Copy, Default)]
pub struct BufferProgress {
    /// Bytes of the start/end of the file already read
    pub buffered: u64,
    /// Bytes needed before playback starts
    pub target: u64,
    /// Download speed, bytes/s
    pub download_speed: u64,
    /// Connected peers
    pub peers: u32,
    /// Peers known (trackers + DHT)
    pub seen: u32,
}

/// Configuration for the torrent engine
#[derive(Debug, Clone)]
pub struct TorrentEngineConfig {
    /// Directory for downloading torrents
    pub download_dir: PathBuf,
    
    /// Port for the HTTP streaming server
    pub server_port: u16,
    
    /// Enable DHT (Distributed Hash Table)
    pub enable_dht: bool,
    
    /// Enable uPnP port forwarding
    pub enable_upnp: bool,
    
    /// Percentage to buffer before marking as ready (0.0 - 1.0)
    pub buffer_percent: f64,
}

impl Default for TorrentEngineConfig {
    fn default() -> Self {
        let download_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("aurivon")
            .join("torrents");

        Self {
            download_dir,
            server_port: 9999,
            enable_dht: true,
            enable_upnp: false,
            buffer_percent: 0.02, // 2% buffer before playing
        }
    }
}

/// Active torrent with its metadata
struct ActiveTorrent {
    handle: Arc<ManagedTorrent>,
    info_hash: String,
    file_index: usize,
    filename: String,
    size: u64,
    #[allow(dead_code)]
    stream_id: String,
}

/// The main torrent streaming engine
pub struct TorrentEngine {
    config: TorrentEngineConfig,
    session: Option<Arc<Session>>,
    server: StreamServer,
    active_torrents: Arc<RwLock<HashMap<String, ActiveTorrent>>>,
}

impl TorrentEngine {
    /// Create a new torrent engine with the given configuration
    pub fn new(config: TorrentEngineConfig) -> Self {
        let server = StreamServer::new(config.server_port);
        
        Self {
            config,
            session: None,
            server,
            active_torrents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(TorrentEngineConfig::default())
    }

    /// Initialize the engine (must be called before use)
    pub async fn initialize(&mut self) -> TorrentResult<()> {
        // Create download directory
        tokio::fs::create_dir_all(&self.config.download_dir)
            .await
            .map_err(TorrentError::Io)?;

        info!("Download directory: {:?}", self.config.download_dir);

        // Configure librqbit session
        let session_opts = SessionOptions {
            disable_dht: !self.config.enable_dht,
            disable_dht_persistence: true,
            enable_upnp_port_forwarding: self.config.enable_upnp,
            ..Default::default()
        };

        // Create session
        let session = Session::new_with_opts(
            self.config.download_dir.clone(),
            session_opts,
        )
        .await
        .map_err(|e| TorrentError::Internal(e.to_string()))?;

        self.session = Some(session);

        // Start HTTP streaming server
        self.server.start().await?;

        info!("Torrent engine initialized");
        info!("Streaming server at: {}", self.server.base_url());

        Ok(())
    }

    /// Add a magnet link and start streaming
    pub async fn add_magnet(
        &self,
        magnet: &str,
        file_index: Option<usize>,
    ) -> TorrentResult<TorrentStream> {
        let session = self.session.as_ref()
            .ok_or(TorrentError::NotInitialized)?;

        info!("Adding magnet: {}...", &magnet[..magnet.len().min(60)]);
        // librqbit ignores `AddTorrentOptions::trackers` for magnets, so the
        // fallback trackers go into the link itself
        let magnet = &with_default_trackers(magnet);

        // A torrent whose metadata was prefetched is already in the session
        // (paused): reuse it instead of resolving the magnet again
        let handle = match self.find_by_magnet(magnet) {
            Some(h) => h,
            None => {
                let add_opts = AddTorrentOptions { initial_peers: None, overwrite: true, ..Default::default() };
                session
                    .add_torrent(AddTorrent::from_url(magnet), Some(add_opts))
                    .await
                    .map_err(|e| TorrentError::Internal(e.to_string()))?
                    .into_handle()
                    .ok_or_else(|| TorrentError::DownloadFailed("Failed to get handle".into()))?
            }
        };

        // Wait for metadata
        info!("Waiting for torrent metadata...");
        handle.wait_until_initialized()
            .await
            .map_err(|e| TorrentError::DownloadFailed(e.to_string()))?;
        if handle.is_paused() {
            session.unpause(&handle).await.map_err(|e| TorrentError::Internal(e.to_string()))?;
        }

        // Get torrent info using with_metadata
        let (torrent_name, info_hash_bytes, files) = handle
            .with_metadata(|metadata| {
                let name = metadata.name.clone().unwrap_or_else(|| "unknown".to_string());
                let hash = handle.info_hash();
                
                // Get file list from file_infos
                let files: Vec<(usize, String, u64)> = metadata.file_infos
                    .iter()
                    .enumerate()
                    .map(|(i, fi)| {
                        let filename = fi.relative_filename.to_string_lossy().to_string();
                        (i, filename, fi.len)
                    })
                    .collect();
                    
                (name, hash, files)
            })
            .map_err(|e| TorrentError::DownloadFailed(e.to_string()))?;

        // Convert info hash to hex string
        let info_hash = hex::encode(info_hash_bytes.0);
        
        info!("Torrent: {}", torrent_name);

        if files.is_empty() {
            return Err(TorrentError::NoVideoFiles);
        }

        // Select file to stream: the scraper's hint when it is a video,
        // otherwise the largest video
        let hinted = file_index
            .and_then(|idx| files.get(idx))
            .filter(|(_, n, _)| crate::is_video_file(n))
            .map(|(i, n, s)| (*i, n.clone(), *s));
        let (selected_index, filename, size) = match hinted {
            Some(f) => f,
            None => find_best_video_file(&files)
                .map(|(i, n, s)| (i, n.to_string(), s))
                .ok_or(TorrentError::NoVideoFiles)?,
        };

        info!("Streaming file: {} ({} bytes)", filename, size);

        // Only download the file we play (season packs, extras, samples)
        if files.len() > 1 {
            let only: HashSet<usize> = [selected_index].into_iter().collect();
            if let Err(e) = session.update_only_files(&handle, &only).await {
                debug!("update_only_files failed: {e:#}");
            }
        }

        // Generate stream ID
        let hash_prefix = &info_hash[..8.min(info_hash.len())];
        let stream_id = format!("{}_{}", hash_prefix, selected_index);

        // Determine file path intelligently
        // Check if we are likely single file mode or directory mode
        let candidate_path_1 = self.config.download_dir.join(&torrent_name).join(&filename);
        let candidate_path_2 = self.config.download_dir.join(&filename);
        
        let file_path = if candidate_path_1.exists() {
            candidate_path_1
        } else if candidate_path_2.exists() {
            candidate_path_2
        } else {
            // Fallback to the structure assuming folder if we are just starting and files aren't created yet
            // Start waiting for the file to exist? 
            // Most likely path 1 is correct for multi-file, path 2 for single-file.
            // If files.len() == 1, usually path 2.
            if files.len() == 1 {
                candidate_path_2
            } else {
                candidate_path_1
            }
        };

        info!("Resolved file path: {:?}", file_path);

        // Register with streaming server
        self.server.register_stream(
            stream_id.clone(),
            StreamableFile {
                info_hash: info_hash.clone(),
                file_index: selected_index,
                filename: filename.clone(),
                size,
                path: file_path,
                handle: Some(handle.clone()),
            },
        ).await;

        // Get torrent ID for handle
        let torrent_id = handle.id();

        // Store active torrent
        {
            let mut torrents = self.active_torrents.write().await;
            torrents.insert(stream_id.clone(), ActiveTorrent {
                handle,
                info_hash: info_hash.clone(),
                file_index: selected_index,
                filename: filename.clone(),
                size,
                stream_id: stream_id.clone(),
            });
        }

        // Create stream info
        let stream_url = self.server.stream_url(&stream_id, Some(&filename));
        
        let stream_info = StreamInfo {
            id: stream_id,
            info_hash,
            file_index: selected_index,
            filename,
            size,
            stream_url,
            progress: 0.0,
            download_speed: 0,
            upload_speed: 0,
            peers: 0,
            seeds: 0,
            status: StreamStatus::Buffering,
        };

        Ok(TorrentStream {
            info: stream_info,
            handle: TorrentHandle { id: torrent_id },
        })
    }

    /// Session torrent for this magnet's info hash, if any
    fn find_by_magnet(&self, magnet: &str) -> Option<Arc<ManagedTorrent>> {
        let session = self.session.as_ref()?;
        let hash = magnet_hash(magnet)?;
        session.with_torrents(|it| it.map(|(_, h)| h.clone()).find(|h| hex::encode(h.info_hash().0) == hash))
    }

    /// Fetch a torrent's metadata in the background without downloading any
    /// of it (the torrent stays paused), so pressing Play skips the slowest
    /// step. Earlier prefetches that were never played are dropped.
    pub async fn prefetch(&self, magnets: &[String]) {
        let Some(session) = self.session.clone() else { return };
        let wanted: HashSet<String> = magnets.iter().filter_map(|m| magnet_hash(m)).collect();
        // Drop stale prefetches (paused and not streaming)
        let active: HashSet<usize> = self.active_torrents.read().await.values().map(|a| a.handle.id()).collect();
        let stale: Vec<usize> = session.with_torrents(|it| {
            it.filter(|(id, h)| !active.contains(id) && h.is_paused() && !wanted.contains(&hex::encode(h.info_hash().0)))
                .map(|(id, _)| id)
                .collect()
        });
        for id in stale {
            let _ = session.delete(TorrentIdOrHash::Id(id), true).await;
        }
        for m in magnets {
            if self.find_by_magnet(m).is_some() {
                continue;
            }
            let session = session.clone();
            let m = with_default_trackers(m);
            tokio::spawn(async move {
                let opts = AddTorrentOptions { paused: true, overwrite: true, ..Default::default() };
                match tokio::time::timeout(Duration::from_secs(120), session.add_torrent(AddTorrent::from_url(&m), Some(opts))).await {
                    Ok(Ok(_)) => debug!("prefetched metadata for {}", &m[..m.len().min(60)]),
                    Ok(Err(e)) => debug!("prefetch failed: {e:#}"),
                    Err(_) => debug!("prefetch timed out"),
                }
            });
        }
    }

    /// Unregister every stream (torrents stay in the session)
    pub async fn stop_streams(&self) {
        let ids: Vec<String> = self.active_torrents.read().await.keys().cloned().collect();
        for id in ids {
            self.remove_stream(&id).await;
        }
    }

    /// Get updated stream info
    pub async fn get_stream_info(&self, stream_id: &str) -> TorrentResult<StreamInfo> {
        let torrents = self.active_torrents.read().await;
        
        let active = torrents.get(stream_id)
            .ok_or_else(|| TorrentError::TorrentNotFound(stream_id.to_string()))?;

        let stats = active.handle.stats();
        
        // Get file info from server
        let stream_url = self.server.stream_url(stream_id, Some(&active.filename));

        // Calculate progress
        let progress = if stats.total_bytes > 0 {
            stats.progress_bytes as f64 / stats.total_bytes as f64
        } else {
            0.0
        };
        
        let status = if progress >= 1.0 {
            StreamStatus::Complete
        } else if progress >= self.config.buffer_percent {
            StreamStatus::Streaming
        } else {
            StreamStatus::Buffering
        };

        // Extract speeds from live stats
        let (download_speed, upload_speed, peers) = stats.live
            .as_ref()
            .map(|l| {
                let dl = (l.download_speed.mbps * 125000.0) as u64; // Mbps to bytes/sec
                let ul = (l.upload_speed.mbps * 125000.0) as u64;
                // Peer count from snapshot
                let p = l.snapshot.peer_stats.queued + l.snapshot.peer_stats.live;
                (dl, ul, p as u32)
            })
            .unwrap_or((0, 0, 0));

        Ok(StreamInfo {
            id: stream_id.to_string(),
            info_hash: active.info_hash.clone(),
            file_index: active.file_index,
            filename: active.filename.clone(),
            size: active.size,
            stream_url,
            progress,
            download_speed,
            upload_speed,
            peers,
            seeds: peers, // librqbit doesn't distinguish seeds/peers clearly
            status,
        })
    }

    /// Read the first `head` and last `tail` bytes of the stream's file so
    /// the player starts on data that is already on disk (mkv cues / mp4
    /// `moov` often live at the end). Reading through librqbit's `FileStream`
    /// gives those pieces top priority; once mpv reads the file the same
    /// stream logic keeps a 32 MiB read-ahead window in front of its position
    /// and re-prioritises on every seek (HTTP Range request).
    pub async fn prebuffer(
        &self,
        stream_id: &str,
        head: u64,
        tail: u64,
        mut on_progress: impl FnMut(BufferProgress) + Send,
    ) -> TorrentResult<()> {
        let (handle, file_index, size) = {
            let torrents = self.active_torrents.read().await;
            let a = torrents.get(stream_id).ok_or_else(|| TorrentError::TorrentNotFound(stream_id.to_string()))?;
            (a.handle.clone(), a.file_index, a.size)
        };
        let head = head.min(size);
        let tail = tail.min(size.saturating_sub(head));
        let done = Arc::new(AtomicU64::new(0));

        let read_range = |start: u64, len: u64, count: bool| {
            let handle = handle.clone();
            let done = done.clone();
            async move {
                if len == 0 {
                    return Ok::<(), TorrentError>(());
                }
                let mut s = handle.stream(file_index).map_err(|e| TorrentError::Internal(e.to_string()))?;
                s.seek(SeekFrom::Start(start)).await.map_err(|e| TorrentError::Internal(e.to_string()))?;
                let mut buf = vec![0u8; 256 * 1024];
                let mut got = 0u64;
                while got < len {
                    let want = (len - got).min(buf.len() as u64) as usize;
                    let n = s.read(&mut buf[..want]).await.map_err(|e| TorrentError::Internal(e.to_string()))?;
                    if n == 0 {
                        break;
                    }
                    got += n as u64;
                    if count {
                        done.fetch_add(n as u64, Ordering::Relaxed);
                    }
                }
                Ok(())
            }
        };
        // The tail only needs to be prioritised, not waited for: mpv reads it
        // a few seconds into playback, by then it is usually there
        if tail > 0 {
            let tail_read = read_range(size - tail, tail, false);
            tokio::spawn(async move {
                let _ = tail_read.await;
            });
        }
        let target = head;
        let work = read_range(0, head, true);
        tokio::pin!(work);
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let mut n = 0u32;
        loop {
            tokio::select! {
                r = &mut work => {
                    on_progress(Self::progress_of(&handle, target, target));
                    return r;
                }
                _ = tick.tick() => {
                    let p = Self::progress_of(&handle, done.load(Ordering::Relaxed).min(target), target);
                    n += 1;
                    if n % 8 == 0 {
                        debug!("prebuffer {stream_id}: {}/{} B, {} B/s, {} peers ({} seen)", p.buffered, p.target, p.download_speed, p.peers, p.seen);
                    }
                    on_progress(p);
                }
            }
        }
    }

    fn progress_of(handle: &ManagedTorrent, buffered: u64, target: u64) -> BufferProgress {
        let stats = handle.stats();
        let (download_speed, peers, seen) = stats
            .live
            .as_ref()
            .map(|l| {
                let ps = &l.snapshot.peer_stats;
                ((l.download_speed.mbps * 125000.0) as u64, ps.live as u32, ps.seen as u32)
            })
            .unwrap_or((0, 0, 0));
        BufferProgress { buffered, target, download_speed, peers, seen }
    }

    /// Stop a stream, drop the torrent from the session and delete its data
    pub async fn remove_stream(&self, stream_id: &str) {
        let active = self.active_torrents.write().await.remove(stream_id);
        self.server.unregister_stream(stream_id).await;
        if let (Some(a), Some(session)) = (active, self.session.as_ref()) {
            if let Err(e) = session.delete(TorrentIdOrHash::Id(a.handle.id()), true).await {
                debug!("Could not delete torrent {stream_id}: {e:#}");
            }
            info!("Removed torrent stream {stream_id}");
        }
    }

    /// Remove every stream except `keep` (only one torrent plays at a time)
    pub async fn remove_all_except(&self, keep: Option<&str>) {
        let ids: Vec<String> = self.active_torrents.read().await.keys().filter(|k| Some(k.as_str()) != keep).cloned().collect();
        for id in ids {
            self.remove_stream(&id).await;
        }
        // Torrents added by a start-up attempt that was cancelled before it
        // registered a stream
        let Some(session) = self.session.as_ref() else { return };
        let kept: HashSet<usize> = self.active_torrents.read().await.values().map(|a| a.handle.id()).collect();
        let orphans: Vec<usize> = session.with_torrents(|it| it.map(|(id, _)| id).filter(|id| !kept.contains(id)).collect());
        for id in orphans {
            let _ = session.delete(TorrentIdOrHash::Id(id), true).await;
        }
    }

    /// Stop a stream and optionally remove the torrent
    pub async fn stop_stream(&self, stream_id: &str, _delete_files: bool) -> TorrentResult<()> {
        let mut torrents = self.active_torrents.write().await;
        
        if let Some(_active) = torrents.remove(stream_id) {
            // Unregister from server
            self.server.unregister_stream(stream_id).await;
            
            // The handle will be dropped, pausing the torrent
            info!("Stopped stream: {}", stream_id);
            
            // TODO: If delete_files, remove downloaded data
        }

        Ok(())
    }

    /// Get all active streams
    pub async fn list_streams(&self) -> Vec<String> {
        let torrents = self.active_torrents.read().await;
        torrents.keys().cloned().collect()
    }

    /// Get the streaming server URL
    pub fn server_url(&self) -> String {
        self.server.base_url()
    }

    /// Shutdown the engine
    pub async fn shutdown(&mut self) {
        info!("Shutting down torrent engine...");
        
        // Clear active torrents
        {
            let mut torrents = self.active_torrents.write().await;
            torrents.clear();
        }
        
        // Stop server
        self.server.stop();
        
        // Session will be dropped
        self.session = None;
        
        info!("Torrent engine shutdown complete");
    }
}

impl Drop for TorrentEngine {
    fn drop(&mut self) {
        self.server.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_trackers_are_appended_once() {
        let m = with_default_trackers("magnet:?xt=urn:btih:abc&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce");
        assert_eq!(m.matches("opentrackr").count(), 1);
        assert_eq!(m.matches("&tr=").count(), DEFAULT_TRACKERS.len());
        assert_eq!(with_default_trackers("http://x/y.mkv"), "http://x/y.mkv");
    }

    #[test]
    fn test_default_config() {
        let config = TorrentEngineConfig::default();
        assert!(config.download_dir.to_string_lossy().contains("torrents"));
        assert_eq!(config.server_port, 9999);
        assert!(config.enable_dht);
    }
}
