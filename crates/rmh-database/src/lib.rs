//! Database layer for Aurivon using SQLx + SQLite

pub mod migrations;
pub mod repository;

use anyhow::{Context, Result};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::path::Path;
use tracing::info;

/// Database connection pool wrapper
#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection, initializing the database file if needed
    pub async fn new(db_path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create database directory")?;
        }

        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        
        info!("Connecting to database: {}", db_path.display());

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .context("Failed to connect to database")?;

        let db = Self { pool };
        
        // Run migrations
        db.run_migrations().await?;

        Ok(db)
    }

    /// Run embedded migrations
    async fn run_migrations(&self) -> Result<()> {
        info!("Running database migrations...");
        // Applied migrations are checksummed: never edit an existing file (not even a
        // comment), add a new numbered one instead, or existing libraries fail to open.
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .context("Failed to run migrations")?;
        info!("Migrations complete");
        Ok(())
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get the default database path in the user's data directory
    pub fn default_path() -> Result<std::path::PathBuf> {
        let data_dir = dirs::data_local_dir()
            .context("Could not determine local data directory")?;
        Ok(data_dir.join("aurivon").join("library.db"))
    }
}

pub use repository::{MediaLink, MediaRepository};

