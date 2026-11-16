//! File system scanner for video and audio files

use anyhow::{Context, Result};
use rmh_core::{is_media_file, MediaItem, ScanProgress};
use rmh_database::{Database, MediaRepository};
use std::path::Path;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;
use walkdir::WalkDir;

/// Scanner for finding video files in directories
pub struct Scanner {
    database: Database,
}

impl Scanner {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// Scan a directory recursively for video files
    /// Returns a channel receiver for progress updates
    pub async fn scan_directory(
        &self,
        library_id: Uuid,
        path: &Path,
        progress_tx: Option<mpsc::Sender<ScanProgress>>,
    ) -> Result<Vec<MediaItem>> {
        info!("Starting scan of directory: {}", path.display());

        if !path.exists() {
            anyhow::bail!("Directory does not exist: {}", path.display());
        }

        if !path.is_dir() {
            anyhow::bail!("Path is not a directory: {}", path.display());
        }

        // First, collect all video files
        let video_files = self.collect_video_files(path)?;
        let total_files = video_files.len();

        info!("Found {} video files to process", total_files);

        let mut items = Vec::with_capacity(total_files);
        let repo = MediaRepository::new(self.database.pool());

        for (index, (file_path, file_name, file_size)) in video_files.into_iter().enumerate() {
            // Send progress update
            if let Some(ref tx) = progress_tx {
                let progress = ScanProgress {
                    total_files,
                    scanned_files: index + 1,
                    current_file: file_name.clone(),
                    is_complete: false,
                };
                let _ = tx.send(progress).await;
            }

            // Check if file already exists in database
            if repo.file_exists(&file_path).await? {
                debug!("File already in database, skipping: {}", file_path);
                continue;
            }

            // Create media item
            let item = MediaItem::from_scan(file_path, file_name, file_size);

            // Insert into database
            if let Err(e) = repo.upsert_media_item(library_id, &item).await {
                warn!("Failed to insert media item: {}", e);
                continue;
            }

            items.push(item);
        }

        // Send completion progress
        if let Some(tx) = progress_tx {
            let progress = ScanProgress {
                total_files,
                scanned_files: total_files,
                current_file: String::new(),
                is_complete: true,
            };
            let _ = tx.send(progress).await;
        }

        // Update library scan timestamp
        repo.update_library_scanned(library_id).await?;

        info!("Scan complete. Added {} new items", items.len());
        Ok(items)
    }

    /// Collect all video files from a directory
    fn collect_video_files(&self, path: &Path) -> Result<Vec<(String, String, i64)>> {
        let mut files = Vec::new();

        for entry in WalkDir::new(path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // Check extension
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            if !is_media_file(extension) {
                continue;
            }

            // Get file metadata
            let metadata = std::fs::metadata(path)
                .with_context(|| format!("Failed to read metadata for: {}", path.display()))?;

            let file_path = path.to_string_lossy().to_string();
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            let file_size = metadata.len() as i64;

            files.push((file_path, file_name, file_size));
        }

        // Sort by filename for consistent ordering
        files.sort_by(|a, b| a.1.cmp(&b.1));

        Ok(files)
    }
}

/// Standalone function to scan for video files (no database)
pub fn scan_for_videos(path: &Path) -> Result<Vec<(String, String, i64)>> {
    let mut files = Vec::new();

    if !path.exists() {
        anyhow::bail!("Path does not exist: {}", path.display());
    }

    for entry in WalkDir::new(path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let entry_path = entry.path();

        if entry_path.is_dir() {
            continue;
        }

        let extension = entry_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        if !is_media_file(extension) {
            continue;
        }

        if let Ok(metadata) = std::fs::metadata(entry_path) {
            files.push((
                entry_path.to_string_lossy().to_string(),
                entry_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown")
                    .to_string(),
                metadata.len() as i64,
            ));
        }
    }

    files.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_scan_for_videos() {
        let dir = tempdir().unwrap();
        
        // Create test video files
        File::create(dir.path().join("movie.mkv")).unwrap();
        File::create(dir.path().join("video.mp4")).unwrap();
        File::create(dir.path().join("song.flac")).unwrap();
        File::create(dir.path().join("document.pdf")).unwrap();

        let files = scan_for_videos(dir.path()).unwrap();
        
        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|(_, name, _)| name == "movie.mkv"));
        assert!(files.iter().any(|(_, name, _)| name == "video.mp4"));
    }
}

