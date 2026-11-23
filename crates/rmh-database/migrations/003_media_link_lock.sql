-- Migration: 003_media_link_lock
-- User-locked matches survive force Re-match
ALTER TABLE media_links ADD COLUMN match_locked INTEGER NOT NULL DEFAULT 0;
