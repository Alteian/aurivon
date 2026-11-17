# Changelog

All notable changes to RustMediaHub. Versions follow [semver](https://semver.org):
MAJOR for breaking changes, MINOR for features, PATCH for fixes.

## [Unreleased]

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
