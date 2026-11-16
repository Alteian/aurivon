-- Migration: 002_media_links
-- How a local file maps onto TMDB / Trakt (movie, or show + season + episode)
CREATE TABLE IF NOT EXISTS media_links (
    media_id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,              -- 'movie' | 'tv' | 'none' (no match found)
    tmdb_id INTEGER,                 -- movie id or show id
    imdb_id TEXT,
    show_title TEXT,
    season INTEGER,
    episode INTEGER,
    episode_title TEXT,
    poster_path TEXT,                -- local cached poster file
    genres TEXT,
    matched_at TEXT NOT NULL DEFAULT (datetime('now')),
    trakt_synced_at TEXT,
    FOREIGN KEY (media_id) REFERENCES media_items(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_media_links_tmdb ON media_links(kind, tmdb_id);
