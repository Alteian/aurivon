# Changelog

All notable changes to Aurivon. Versions follow [semver](https://semver.org):
MAJOR for breaking changes, MINOR for features, PATCH for fixes.

## [Unreleased]

### Added
- Fullscreen everywhere: F11 or the ⛶ button in the top bar. On the player, F, the ⛶ button or a double-click on the video goes fullscreen with the video only (no sidebar, panels or bars; mpv's on-screen controller handles the controls); Esc or F leaves
- Stream filters in the stream picker: resolution (4K / 1080p / 720p / SD), HDR / Dolby Vision / SDR only, codec (HEVC / AV1 / H.264), audio (5.1+ / Atmos), source (Remux / BluRay / WEB / HDTV), min / max size, Real-Debrid cached only, and CAM / TS releases hidden by default
- Sort streams by best match, resolution, largest, smallest or seeders; badges show what each release has (e.g. "4K · DV · HEVC · Atmos · Remux")
- Default stream filters in Settings, plus "Save as default" in the picker
- Settings → Image cache: see its size, pick a limit (100 MB – 1 GB) or clear it

### Changed
- Much smaller first load: posters download only when they scroll into view, a few at a time, instead of every poster of every row up front; the Discover-layout trending row and hero only load when that home layout is used
- Release calendar fetches just the month on screen (not the whole 6-week grid), with fewer parallel lookups and lazy posters
- Image cache is capped (250 MB by default); the least recently used images are removed first, library posters are kept
- The stream picker keeps every result the scrapers return (was the top 50), so filters have something to work with
- "What's new" lists the changes of every version since yours, not just the newest

## [0.3.2] - 2026-11-17

### Fixed
- Existing libraries open again after the rename (0.3.0/0.3.1 changed an applied database migration and showed "Failed to run migrations"; no data was touched)

## [0.3.1] - 2026-11-17

### Fixed
- `aurivon --version`, `--check-update` and `--update` print their output when run from scripts too (only launcher starts go to the log)

## [0.3.0] - 2026-11-17

### Changed
- RustMediaHub is now **Aurivon**: `aurivon` command, "Aurivon" in the app launcher, new lyre-inspired icon
- Settings, keys, library and caches move automatically from the old `rustmediahub` folders to `aurivon`; the old install is replaced
- Releases are published to github.com/Alteian/aurivon (the repository was renamed)
- Settings → Updates shows when the last check ran

### Fixed
- Release notes keep their section spacing

## [0.2.0] - 2026-11-17

### Added
- Installable app: `make install` puts RustMediaHub in `~/.local` with an app-launcher entry (COSMIC, GNOME, KDE) and icons
- Automatic updates from GitHub releases: check on startup and every 6 hours, changelog, sha256-verified install, restart prompt
- `rustmediahub --check-update`, `--update`, `--version`; "Check for Updates" action on the launcher icon
- Settings → Updates: automatic checks and pre-release opt-in
- Instant search (Ctrl+K), Browse with filters, person pages, trailers, similar titles, watchlist
- Release calendar (Trakt / TMDB), OpenSubtitles, Simkl and AniList
- In-app video with chapters, skip intro/credits, tracks, speed, autoplay next, Trakt sync

### Changed
- The installed app reads API keys from `~/.config/rustmediahub/.env`
- Services that aren't set up stay out of the way

