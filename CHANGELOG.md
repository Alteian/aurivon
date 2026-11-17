# Changelog

All notable changes to Aurivon. Versions follow [semver](https://semver.org):
MAJOR for breaking changes, MINOR for features, PATCH for fixes.

## [Unreleased]

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

