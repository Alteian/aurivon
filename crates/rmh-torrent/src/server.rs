//! HTTP streaming server for serving torrent files to MPV

use crate::error::{TorrentError, TorrentResult};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use librqbit::ManagedTorrent;
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tokio_util::io::ReaderStream;
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, info, warn};

/// Information about a streamable file
#[derive(Clone)]
pub struct StreamableFile {
    pub info_hash: String,
    pub file_index: usize,
    pub filename: String,
    pub size: u64,
    pub path: std::path::PathBuf,
    pub handle: Option<Arc<ManagedTorrent>>,
}

/// Shared state for the streaming server
#[derive(Default)]
pub struct StreamServerState {
    /// Active streams: stream_id -> file info
    pub streams: HashMap<String, StreamableFile>,
}

/// HTTP server for streaming torrent files
pub struct StreamServer {
    state: Arc<RwLock<StreamServerState>>,
    addr: SocketAddr,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl StreamServer {
    /// Create a new stream server on the specified port
    pub fn new(port: u16) -> Self {
        Self {
            state: Arc::new(RwLock::new(StreamServerState::default())),
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
            shutdown_tx: None,
        }
    }

    /// Get the base URL for streams
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Get the stream URL for a specific stream ID
    pub fn stream_url(&self, stream_id: &str, filename: Option<&str>) -> String {
        match filename {
            Some(name) => {
                 // Simple encoding for URL safety
                let safe_name = name.replace(" ", "%20")
                    .replace("?", "%3F")
                    .replace("#", "%23");
                format!("{}/stream/{}/{}", self.base_url(), stream_id, safe_name)
            },
            None => format!("{}/stream/{}", self.base_url(), stream_id),
        }
    }

    /// Register a file for streaming
    pub async fn register_stream(&self, stream_id: String, file: StreamableFile) {
        let mut state = self.state.write().await;
        info!("Registering stream: {} -> {}", stream_id, file.filename);
        state.streams.insert(stream_id, file);
    }

    /// Unregister a stream
    pub async fn unregister_stream(&self, stream_id: &str) {
        let mut state = self.state.write().await;
        state.streams.remove(stream_id);
        debug!("Unregistered stream: {}", stream_id);
    }

    /// Start the HTTP server
    pub async fn start(&mut self) -> TorrentResult<()> {
        let state = self.state.clone();
        let addr = self.addr;

        let app = Router::new()
            .route("/stream/:stream_id", get(handle_stream))
            .route("/stream/:stream_id/:filename", get(handle_stream_with_filename))
            .route("/health", get(|| async { "OK" }))
            .layer(CorsLayer::new().allow_origin(Any))
            .with_state(state);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        info!("Starting stream server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| TorrentError::ServerError(e.to_string()))?;

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        Ok(())
    }

    /// Stop the HTTP server
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            info!("Stream server stopped");
        }
    }
}

/// Handle stream requests
async fn handle_stream(
    State(state): State<Arc<RwLock<StreamServerState>>>,
    Path(stream_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    serve_stream(state, &stream_id, headers).await
}

/// Handle stream requests with filename (for better player compatibility)
async fn handle_stream_with_filename(
    State(state): State<Arc<RwLock<StreamServerState>>>,
    Path((stream_id, _filename)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    serve_stream(state, &stream_id, headers).await
}

/// Serve a stream with Range request support
async fn serve_stream(
    state: Arc<RwLock<StreamServerState>>,
    stream_id: &str,
    headers: HeaderMap,
) -> Response {
    let state = state.read().await;
    
    info!("Handling stream request for ID: {}", stream_id);

    let file_info = match state.streams.get(stream_id) {
        Some(f) => f.clone(),
        None => {
            warn!("Stream not found: {}", stream_id);
            warn!("Available streams: {:?}", state.streams.keys());
            return (StatusCode::NOT_FOUND, "Stream not found").into_response();
        }
    };
    
    drop(state); // Release lock before file operations

    let file_size = file_info.size;
    let content_type = guess_content_type(&file_info.filename);

    // Parse Range header for seeking support
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| parse_range(s, file_size));

    let (start, end) = range.unwrap_or((0, file_size - 1));
    let length = end - start + 1;

    let mut response_builder = Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, length)
        .header(header::ACCEPT_RANGES, "bytes");

    if range.is_some() {
        response_builder = response_builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, file_size));
    } else {
        response_builder = response_builder.status(StatusCode::OK);
    }

    // Try to stream from handle if available
    if let Some(handle) = &file_info.handle {
        info!("Streaming via librqbit handle: file_index={}, start={}, len={}", file_info.file_index, start, length);
        
        // Use librqbit's stream method
        match handle.clone().stream(file_info.file_index) {
            Ok(mut file_stream) => {
                use tokio::io::{AsyncSeekExt, SeekFrom};

                if start > 0 {
                    if let Err(e) = file_stream.seek(SeekFrom::Start(start)).await {
                         warn!("Seek failed on stream: {}", e);
                         return (StatusCode::INTERNAL_SERVER_ERROR, "Seek failed").into_response();
                    }
                }
                
                let stream = ReaderStream::new(file_stream.take(length));
                let body = Body::from_stream(stream);
                return response_builder.body(body).unwrap();
            }
            Err(e) => {
                warn!("Failed to create torrent stream: {:?}. Falling back to file.", e);
            }
        }
    }

    // Fallback to disk file
    info!("Stream file path: {:?}", file_info.path);
    if !file_info.path.exists() {
        warn!("FILE DOES NOT EXIST ON DISK: {:?}", file_info.path);
    }
    
    let file = match tokio::fs::File::open(&file_info.path).await {
        Ok(f) => f,
        Err(e) => {
            warn!("Failed to open file {:?}: {}", file_info.path, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file").into_response();
        }
    };

    let file = match seek_file(file, start).await {
        Ok(f) => f,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Seek failed").into_response(),
    };
    
    // Limit to requested length
    let stream = ReaderStream::new(file.take(length));
    let body = Body::from_stream(stream);

    response_builder.body(body).unwrap()
}

/// Seek a file to a position
async fn seek_file(file: tokio::fs::File, pos: u64) -> std::io::Result<tokio::fs::File> {
    use tokio::io::{AsyncSeekExt, SeekFrom};
    let mut file = file;
    file.seek(SeekFrom::Start(pos)).await?;
    Ok(file)
}

/// Parse HTTP Range header
fn parse_range(range: &str, file_size: u64) -> Option<(u64, u64)> {
    let range = range.strip_prefix("bytes=")?.trim();
    // Only the first range of a multi-range request is served
    let range = range.split(',').next()?.trim();
    let (a, b) = range.split_once('-')?;
    if file_size == 0 {
        return None;
    }
    let (start, end) = if a.is_empty() {
        // Suffix range: the last N bytes
        let n: u64 = b.parse().ok()?;
        if n == 0 {
            return None;
        }
        (file_size.saturating_sub(n), file_size - 1)
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() { file_size - 1 } else { b.parse::<u64>().ok()?.min(file_size - 1) };
        (start, end)
    };
    (start <= end && end < file_size).then_some((start, end))
}

#[cfg(test)]
mod range_tests {
    use super::parse_range;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=90-200", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-500", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=0-9, 20-29", 100), Some((0, 9)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
    }
}

/// Guess content type from filename
fn guess_content_type(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    
    if lower.ends_with(".mkv") {
        "video/x-matroska"
    } else if lower.ends_with(".mp4") || lower.ends_with(".m4v") {
        "video/mp4"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".avi") {
        "video/x-msvideo"
    } else if lower.ends_with(".mov") {
        "video/quicktime"
    } else if lower.ends_with(".ts") || lower.ends_with(".m2ts") {
        "video/mp2t"
    } else if lower.ends_with(".wmv") {
        "video/x-ms-wmv"
    } else if lower.ends_with(".flv") {
        "video/x-flv"
    } else {
        "application/octet-stream"
    }
}

impl Drop for StreamServer {
    fn drop(&mut self) {
        self.stop();
    }
}
