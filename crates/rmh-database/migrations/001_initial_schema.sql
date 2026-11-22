-- Aurivon database schema
-- Migration: 001_initial_schema

-- Libraries table: folders/sources being scanned
CREATE TABLE IF NOT EXISTS libraries (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    media_type TEXT NOT NULL DEFAULT 'movie',
    last_scanned TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Media items table: movies, shows, episodes
CREATE TABLE IF NOT EXISTS media_items (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT NOT NULL,
    title TEXT NOT NULL,
    file_path TEXT NOT NULL UNIQUE,
    file_name TEXT NOT NULL,
    file_size INTEGER NOT NULL DEFAULT 0,
    media_type TEXT NOT NULL DEFAULT 'movie',
    poster_url TEXT,
    backdrop_url TEXT,
    overview TEXT,
    release_date TEXT,
    rating REAL,
    runtime_minutes INTEGER,
    watched INTEGER NOT NULL DEFAULT 0,
    watch_progress INTEGER NOT NULL DEFAULT 0,
    tmdb_id INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_media_items_library ON media_items(library_id);
CREATE INDEX IF NOT EXISTS idx_media_items_title ON media_items(title);
CREATE INDEX IF NOT EXISTS idx_media_items_watched ON media_items(watched);
CREATE INDEX IF NOT EXISTS idx_media_items_tmdb_id ON media_items(tmdb_id);

-- Full-text search for media items (optional, for fast search)
CREATE VIRTUAL TABLE IF NOT EXISTS media_items_fts USING fts5(
    title,
    overview,
    content='media_items',
    content_rowid='rowid'
);

-- Triggers to keep FTS in sync
CREATE TRIGGER IF NOT EXISTS media_items_ai AFTER INSERT ON media_items BEGIN
    INSERT INTO media_items_fts(rowid, title, overview) 
    VALUES (NEW.rowid, NEW.title, NEW.overview);
END;

CREATE TRIGGER IF NOT EXISTS media_items_ad AFTER DELETE ON media_items BEGIN
    INSERT INTO media_items_fts(media_items_fts, rowid, title, overview) 
    VALUES('delete', OLD.rowid, OLD.title, OLD.overview);
END;

CREATE TRIGGER IF NOT EXISTS media_items_au AFTER UPDATE ON media_items BEGIN
    INSERT INTO media_items_fts(media_items_fts, rowid, title, overview) 
    VALUES('delete', OLD.rowid, OLD.title, OLD.overview);
    INSERT INTO media_items_fts(rowid, title, overview) 
    VALUES (NEW.rowid, NEW.title, NEW.overview);
END;

-- App settings/configuration
CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
