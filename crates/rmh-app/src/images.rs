//! Images: downloaded on demand (only what is on screen), a few at a time, into
//! a size-capped disk cache that evicts the least recently used files first.

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use rmh_metadata::{cache_image, cached_image_path, PosterSize, Title};
use tokio::sync::Semaphore;
use tracing::info;

use crate::service::{MediaService, ServiceResult};

/// Parallel image downloads (TMDB's CDN is fast; this keeps bursts polite)
const PARALLEL_DOWNLOADS: usize = 6;
static PERMITS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(PARALLEL_DOWNLOADS));

/// Cached file for `url`, downloading it (rate limited) when missing
pub(crate) async fn fetch_image(http: &reqwest::Client, url: &str, dir: &Path) -> anyhow::Result<PathBuf> {
    if cached_image_path(url, dir).exists() {
        return cache_image(http, url, dir).await;
    }
    let _permit = PERMITS.acquire().await?;
    cache_image(http, url, dir).await
}

/// Poster URL used for cards and the details page
pub fn poster_url(t: &Title) -> Option<String> {
    t.poster_url(PosterSize::W342)
}

/// Poster for Home rows (cards ~150 px wide)
pub fn row_poster_url(t: &Title) -> Option<String> {
    t.poster_url(PosterSize::W185)
}

/// Backdrop for landscape row cards (W300, W780 for large cards)
pub fn row_backdrop_url(t: &Title, large: bool) -> Option<String> {
    let size = if large { rmh_metadata::BackdropSize::W780 } else { rmh_metadata::BackdropSize::W300 };
    t.backdrop_path.as_deref().map(|b| rmh_metadata::TmdbClient::backdrop_url(b, size))
}

/// Backdrop for the Home hero carousel
pub fn hero_backdrop_url(t: &Title) -> Option<String> {
    t.backdrop_path.as_deref().map(|b| rmh_metadata::TmdbClient::backdrop_url(b, rmh_metadata::BackdropSize::W1280))
}

/// Small poster for list rows (calendar, credits, search list)
pub fn thumb_url(t: &Title) -> Option<String> {
    t.poster_url(PosterSize::W154)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheUsage {
    pub files: u64,
    pub bytes: u64,
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64, std::time::SystemTime)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            walk(&e.path(), out);
        } else {
            out.push((e.path(), md.len(), md.modified().unwrap_or(std::time::UNIX_EPOCH)));
        }
    }
}

pub fn usage(dir: &Path) -> CacheUsage {
    let mut v = vec![];
    walk(dir, &mut v);
    CacheUsage { files: v.len() as u64, bytes: v.iter().map(|f| f.1).sum() }
}

/// Delete least recently used files until `dir` is at most `max_bytes`;
/// never touches `keep`. Returns (files removed, bytes removed).
pub fn prune(dir: &Path, max_bytes: u64, keep: &HashSet<PathBuf>) -> (u64, u64) {
    let mut v = vec![];
    walk(dir, &mut v);
    let mut total: u64 = v.iter().map(|f| f.1).sum();
    if total <= max_bytes {
        return (0, 0);
    }
    v.sort_by_key(|f| f.2);
    let (mut n, mut freed) = (0, 0);
    for (path, len, _) in v {
        if total <= max_bytes {
            break;
        }
        if keep.contains(&path) {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
            freed += len;
            n += 1;
        }
    }
    (n, freed)
}

impl MediaService {
    /// Local copy of an image URL, downloading it if needed
    pub fn image(&self, url: String) -> impl Future<Output = ServiceResult<PathBuf>> + 'static {
        let http = self.inner.http.clone();
        let dir = self.inner.image_dir.clone();
        self.run(async move { fetch_image(&http, &url, &dir).await.map_err(|e| format!("{e:#}")) })
    }

    /// Already-downloaded copy of an image URL (no network)
    pub fn cached_image(&self, url: &str) -> Option<PathBuf> {
        let p = cached_image_path(url, &self.inner.image_dir);
        p.exists().then_some(p)
    }

    pub fn image_cache_usage(&self) -> CacheUsage {
        usage(&self.inner.image_dir)
    }

    /// Trim the image cache to the configured cap (`max_mb` overrides it;
    /// 0 clears everything except library posters).
    pub fn prune_image_cache(&self, max_mb: Option<u32>) -> impl Future<Output = ServiceResult<(u64, u64)>> + 'static {
        let cap = max_mb.unwrap_or(self.config().image_cache_mb) as u64 * 1_000_000;
        let dir = self.inner.image_dir.clone();
        let entries = self.library_entries();
        self.run(async move {
            // posters of library items are referenced by path from the database
            let keep: HashSet<PathBuf> = entries
                .await
                .unwrap_or_default()
                .iter()
                .filter_map(|e| e.link.as_ref()?.poster_path.as_ref().map(PathBuf::from))
                .collect();
            let (n, bytes) = tokio::task::spawn_blocking(move || prune(&dir, cap, &keep)).await.map_err(|e| e.to_string())?;
            if n > 0 {
                info!("Image cache: removed {n} files ({:.1} MB)", bytes as f64 / 1e6);
            }
            Ok((n, bytes))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_evicts_oldest_and_keeps_protected() {
        let d = tempfile::tempdir().unwrap();
        let mk = |name: &str, age_s: u64| {
            let p = d.path().join(name);
            std::fs::write(&p, vec![0u8; 1000]).unwrap();
            let f = std::fs::File::options().append(true).open(&p).unwrap();
            f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(age_s)).unwrap();
            p
        };
        let oldest = mk("a.jpg", 400);
        let lib = mk("lib.jpg", 300);
        let mid = mk("b.jpg", 200);
        let new = mk("c.jpg", 100);
        let keep: HashSet<PathBuf> = [lib.clone()].into_iter().collect();
        assert_eq!(usage(d.path()).bytes, 4000);
        let (n, freed) = prune(d.path(), 2500, &keep);
        assert_eq!((n, freed), (2, 2000));
        assert!(!oldest.exists() && !mid.exists());
        assert!(lib.exists() && new.exists());
        assert_eq!(prune(d.path(), 2500, &keep), (0, 0));
    }
}
