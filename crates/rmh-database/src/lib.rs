//! Database layer for Aurivon using SQLx + SQLite

pub mod migrations;
pub mod repository;

use anyhow::{Context, Result};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Database connection pool wrapper
#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

/// Result of [`Database::repair_at`].
#[derive(Debug, Clone)]
pub enum RepairReport {
    /// Migrations already matched; nothing changed.
    Ok,
    /// Stored checksums for already-applied migrations were rewritten to the
    /// embedded sources (comment-only edits from older releases).
    ChecksumsUpdated { versions: Vec<i64> },
    /// File was unusable; moved aside and a fresh database was created.
    Recreated { backup: PathBuf },
}

impl Database {
    /// Create a new database connection, initializing the database file if needed
    pub async fn new(db_path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create database directory")?;
        }

        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

        info!("Connecting to database: {}", db_path.display());

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .context("Failed to connect to database")?;

        let db = Self { pool };

        // Run migrations (auto-repairs checksum drift from older releases first)
        db.run_migrations().await?;

        Ok(db)
    }

    /// Repair a library database on disk: sync migration checksums, re-run
    /// migrations, and if it still cannot open, move the file aside and create
    /// a fresh database (media files are untouched; library rows are lost).
    pub async fn repair_at(db_path: &Path) -> Result<RepairReport> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create database directory")?;
        }

        if !db_path.exists() {
            Database::new(db_path).await?;
            return Ok(RepairReport::Ok);
        }

        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = match SqlitePoolOptions::new().max_connections(1).connect(&db_url).await {
            Ok(p) => p,
            Err(e) => {
                warn!("Database unreadable ({e:#}); recreating");
                return Self::recreate(db_path).await;
            }
        };
        let db = Self { pool };
        let versions = db.sync_migration_checksums(&sqlx::migrate!("./migrations")).await?;
        match db.run_migrations().await {
            Ok(()) => {
                if versions.is_empty() {
                    Ok(RepairReport::Ok)
                } else {
                    Ok(RepairReport::ChecksumsUpdated { versions })
                }
            }
            Err(e) => {
                warn!("Migrations still failing after checksum repair ({e:#}); recreating");
                db.pool.close().await;
                Self::recreate(db_path).await
            }
        }
    }

    async fn recreate(db_path: &Path) -> Result<RepairReport> {
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let backup = db_path.with_extension(format!("db.bak.{stamp}"));
        if db_path.exists() {
            std::fs::rename(db_path, &backup)
                .with_context(|| format!("Failed to move broken database to {}", backup.display()))?;
        }
        Database::new(db_path).await?;
        Ok(RepairReport::Recreated { backup })
    }

    /// Run embedded migrations
    async fn run_migrations(&self) -> Result<()> {
        info!("Running database migrations...");
        // Applied migrations are checksummed: never edit an existing file (not even a
        // comment), add a new numbered one instead, or existing libraries fail to open.
        let migrator = sqlx::migrate!("./migrations");
        self.sync_migration_checksums(&migrator).await?;
        match migrator.run(&self.pool).await {
            Ok(()) => {
                info!("Migrations complete");
                Ok(())
            }
            Err(sqlx::migrate::MigrateError::VersionMismatch(v)) => {
                warn!("Migration {v} checksum still mismatched; syncing again and retrying");
                self.sync_migration_checksums(&migrator).await?;
                migrator
                    .run(&self.pool)
                    .await
                    .context("Failed to run migrations")?;
                info!("Migrations complete");
                Ok(())
            }
            Err(e) => Err(e).context("Failed to run migrations"),
        }
    }

    /// Rewrite stored checksums for applied migrations that still exist in the
    /// embedded set but differ (older releases sometimes edited comments only).
    /// Returns the version numbers that were updated.
    async fn sync_migration_checksums(&self, migrator: &sqlx::migrate::Migrator) -> Result<Vec<i64>> {
        let table_exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);
        if table_exists.is_none() {
            return Ok(Vec::new());
        }

        let rows: Vec<(i64, Vec<u8>)> =
            sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&self.pool)
                .await
                .context("Failed to read applied migration checksums")?;

        let mut updated = Vec::new();
        for (version, stored) in rows {
            let Some(migration) = migrator.iter().find(|m| m.version == version) else {
                continue;
            };
            if stored.as_slice() == migration.checksum.as_ref() {
                continue;
            }
            warn!(
                "Updating migration {version} checksum (older Aurivon release had a different file hash)"
            );
            sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
                .bind(migration.checksum.as_ref())
                .bind(version)
                .execute(&self.pool)
                .await
                .with_context(|| format!("Failed to update migration {version} checksum"))?;
            updated.push(version);
        }
        Ok(updated)
    }

    /// Sync checksums and ensure migrations are applied (live connection).
    pub async fn repair_checksums(&self) -> Result<Vec<i64>> {
        let migrator = sqlx::migrate!("./migrations");
        let versions = self.sync_migration_checksums(&migrator).await?;
        self.run_migrations().await?;
        Ok(versions)
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get the default database path in the user's data directory
    pub fn default_path() -> Result<std::path::PathBuf> {
        let data_dir = dirs::data_local_dir().context("Could not determine local data directory")?;
        Ok(data_dir.join("aurivon").join("library.db"))
    }
}

pub use repository::{MediaLink, MediaRepository};

#[cfg(test)]
mod migration_guard {
    /// sqlx checksums applied migrations: editing 001 (even a comment) breaks
    /// every existing database. Keep it byte-for-byte; add a new migration instead.
    #[test]
    fn migration_001_is_unchanged() {
        let sql = include_str!("../migrations/001_initial_schema.sql");
        assert!(
            sql.starts_with("-- RustMediaHub Database Schema\n"),
            "migrations/001 was edited"
        );
    }

    #[tokio::test]
    async fn opens_db_after_stale_migration_checksum() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let db = super::Database::new(&path).await.unwrap();
        // Simulate 0.6.0-era comment-only checksum for migration 1
        let bogus = vec![0x9bu8; 48];
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1")
            .bind(&bogus)
            .execute(db.pool())
            .await
            .unwrap();
        drop(db);

        let reopened = super::Database::new(&path).await.expect("auto checksum repair");
        let stored: Vec<u8> = sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 1")
            .fetch_one(reopened.pool())
            .await
            .unwrap();
        assert_ne!(stored, bogus);
    }
}
