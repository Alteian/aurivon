//! Disk usage reporting and pruning for every cache Aurivon keeps on disk.

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};

use rmh_database::{Database, RepairReport};
use tracing::info;

use crate::images::{prune, usage, CacheUsage};
use crate::service::{MediaService, ServiceResult};

/// One-shot repair for `aurivon --repair-db` (no UI / MediaService required).
pub fn repair_default_database() -> Result<String, String> {
    let path = Database::default_path().map_err(|e| format!("{e:#}"))?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("runtime: {e}"))?;
    let report = rt
        .block_on(Database::repair_at(&path))
        .map_err(|e| format!("{e:#}"))?;
    Ok(match report {
        RepairReport::Ok => format!("Library database is fine ({})", path.display()),
        RepairReport::ChecksumsUpdated { versions } => {
            format!(
                "Repaired migration checksums for versions {versions:?} ({})",
                path.display()
            )
        }
        RepairReport::Recreated { backup } => format!(
            "Database recreated (was unusable). Backup: {}. Re-add libraries and rescan.",
            backup.display()
        ),
    })
}

/// One on-disk cache bucket
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBucket {
    Images,
    Torrents,
    Tmdb,
    Calendar,
    Subtitles,
}

impl StorageBucket {
    pub const ALL: [StorageBucket; 5] = [
        StorageBucket::Images,
        StorageBucket::Torrents,
        StorageBucket::Tmdb,
        StorageBucket::Calendar,
        StorageBucket::Subtitles,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StorageBucket::Images => "Images",
            StorageBucket::Torrents => "Torrents",
            StorageBucket::Tmdb => "TMDB metadata",
            StorageBucket::Calendar => "Calendar",
            StorageBucket::Subtitles => "Subtitles",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageReport {
    pub images: CacheUsage,
    pub torrents: CacheUsage,
    pub tmdb: CacheUsage,
    pub calendar: CacheUsage,
    pub subtitles: CacheUsage,
}

impl StorageReport {
    pub fn total_bytes(&self) -> u64 {
        self.images.bytes + self.torrents.bytes + self.tmdb.bytes + self.calendar.bytes + self.subtitles.bytes
    }

    pub fn for_bucket(&self, b: StorageBucket) -> CacheUsage {
        match b {
            StorageBucket::Images => self.images,
            StorageBucket::Torrents => self.torrents,
            StorageBucket::Tmdb => self.tmdb,
            StorageBucket::Calendar => self.calendar,
            StorageBucket::Subtitles => self.subtitles,
        }
    }
}

fn dir_usage(dir: &Path) -> CacheUsage {
    if !dir.exists() {
        return CacheUsage::default();
    }
    usage(dir)
}

/// Delete everything under `dir` (files only; empty dirs left behind are fine)
pub fn clear_dir(dir: &Path) -> (u64, u64) {
    if !dir.exists() {
        return (0, 0);
    }
    prune(dir, 0, &HashSet::new())
}

/// Remove leftover torrent data that is not part of an active stream
pub fn cleanup_torrent_orphans(dir: &Path, keep: &HashSet<PathBuf>) -> (u64, u64) {
    if !dir.exists() {
        return (0, 0);
    }
    let mut removed = (0u64, 0u64);
    let Ok(rd) = std::fs::read_dir(dir) else {
        return removed;
    };
    for e in rd.flatten() {
        let path = e.path();
        if keep.iter().any(|k| k == &path || k.starts_with(&path) || path.starts_with(k)) {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            let (n, b) = clear_dir(&path);
            let _ = std::fs::remove_dir_all(&path);
            removed.0 += n;
            removed.1 += b;
        } else if std::fs::remove_file(&path).is_ok() {
            removed.0 += 1;
            removed.1 += md.len();
        }
    }
    removed
}

impl MediaService {
    pub fn cache_base(&self) -> PathBuf {
        self.inner.image_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| self.inner.data_dir.clone())
    }

    pub fn torrent_dir(&self) -> PathBuf {
        self.config()
            .torrent_dir
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| self.cache_base().join("torrents"))
    }

    pub fn calendar_dir(&self) -> PathBuf {
        self.cache_base().join("calendar")
    }

    pub fn storage_report(&self) -> StorageReport {
        StorageReport {
            images: dir_usage(&self.inner.image_dir),
            torrents: dir_usage(&self.torrent_dir()),
            tmdb: dir_usage(&self.inner.tmdb_cache),
            calendar: dir_usage(&self.calendar_dir()),
            subtitles: dir_usage(&self.inner.subs_dir),
        }
    }

    /// Cap every bucket to the configured limits. Returns total (files, bytes) removed.
    pub fn prune_all_caches(&self) -> impl Future<Output = ServiceResult<(u64, u64)>> + 'static {
        let cfg = self.config();
        let image_dir = self.inner.image_dir.clone();
        let torrent_dir = self.torrent_dir();
        let tmdb = self.inner.tmdb_cache.clone();
        let calendar = self.calendar_dir();
        let subs = self.inner.subs_dir.clone();
        let image_cap = cfg.image_cache_mb as u64 * 1_000_000;
        let torrent_cap = cfg.torrent_cache_gb as u64 * 1_000_000_000;
        let tmdb_cap = cfg.tmdb_cache_mb as u64 * 1_000_000;
        let calendar_cap = cfg.calendar_cache_mb as u64 * 1_000_000;
        let subs_cap = cfg.subs_cache_mb as u64 * 1_000_000;
        let entries = self.library_entries();
        let torrent = self.inner.torrent.clone();
        self.run(async move {
            let keep: HashSet<PathBuf> = entries
                .await
                .unwrap_or_default()
                .iter()
                .filter_map(|e| e.link.as_ref()?.poster_path.as_ref().map(PathBuf::from))
                .collect();
            let mut total = (0u64, 0u64);
            let (n, b) = tokio::task::spawn_blocking({
                let d = image_dir.clone();
                let k = keep.clone();
                move || prune(&d, image_cap, &k)
            })
            .await
            .map_err(|e| e.to_string())?;
            total.0 += n;
            total.1 += b;

            // Active torrent paths stay; everything else is orphan data
            let active_paths: HashSet<PathBuf> = {
                let engine = torrent.lock().await.clone();
                match engine {
                    Some(e) => e.active_paths().await.into_iter().collect(),
                    None => HashSet::new(),
                }
            };
            let (n, b) = tokio::task::spawn_blocking({
                let d = torrent_dir.clone();
                let keep = active_paths.clone();
                move || {
                    let orphans = cleanup_torrent_orphans(&d, &keep);
                    let pruned = prune(&d, torrent_cap, &keep);
                    (orphans.0 + pruned.0, orphans.1 + pruned.1)
                }
            })
            .await
            .map_err(|e| e.to_string())?;
            total.0 += n;
            total.1 += b;

            for (dir, cap) in [
                (tmdb, tmdb_cap),
                (calendar, calendar_cap),
                (subs, subs_cap),
            ] {
                let empty = HashSet::new();
                let (n, b) = tokio::task::spawn_blocking(move || prune(&dir, cap, &empty))
                    .await
                    .map_err(|e| e.to_string())?;
                total.0 += n;
                total.1 += b;
            }
            if total.0 > 0 {
                info!("Storage prune: removed {} files ({:.1} MB)", total.0, total.1 as f64 / 1e6);
            }
            Ok(total)
        })
    }

    pub fn clear_storage_bucket(&self, bucket: StorageBucket) -> impl Future<Output = ServiceResult<(u64, u64)>> + 'static {
        let path = match bucket {
            StorageBucket::Images => self.inner.image_dir.clone(),
            StorageBucket::Torrents => self.torrent_dir(),
            StorageBucket::Tmdb => self.inner.tmdb_cache.clone(),
            StorageBucket::Calendar => self.calendar_dir(),
            StorageBucket::Subtitles => self.inner.subs_dir.clone(),
        };
        let torrent = self.inner.torrent.clone();
        self.run(async move {
            if bucket == StorageBucket::Torrents {
                if let Some(e) = torrent.lock().await.clone() {
                    e.remove_all_except(None).await;
                }
            }
            let (n, b) = tokio::task::spawn_blocking(move || clear_dir(&path))
                .await
                .map_err(|e| e.to_string())?;
            info!("Cleared {}: {} files ({:.1} MB)", bucket.label(), n, b as f64 / 1e6);
            Ok((n, b))
        })
    }

    /// True when torrent cache is at or over the configured cap
    pub fn torrent_disk_pressure(&self) -> bool {
        let cap = self.config().torrent_cache_gb as u64 * 1_000_000_000;
        if cap == 0 {
            return false;
        }
        dir_usage(&self.torrent_dir()).bytes >= cap.saturating_mul(9) / 10
    }

    /// Fix migration checksum drift from older releases; if the file is still
    /// unusable, move it aside and create a fresh library database.
    pub fn repair_database(&self) -> impl Future<Output = ServiceResult<String>> + 'static {
        let path = self.inner.db_path.clone();
        let existing = self.inner.db.get().cloned();
        self.run(async move {
            if let Some(db) = existing {
                let versions = db.repair_checksums().await.map_err(|e| format!("{e:#}"))?;
                return Ok(if versions.is_empty() {
                    "Library database is fine.".into()
                } else {
                    format!("Repaired migration checksums for versions {versions:?}.")
                });
            }
            match Database::repair_at(&path).await.map_err(|e| format!("{e:#}"))? {
                RepairReport::Ok => Ok("Library database is fine.".into()),
                RepairReport::ChecksumsUpdated { versions } => {
                    Ok(format!("Repaired migration checksums for versions {versions:?}."))
                }
                RepairReport::Recreated { backup } => Ok(format!(
                    "Database recreated (was unusable). Backup: {}. Re-add libraries and rescan.",
                    backup.display()
                )),
            }
        })
    }
}
