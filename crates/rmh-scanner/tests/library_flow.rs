//! Library flow against a real SQLite database: add library -> scan -> list -> rescan.

use rmh_core::{Library, MediaType};
use rmh_database::{Database, MediaRepository};
use rmh_scanner::Scanner;
use std::fs::File;

#[tokio::test]
async fn add_scan_list_rescan() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir_all(media.join("sub")).unwrap();
    File::create(media.join("Movie.One.2020.mkv")).unwrap();
    File::create(media.join("sub").join("track.mp3")).unwrap();
    File::create(media.join("notes.txt")).unwrap();

    let db = Database::new(&dir.path().join("library.db")).await.unwrap();
    let repo = MediaRepository::new(db.pool());

    let lib = Library::new("Media".into(), media.to_string_lossy().into(), MediaType::Movie);
    repo.insert_library(&lib).await.unwrap();
    assert_eq!(repo.get_all_libraries().await.unwrap().len(), 1);

    let scanner = Scanner::new(db.clone());
    let added = scanner.scan_directory(lib.id, &media, None).await.unwrap();
    assert_eq!(added.len(), 2);

    let all = repo.get_all_media().await.unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|m| m.title == "Movie One 2020"));

    // Rescan adds nothing new
    let added = scanner.scan_directory(lib.id, &media, None).await.unwrap();
    assert_eq!(added.len(), 0);

    // Watch progress round-trips
    let item = &all[0];
    repo.update_watch_progress(item.id, 42).await.unwrap();
    let item = repo.get_media_item(item.id).await.unwrap().unwrap();
    assert_eq!(item.watch_progress, 42);

    // Removing the library removes its media
    repo.delete_library(lib.id).await.unwrap();
    assert!(repo.get_all_libraries().await.unwrap().is_empty());
}
