//! Torrent streaming engine using librqbit
//!
//! Provides Stremio-like sequential downloading for instant video playback.

use crate::error::{TorrentError, TorrentResult};
use crate::server::{StreamServer, StreamableFile};
use crate::stream::{StreamInfo, StreamStatus, TorrentHandle, TorrentStream};
use crate::find_best_video_file;
use librqbit::{AddTorrent, AddTorrentOptions, ManagedTorrent, Session, SessionOptions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

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

        // Parse and add the torrent
        let add_opts = AddTorrentOptions {
            initial_peers: None,
            overwrite: true,
            ..Default::default()
        };

        let response = session
            .add_torrent(
                AddTorrent::from_url(magnet),
                Some(add_opts),
            )
            .await
            .map_err(|e| TorrentError::Internal(e.to_string()))?;

        let handle = response
            .into_handle()
            .ok_or_else(|| TorrentError::DownloadFailed("Failed to get handle".into()))?;

        // Wait for metadata
        info!("Waiting for torrent metadata...");
        handle.wait_until_initialized()
            .await
            .map_err(|e| TorrentError::DownloadFailed(e.to_string()))?;

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

        // Select file to stream
        let (selected_index, filename, size) = if let Some(idx) = file_index {
            files.get(idx)
                .map(|(i, n, s)| (*i, n.clone(), *s))
                .ok_or_else(|| TorrentError::FileNotFound(format!("Index {} not found", idx)))?
        } else {
            find_best_video_file(&files)
                .map(|(i, n, s)| (i, n.to_string(), s))
                .ok_or(TorrentError::NoVideoFiles)?
        };

        info!("Streaming file: {} ({} bytes)", filename, size);

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
    fn test_default_config() {
        let config = TorrentEngineConfig::default();
        assert!(config.download_dir.to_string_lossy().contains("torrents"));
        assert_eq!(config.server_port, 9999);
        assert!(config.enable_dht);
    }
}
