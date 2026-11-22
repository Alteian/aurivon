# Changelog

All notable changes to Aurivon. Versions follow [semver](https://semver.org):
MAJOR for breaking changes, MINOR for features, PATCH for fixes.

## [Unreleased]

## [0.9.1] - 2026-11-23

### Added
- Built-in Trakt public client id (PKCE); optional client-id override on Addons
- `aurivon --repair-db` and Settings → Storage → **Repair library database** (checksum fix; if still unusable, backs up and recreates an empty library DB)
- README + Integrations TMDB API key guide (key not baked into the app)

### Changed
- Settings uses category cards (Appearance, Libraries, Player, Integrations, …) instead of one long scroll; Addons is its own sidebar page (Ctrl+8)
- Trakt connect / sync / scrobble moved from Settings to Addons

### Fixed
- Upgrading from older releases that only changed migration SQL comments no longer fails with "migration … was previously applied but has been modified"; startup syncs stored checksums to the current embedded migrations

## [0.9.0] - 2026-11-23

### Added
- Bundled Aurivon addon catalog (official + community) with one-click install; paste-any-URL remains
- Official Trakt catalogs addon: History and Recommendations rows on Home when Trakt is connected
- Stronger Trakt sync: auto watched + watchlist sync on start and every ~45 min; immediate history push on local mark-watched; last-synced shown in Settings

### Fixed
- Windows embed: `RegisterClassExW` ATOM handling for current `windows` crate (unblocks Windows release builds)

### Changed
- Home drops the layout pill strip and duplicate search field; layout stays in Settings → Appearance (Ctrl+L), search in the page top bar / nav / Ctrl+K
- Addons rebranded away from Stremio naming; HTTP addon protocol unchanged (Torrentio-style hosts still work)
- Built-in Torrentio can be toggled; installing a remote Torrentio URL disables the built-in scraper to avoid double queries

## [0.8.1] - 2026-11-23

### Fixed
- First installable build of the 0.8.0 feature set: Linux/Windows packages (v0.8.0’s release job failed linking without `libxcb` / `libxkbcommon-x11`)

### Changed
- Ships on top of 0.8.0 with Trakt PKCE (no `client_secret`) and the CI linker deps from 0.7.1

## [0.8.0] - 2026-11-22

### Added
- Streaming-first Home (new default): auto-rotating hero carousel of what's trending (backdrop, rating, overview, ▶ Play best / + Watchlist / Details, ‹ › and ←/→), Continue watching (local + Trakt), Up next, New episodes this week, Trending today / this week, Because you watched…, Popular movies / series, New on streaming, Now in theaters, Top rated, genre rows, Watchlist; the local library is a small row at the bottom (hidden when empty)
- Rows load only when scrolled into view; small W185 posters; cards grow on hover and show rating / watched / progress; ‹ › row arrows and "See all" into a Browse grid of the whole list
- New logo (gold lyre + play + aurora) with wordmark, regenerated hicolor icons and an embedded in-app mark; BRANDING.md
- Brand system: design tokens (radii, spacing, type, elevation, motion) and six live-switchable themes — Aurum (default), Midnight, OLED Black, Aurora, Light, Violet (classic) — plus an accent picker and "follow system light / dark"
- Settings → Appearance: theme, accent, Home layout (Streaming, Cinematic, Compact, Continue & Up Next, Discover, Library), poster or landscape cards in three sizes, and sidebar / icon rail / top bar navigation, all applied instantly

### Changed
- Sidebar: Home, Search, Movies, Series, Calendar, Watchlist, Library, Settings; Ctrl+1–8 follow that order
- TMDB list / trending / recommendation responses are cached for 6 h (was 4 h)
- Existing installs switch to the new Home once; any later layout choice sticks

### Fixed
- Restored migration 001 byte-for-byte (a comment edit in 0.6.0 made existing databases fail with "migration 1 was previously applied but has been modified")

## [0.7.1] - 2026-11-23

### Fixed
- Linux CI/release link: install `libxcb1-dev` + `libxkbcommon-x11-dev` so mold can resolve `-lxcb` / `-lxkbcommon-x11`

### Changed
- Trakt login uses OAuth Authorization Code + PKCE (S256): client id only, HTTPS redirect via GitHub Pages → loopback; `client_secret` and device-code / oob flow removed (re-auth required)

## [0.7.0] - 2026-11-22

### Added
- Windows release: Inno Setup installer (`*-windows-setup.exe`) with bundled mpv, portable zip for the in-app updater, Win32 HWND embedded video
- mpv IPC over Windows named pipes (playback no longer Linux-only)

### Changed
- CI uses fewer Actions minutes: no release tarball on every push (only on tags / manual dispatch), mold linker, cache writes only from master; workspace tests paused (`cargo check` only)
- Updater picks OS-specific assets (Linux `.tar.gz` / Windows `.zip`) and activates `current` with a junction on Windows

## [0.6.0] - 2026-11-22

### Added
- GitHub Actions CI (check/test + Linux tarball artifact) and Release workflow that attaches `aurivon-*-x86_64-linux.tar.gz` to `v*` tags; `./install.sh` inside the archive is the user installer

### Changed
- Branding/docs/env: prefer `AURIVON_*` over legacy `RMH_*` (still accepted); addon ids `org.aurivon.*`; docs refer only to Aurivon paths (`~/.config/aurivon`, etc.)
- Updater downloads public release assets via CDN (`browser_download_url`); CI packages only when code/packaging paths change
- Settings → Storage: usage by images / torrents / TMDB / calendar / subtitles, image + torrent caps, prune/clear, low-power mode, prefer-debrid-when-disk-full
- Torrent orphan cleanup on startup and engine init; `stop_stream` can delete data; configurable torrent cache size
- Watchlist page (nav ★ / Ctrl+3) with bi-directional Trakt sync; local remove also removes on Trakt; single Home watchlist row
- Library type filter (All / Movies / Shows / Other), hide watched, rematch from Library
- Installable Stremio addons (manifest URL), TorBox + AllDebrid keys beside Real-Debrid
- Home row visibility toggles; next-episode stream presearch; TheIntroDB skip-intro when chapters are missing
- Local library / addon content-type detection for movies vs series (SxxExx / Season folders)

## [0.5.0] - 2026-11-18

### Added
- "▶ Play best" in the stream picker: plays the stream most likely to start fast among those that pass your filters: a Real-Debrid cached link if there is one, otherwise the healthiest torrent (more seeders first, quality breaks ties, 30 GB+ files count as slower)
- Torrent health in the picker: 🔥 100+ seeders, ▲ 25+, ▽ 5+, ✕ fewer; new "Starts fastest" sort
- Start-up status while a stream starts: checking Real-Debrid, finding peers, then "Buffering 40% · 3.1 MB/s · 18 peers"
- Up to 4 torrents race each other on Play best; the first one with its first few MB on disk plays, the others are dropped
- The torrent engine starts (DHT bootstrap) as soon as the stream list shows, not on the first Play

### Fixed
- Real-Debrid cache detection works again. Real-Debrid switched off `instantAvailability` (it now answers "disabled_endpoint"), so every stream showed as P2P and Real-Debrid was never used. Up to 12 candidates (the 6 that start fastest plus the 6 best-quality ones) are now checked the way the RD web UI does it (add the magnet, see if it is instantly "downloaded", delete it again); torrents RD refuses ("infringing_file") count as not cached
- Torrents find peers much faster: the trackers Torrentio lists for each torrent (and a few public fallback trackers) are now part of the magnet link; before, peers came only from the DHT
- The right file plays from season packs and torrents with extras (Torrentio's file index is used), and only that file is downloaded
- Playback starts on data that is already there: the first 4 MiB are buffered before mpv opens the stream and the end of the file (mkv index / mp4 header) is fetched first, instead of mpv stalling on an empty stream
- Torrents are stopped and their data deleted when you stop playback or start another stream (before, every stream you ever opened kept downloading until you quit)
- The local stream server answers "last N bytes" range requests

## [0.4.0] - 2026-11-18

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

