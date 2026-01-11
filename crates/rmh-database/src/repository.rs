//! Repository pattern for database operations

use anyhow::{Context, Result};
use chrono::Utc;
use rmh_core::{Library, MediaItem, MediaType};
use sqlx::SqlitePool;
use tracing::{debug, instrument};
use uuid::Uuid;

/// Repository for media-related database operations
pub struct MediaRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> MediaRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    // ==================== LIBRARY OPERATIONS ====================

    /// Insert a new library
    #[instrument(skip(self))]
    pub async fn insert_library(&self, library: &Library) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO libraries (id, name, path, media_type, last_scanned, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(library.id.to_string())
        .bind(&library.name)
        .bind(&library.path)
        .bind(library.media_type.to_string())
        .bind(library.last_scanned.map(|dt| dt.to_rfc3339()))
        .bind(library.created_at.to_rfc3339())
        .execute(self.pool)
        .await
        .context("Failed to insert library")?;

        debug!("Inserted library: {}", library.name);
        Ok(())
    }

    /// Get all libraries
    #[instrument(skip(self))]
    pub async fn get_all_libraries(&self) -> Result<Vec<Library>> {
        let rows = sqlx::query_as::<_, LibraryRow>("SELECT * FROM libraries ORDER BY name")
            .fetch_all(self.pool)
            .await
            .context("Failed to fetch libraries")?;

        Ok(rows.into_iter().map(Library::from).collect())
    }

    /// Get library by ID
    #[instrument(skip(self))]
    pub async fn get_library(&self, id: Uuid) -> Result<Option<Library>> {
        let row = sqlx::query_as::<_, LibraryRow>("SELECT * FROM libraries WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.pool)
            .await
            .context("Failed to fetch library")?;

        Ok(row.map(Library::from))
    }

    /// Delete a library and all its media items
    #[instrument(skip(self))]
    pub async fn delete_library(&self, id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM libraries WHERE id = ?")
            .bind(id.to_string())
            .execute(self.pool)
            .await
            .context("Failed to delete library")?;

        Ok(())
    }

    /// Update library last_scanned timestamp
    #[instrument(skip(self))]
    pub async fn update_library_scanned(&self, id: Uuid) -> Result<()> {
        sqlx::query("UPDATE libraries SET last_scanned = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(id.to_string())
            .execute(self.pool)
            .await
            .context("Failed to update library scan time")?;

        Ok(())
    }

    // ==================== MEDIA ITEM OPERATIONS ====================

    /// Insert a new media item
    #[instrument(skip(self, item), fields(title = %item.title))]
    pub async fn insert_media_item(&self, library_id: Uuid, item: &MediaItem) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO media_items (
                id, library_id, title, file_path, file_name, file_size, media_type,
                poster_url, backdrop_url, overview, release_date, rating, runtime_minutes,
                watched, watch_progress, tmdb_id, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(item.id.to_string())
        .bind(library_id.to_string())
        .bind(&item.title)
        .bind(&item.file_path)
        .bind(&item.file_name)
        .bind(item.file_size)
        .bind(item.media_type.to_string())
        .bind(&item.poster_url)
        .bind(&item.backdrop_url)
        .bind(&item.overview)
        .bind(&item.release_date)
        .bind(item.rating)
        .bind(item.runtime_minutes)
        .bind(item.watched as i32)
        .bind(item.watch_progress)
        .bind(item.tmdb_id)
        .bind(item.created_at.to_rfc3339())
        .bind(item.updated_at.to_rfc3339())
        .execute(self.pool)
        .await
        .context("Failed to insert media item")?;

        debug!("Inserted media item: {}", item.title);
        Ok(())
    }

    /// Insert or update a media item (upsert by file_path)
    #[instrument(skip(self, item), fields(title = %item.title))]
    pub async fn upsert_media_item(&self, library_id: Uuid, item: &MediaItem) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO media_items (
                id, library_id, title, file_path, file_name, file_size, media_type,
                poster_url, backdrop_url, overview, release_date, rating, runtime_minutes,
                watched, watch_progress, tmdb_id, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(file_path) DO UPDATE SET
                title = excluded.title,
                file_size = excluded.file_size,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(item.id.to_string())
        .bind(library_id.to_string())
        .bind(&item.title)
        .bind(&item.file_path)
        .bind(&item.file_name)
        .bind(item.file_size)
        .bind(item.media_type.to_string())
        .bind(&item.poster_url)
        .bind(&item.backdrop_url)
        .bind(&item.overview)
        .bind(&item.release_date)
        .bind(item.rating)
        .bind(item.runtime_minutes)
        .bind(item.watched as i32)
        .bind(item.watch_progress)
        .bind(item.tmdb_id)
        .bind(item.created_at.to_rfc3339())
        .bind(item.updated_at.to_rfc3339())
        .execute(self.pool)
        .await
        .context("Failed to upsert media item")?;

        Ok(())
    }

    /// Get all media items for a library
    #[instrument(skip(self))]
    pub async fn get_library_media(&self, library_id: Uuid) -> Result<Vec<MediaItem>> {
        let rows = sqlx::query_as::<_, MediaItemRow>(
            "SELECT * FROM media_items WHERE library_id = ? ORDER BY title",
        )
        .bind(library_id.to_string())
        .fetch_all(self.pool)
        .await
        .context("Failed to fetch media items")?;

        Ok(rows.into_iter().map(MediaItem::from).collect())
    }

    /// Get all media items
    #[instrument(skip(self))]
    pub async fn get_all_media(&self) -> Result<Vec<MediaItem>> {
        let rows =
            sqlx::query_as::<_, MediaItemRow>("SELECT * FROM media_items ORDER BY title")
                .fetch_all(self.pool)
                .await
                .context("Failed to fetch all media items")?;

        Ok(rows.into_iter().map(MediaItem::from).collect())
    }

    /// Get a single media item by ID
    #[instrument(skip(self))]
    pub async fn get_media_item(&self, id: Uuid) -> Result<Option<MediaItem>> {
        let row =
            sqlx::query_as::<_, MediaItemRow>("SELECT * FROM media_items WHERE id = ?")
                .bind(id.to_string())
                .fetch_optional(self.pool)
                .await
                .context("Failed to fetch media item")?;

        Ok(row.map(MediaItem::from))
    }

    /// Update metadata for a media item
    #[instrument(skip(self))]
    pub async fn update_media_metadata(
        &self,
        id: Uuid,
        title: &str,
        poster_url: Option<&str>,
        backdrop_url: Option<&str>,
        overview: Option<&str>,
        release_date: Option<&str>,
        rating: Option<f32>,
        runtime_minutes: Option<i32>,
        tmdb_id: Option<i32>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE media_items SET
                title = ?, poster_url = ?, backdrop_url = ?, overview = ?,
                release_date = ?, rating = ?, runtime_minutes = ?, tmdb_id = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(title)
        .bind(poster_url)
        .bind(backdrop_url)
        .bind(overview)
        .bind(release_date)
        .bind(rating)
        .bind(runtime_minutes)
        .bind(tmdb_id)
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(self.pool)
        .await
        .context("Failed to update media metadata")?;

        Ok(())
    }

    /// Update watched status
    #[instrument(skip(self))]
    pub async fn update_watched_status(&self, id: Uuid, watched: bool) -> Result<()> {
        sqlx::query("UPDATE media_items SET watched = ?, updated_at = ? WHERE id = ?")
            .bind(watched as i32)
            .bind(Utc::now().to_rfc3339())
            .bind(id.to_string())
            .execute(self.pool)
            .await
            .context("Failed to update watched status")?;

        Ok(())
    }

    /// Update watch progress
    #[instrument(skip(self))]
    pub async fn update_watch_progress(&self, id: Uuid, progress_seconds: i64) -> Result<()> {
        sqlx::query("UPDATE media_items SET watch_progress = ?, updated_at = ? WHERE id = ?")
            .bind(progress_seconds)
            .bind(Utc::now().to_rfc3339())
            .bind(id.to_string())
            .execute(self.pool)
            .await
            .context("Failed to update watch progress")?;

        Ok(())
    }

    /// Search media items by title
    #[instrument(skip(self))]
    pub async fn search_media(&self, query: &str) -> Result<Vec<MediaItem>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, MediaItemRow>(
            "SELECT * FROM media_items WHERE title LIKE ? ORDER BY title LIMIT 50",
        )
        .bind(pattern)
        .fetch_all(self.pool)
        .await
        .context("Failed to search media items")?;

        Ok(rows.into_iter().map(MediaItem::from).collect())
    }

    /// Check if a file path already exists in the database
    #[instrument(skip(self))]
    pub async fn file_exists(&self, file_path: &str) -> Result<bool> {
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM media_items WHERE file_path = ?")
                .bind(file_path)
                .fetch_one(self.pool)
                .await
                .context("Failed to check file existence")?;

        Ok(count.0 > 0)
    }

    /// Get count of media items
    #[instrument(skip(self))]
    pub async fn get_media_count(&self) -> Result<i64> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM media_items")
            .fetch_one(self.pool)
            .await
            .context("Failed to count media items")?;

        Ok(count.0)
    }
}

// ==================== ROW TYPES FOR SQLX ====================

#[derive(Debug, sqlx::FromRow)]
struct LibraryRow {
    id: String,
    name: String,
    path: String,
    media_type: String,
    last_scanned: Option<String>,
    created_at: String,
}

impl From<LibraryRow> for Library {
    fn from(row: LibraryRow) -> Self {
        Library {
            id: Uuid::parse_str(&row.id).unwrap_or_else(|_| Uuid::new_v4()),
            name: row.name,
            path: row.path,
            media_type: MediaType::from(row.media_type),
            last_scanned: row
                .last_scanned
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct MediaItemRow {
    id: String,
    #[allow(dead_code)]
    library_id: String,
    title: String,
    file_path: String,
    file_name: String,
    file_size: i64,
    media_type: String,
    poster_url: Option<String>,
    backdrop_url: Option<String>,
    overview: Option<String>,
    release_date: Option<String>,
    rating: Option<f64>,
    runtime_minutes: Option<i32>,
    watched: i32,
    watch_progress: i64,
    tmdb_id: Option<i32>,
    created_at: String,
    updated_at: String,
}

impl From<MediaItemRow> for MediaItem {
    fn from(row: MediaItemRow) -> Self {
        MediaItem {
            id: Uuid::parse_str(&row.id).unwrap_or_else(|_| Uuid::new_v4()),
            title: row.title,
            file_path: row.file_path,
            file_name: row.file_name,
            file_size: row.file_size,
            media_type: MediaType::from(row.media_type),
            poster_url: row.poster_url,
            backdrop_url: row.backdrop_url,
            overview: row.overview,
            release_date: row.release_date,
            rating: row.rating.map(|r| r as f32),
            runtime_minutes: row.runtime_minutes,
            watched: row.watched != 0,
            watch_progress: row.watch_progress,
            tmdb_id: row.tmdb_id,
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            updated_at: chrono::DateTime::parse_from_rfc3339(&row.updated_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
        }
    }
}
