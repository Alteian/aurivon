use rmh_app::{AppConfig, MediaService, ProgressUpdate};
use std::fs::File;

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    // Any executor works; use a throwaway current-thread runtime
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
}

#[test]
fn library_lifecycle_through_service() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("Movies");
    std::fs::create_dir_all(&media).unwrap();
    File::create(media.join("Some.Film.2021.mkv")).unwrap();
    File::create(media.join("song.flac")).unwrap();

    let cfg = AppConfig { tmdb_api_key: None, real_debrid_api_key: None, trakt_client_id: None, trakt_client_secret: None, ..AppConfig::default() };
    let svc = MediaService::with_paths(dir.path().join("lib.db"), cfg).unwrap();

    assert!(block_on(svc.add_library("/definitely/missing")).is_err());

    let (lib, added) = block_on(svc.add_library(media.to_string_lossy())).unwrap();
    assert_eq!(added, 2);
    assert_eq!(lib.name, "Movies");
    assert!(block_on(svc.add_library(media.to_string_lossy())).unwrap_err().contains("already"));

    let items = block_on(svc.media()).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(block_on(svc.scan_library(&lib)).unwrap(), 0);

    let id = items.iter().find(|m| m.title == "Some Film 2021").unwrap().id;
    block_on(svc.save_progress(ProgressUpdate { media_id: id, position: 77, watched: false })).unwrap();
    let items = block_on(svc.media()).unwrap();
    assert_eq!(items.iter().find(|m| m.id == id).unwrap().watch_progress, 77);

    block_on(svc.remove_library(lib.id)).unwrap();
    assert!(block_on(svc.libraries()).unwrap().is_empty());
    assert!(block_on(svc.media()).unwrap().is_empty());

    // Discover without a TMDB key gives a friendly error, not a panic
    assert!(block_on(svc.search_movies("matrix")).unwrap_err().contains("TMDB"));
}

/// Seek-bar previews: one cached frame per 5 s bucket (skipped without ffmpeg)
#[test]
fn seek_thumbnails_are_cached_per_bucket() {
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("clip.mkv");
    let made = std::process::Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=10:duration=12"])
        .arg(&clip)
        .status();
    if !matches!(made, Ok(s) if s.success()) {
        eprintln!("ffmpeg not available, skipping");
        return;
    }
    let cfg = AppConfig { tmdb_api_key: None, real_debrid_api_key: None, trakt_client_id: None, trakt_client_secret: None, ..AppConfig::default() };
    let svc = MediaService::with_paths(dir.path().join("lib.db"), cfg).unwrap();
    let src = clip.to_string_lossy().to_string();
    let a = block_on(svc.thumbnail(src.clone(), 6.2)).unwrap();
    let b = block_on(svc.thumbnail(src.clone(), 9.9)).unwrap();
    let c = block_on(svc.thumbnail(src, 1.0)).unwrap();
    assert!(a.exists() && std::fs::metadata(&a).unwrap().len() > 0);
    assert_eq!(a, b, "same 5 s bucket reuses the frame");
    assert_ne!(a, c);
}
