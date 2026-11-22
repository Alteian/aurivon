# Aurivon

A lightweight, high-performance media center application built entirely in Rust. Designed as a modern, extensible alternative to Stremio and Kodi.

## Features

- 🎬 **Native GPU-accelerated UI** - GPUI (the Zed UI framework), GPU-rendered
- 🔌 **Extensible Addon System** - Stremio-compatible plugin architecture
- 📂 **Library Management** - Scan directories for video and audio files
- ⏯️ **In-app playback controls** - Play/pause, seek, ±10s, volume, stop, resume where you left off
- 🎯 **Metadata Fetching** - Automatic metadata from TMDB (The Movie Database)
- ▶️ **Hardware-Accelerated Playback** - Uses MPV with `--hwdec=auto`
- 💾 **SQLite Database** - Fast, embedded storage with compile-time checked queries
- 🔍 **Full-Text Search** - SQLite FTS5 for instant search
- 🔎 **Instant global search** (`Ctrl+K` / `/`) across your library, TMDB titles and people, typo tolerant ("interstelar" → Interstellar)
- 🧭 **Browse** with genre, year, rating, runtime, language and streaming-provider filters + sort, curated rows, keyboard-navigable grid
- 👤 **Person pages**, cast & crew, trailers, *Recommended* / *Similar* titles, "available locally / Real-Debrid / on Netflix" badges
- ⭐ **Watchlist from anywhere** (`W` or the + on any card), mirrored to Trakt / Simkl / AniList
- 📅 **Release calendar** (Trakt calendars, TMDB fallback): month + agenda views, my shows / everything, cinema vs digital, local time
- 💬 **OpenSubtitles** search by file hash + TMDB id, auto-download in your languages (Czech + English by default)
- 🔁 **Simkl + AniList** scrobbling / sync

## Architecture

```
aurivon/
├── src/                    # Main application entry point
├── crates/
│   ├── rmh-core/          # Core types, models, error handling
│   ├── rmh-database/      # SQLx + SQLite repository layer
│   ├── rmh-scanner/       # Recursive directory scanner
│   ├── rmh-metadata/      # TMDB API client
│   ├── rmh-player/        # UI-agnostic mpv player (JSON IPC)
│   ├── rmh-addon/         # Stremio-compatible addon system
│   ├── rmh-torrent/       # Torrent streaming engine
│   ├── rmh-scraper/       # Stream scrapers / Real-Debrid
│   └── rmh-ui/            # Iced GUI components
```

### Playback architecture

Video is played by `mpv` running as a separate process in its own window, controlled
over its JSON IPC socket. `rmh-player` exposes a small synchronous handle
(`Player::play`, `set_paused`, `seek`, `seek_relative`, `set_volume`, `stop`,
`snapshot()`) backed by its own background thread and Tokio runtime, so it does not
depend on any GUI toolkit. The iced UI just calls those methods and polls
`snapshot()` to draw the now-playing bar. All core crates (`rmh-core`,
`rmh-database`, `rmh-scanner`, `rmh-player`, ...) are UI-agnostic, so a different
front end (e.g. GPUI) can be dropped in by replacing `rmh-ui`.

## Addon System

Aurivon features a Stremio-compatible addon architecture for extensibility:

### Addon Types

| Resource | Description |
|----------|-------------|
| **Catalog** | Browse and discover content (movies, series, channels) |
| **Meta** | Fetch detailed metadata for content items |
| **Stream** | Resolve playable stream URLs |
| **Subtitles** | Fetch subtitles for content |

### Built-in Addons

- **Local Files** - Scans and serves your local media library

### Creating Custom Addons

```rust
use rmh_addon::{
    Addon, AddonManifest, CatalogProvider, ContentType, 
    CatalogResponse, ExtraParams, MetaItem,
};

struct MyAddon {
    manifest: AddonManifest,
}

#[async_trait]
impl Addon for MyAddon {
    fn manifest(&self) -> &AddonManifest {
        &self.manifest
    }
}

#[async_trait]
impl CatalogProvider for MyAddon {
    async fn catalog(
        &self,
        content_type: &ContentType,
        catalog_id: &str,
        extra: &ExtraParams,
    ) -> AddonResult<CatalogResponse> {
        // Return your catalog items...
        Ok(CatalogResponse::new(vec![
            MetaItem::new("my:123", "movie", "My Movie")
                .with_poster("https://example.com/poster.jpg")
                .with_year(2024),
        ]))
    }
}
```

## Requirements

### System Dependencies

Runtime: **mpv** (the player). Everything else is pure Rust or already on a
typical desktop. `libmpv-dev` is **not** needed (mpv is driven over IPC, not linked).

**Ubuntu/Debian/Pop!_OS:**
```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev \
    mpv libxkbcommon-dev libwayland-dev libvulkan-dev
# optional, only for running the playback tests:
sudo apt install -y ffmpeg
```

The "Browse..." folder picker uses the XDG desktop portal (`xdg-desktop-portal`,
present on GNOME/KDE/COSMIC). If it is unavailable, type the folder path instead.

**Fedora:**
```bash
sudo dnf install -y gcc gcc-c++ pkg-config openssl-devel mpv \
    libxkbcommon-devel wayland-devel vulkan-loader-devel
```

**Arch Linux:**
```bash
sudo pacman -S --needed base-devel pkgconf openssl mpv libxkbcommon wayland vulkan-icd-loader
```

**macOS:**
```bash
brew install mpv pkg-config
```

**Windows (from source):**
1. Install [MSVC Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. Either use the release Setup installer (bundles mpv), or put `mpv.exe` on PATH /
   next to `aurivon.exe` under `mpv\mpv.exe`

### Rust

Requires Rust 1.75 or later:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update
```

## Building

### Development Build

```bash
# Clone the repository
git clone git@github.com:Alteian/aurivon.git
cd aurivon

# Build in debug mode
cargo build

# Run with debug logging
RUST_LOG=debug cargo run
```

### Release Build (Optimized)

```bash
cargo build --release

# Binary will be at: target/release/aurivon
```

## Configuration

### TMDB API Key (Optional)

To enable metadata fetching, get a free API key from [TMDB](https://www.themoviedb.org/settings/api):

```bash
# Create a .env file in the project root
echo "TMDB_API_KEY=your_api_key_here" > .env
```

Or set the environment variable:

```bash
export TMDB_API_KEY=your_api_key_here
```

### Data Location

Application data is stored in:

- **Linux:** `~/.local/share/aurivon/`
- **macOS:** `~/Library/Application Support/aurivon/`
- **Windows:** `%LOCALAPPDATA%\aurivon\`

## Install & updates

**From a GitHub Release (recommended):** each `v*` tag builds install packages in CI.

**Linux** — download `aurivon-<version>-x86_64-linux.tar.gz` (and `.sha256`), then:

```bash
tar xzf aurivon-*-x86_64-linux.tar.gz
cd aurivon-*
./install.sh          # ~/.local — launcher entry, icons, no root
```

**Windows** — download and run `aurivon-<version>-x86_64-windows-setup.exe`
(Inno Setup installer). It installs under `%LOCALAPPDATA%\aurivon\app\`, adds a Start Menu
shortcut, and **bundles mpv** so playback works without a separate install. Video embeds in
the app window via a Win32 HWND (`mpv --wid`). A portable
`aurivon-<version>-x86_64-windows.zip` is also attached (used by the in-app updater).

After the first install, stay current via Settings → Updates or `aurivon --update`: the app
checks GitHub Releases, verifies sha256, installs the new version side-by-side under
`…/aurivon/app/versions/`, and swaps `current` atomically.

**From a checkout:**

```bash
make install        # release build -> ~/.local, app launcher entry + icons
make uninstall      # remove (library/settings kept; scripts/uninstall.sh --purge removes them)
make package        # dist/aurivon-<version>-<arch>-linux.tar.gz + .sha256
```

- The app appears in the app launcher (COSMIC launcher / app library, GNOME, KDE) as
  **Aurivon**; from a terminal run `aurivon` (`~/.local/bin`).
- Files: `~/.local/share/aurivon/app/versions/<version>` (+ `current` symlink),
  launcher wrapper `~/.local/bin/aurivon`, `~/.local/share/applications/aurivon.desktop`,
  icons in `~/.local/share/icons/hicolor`, log in `~/.local/state/aurivon/aurivon.log`.
- API keys: the installed app reads `~/.config/aurivon/.env` (mode 0600).
  `make install` copies `TMDB_API_KEY` and `REAL_DEBRID_API_KEY` there from the repo `.env`
  the first time; Settings can override them. Other services are optional.
- On Wayland the launcher runs the app through XWayland so video stays embedded.
- Data lives under Aurivon paths only: `~/.config/aurivon`, `~/.local/share/aurivon`,
  `~/.cache/aurivon`, `~/.local/state/aurivon`.

**Updates.** Releases are GitHub releases tagged `vMAJOR.MINOR.PATCH` with a tarball and
its sha256 (built by `.github/workflows/release.yml`). The app checks on startup and every
6 hours (Settings → Updates), shows *Update available* with the changelog, downloads,
verifies the checksum, installs the new version next to the old one, switches atomically
and offers a restart. Pre-releases (`0.3.0-beta.1`) are only offered when opted in:

```bash
aurivon --version
aurivon --check-update     # add --pre to include pre-releases
aurivon --update
```

Public releases need no GitHub token. A token (`gh auth login`, `AURIVON_GITHUB_TOKEN`, or
`~/.config/aurivon/github_token`) is only useful for higher API rate limits.

**Releasing.** From a clean tree (normally on `master`):

```bash
scripts/release.sh patch            # 0.2.0 -> 0.2.1   fixes
scripts/release.sh minor            # 0.2.1 -> 0.3.0   features
scripts/release.sh major            # 0.3.0 -> 1.0.0   breaking
scripts/release.sh minor --pre beta # 0.3.0-beta.1 (pre-release)
# optional: also upload a locally built tarball
scripts/release.sh patch --with-local-package
```

It bumps `Cargo.toml`, moves *Unreleased* notes into the new version, tests, commits,
tags, pushes and creates the GitHub release. CI then attaches
`aurivon-<v>-x86_64-linux.tar.gz` + `.sha256`. Installed apps pick it up on their next check.

## Usage

1. **Launch the application:**
   ```bash
   cargo run --release
   ```

2. **Add a library:** On the Library page (or Settings → Libraries) type a folder path
   such as `~/Videos` and press Enter / **Add**, or click **Browse...** to pick one.

3. **Scan:** the folder is scanned recursively right after adding it. Use **Scan** in
   Settings to pick up new files later, and **Refresh** to reload the grid.

4. **Play:** double-click a card (single click opens the TMDB details page with
   seasons/episodes). Video plays **inside the app window** (mpv draws into an X11
   child window; on Wayland the app runs through XWayland). Around the video:
   - seek bar with **chapter marks** and a **thumbnail preview** on hover (needs `ffmpeg`)
   - side panel: **Chapters**, **Audio** and **Subtitle** tracks (incl. loading a file), **Speed**
   - **Skip intro / recap / credits** from chapter names, **frame stepping**, speed 0.25–3×
   - **Autoplay next episode** with a 10 s countdown (Play now / Cancel)
   - theater / fullscreen mode; mpv's own on-screen controller still works over the video.
   Progress is saved and resumes next time; items watched past 90 % are marked ✓.
   Settings → Player → *Separate mpv window* restores the old standalone fullscreen window.

5. **Find something:** `Ctrl+K` (or `/`) opens search from anywhere, `↑/↓` + `Enter`.
   `aurivon --search "dune"` starts with the search open. **Browse** (`Ctrl+3`) has
   filter panels (`F`), sort (`O`), movies/series (`V`), clear (`X`), more results (`L`).
   **Calendar** (`Ctrl+4`): `←/→/↑/↓` day, `PgUp/PgDn` month, `V` month/agenda, `M` mine,
   `Y` movies/series, `D` cinema/digital, `T` today, `R` refresh. Results are cached on disk
   (`~/.cache/aurivon/{tmdb,calendar,images}`).

6. **Home:** three layouts to compare (Settings → Home layout, the pills on Home, or `Ctrl+L`):
   **A** Continue watching + Up next, **B** Discover with a trending hero, **C** library grid.

### App shortcuts

- `Space`/`K` play/pause · `←/→` 10 s (`Shift` 60 s) · `↑/↓` volume · `M` mute
- `PgUp/PgDn` next/previous chapter · `I` skip intro · `N` next episode
- `,`/`.` frame back/forward · `[`/`]` speed · `Backspace` normal speed
- `A` cycle audio · `J` cycle subtitles · `O` search OpenSubtitles · `T` side-panel tab · `F` fullscreen · `Esc` leave · `S` stop
- `Ctrl+K` or `/` search · `W` add/remove watchlist (details, browse, calendar) · `Enter` open / resume
- `Ctrl+1–6` Home, Library, Browse, Calendar, Player, Settings · `Ctrl+L` home layout · `Ctrl+I` details

### Trakt

1. Create an API app at <https://trakt.tv/oauth/applications> with redirect URI
   `urn:ietf:wg:oauth:2.0:oob`.
2. Paste its client id + secret in Settings → Services → Trakt (or set
   `TRAKT_CLIENT_ID` / `TRAKT_CLIENT_SECRET`) and click **Save & connect**.
3. Enter the shown code at <https://trakt.tv/activate>. The token is stored in
   `~/.config/aurivon/trakt.json` (mode 0600).

Then playback is scrobbled (start/pause/stop), **Sync now** syncs watched history both
ways, and Trakt watchlist / progress rows appear on Home.

### More services (Settings → More services)

All keys come from Settings or the environment (`.env`); nothing is hard-coded.

- **Region / time zone:** `region` (default `CZ`) drives providers, release dates and the
  TMDB calendar; `timezone` (default `Europe/Prague`) is used for calendar times.
- **OpenSubtitles:** API key from <https://www.opensubtitles.com/consumers>
  (`OPENSUBTITLES_API_KEY`), optionally your account (`OPENSUBTITLES_USERNAME` /
  `OPENSUBTITLES_PASSWORD`) for more daily downloads. Preferred languages default to
  Czech + English; with *auto subtitles* on, the best match is fetched when playback starts.
- **Simkl:** client id from <https://simkl.com/settings/developer/> (`SIMKL_CLIENT_ID`),
  then **Connect** and enter the PIN at <https://simkl.com/pin>.
- **AniList:** client id from <https://anilist.co/settings/developer> with redirect URL
  `https://anilist.co/api/v2/oauth/pin` (`ANILIST_CLIENT_ID`), **Open login**, paste the token.
- Tokens are stored next to the database (`simkl.json`, `anilist.json`, mode 0600).
- The release calendar uses Trakt when you're connected (your shows + global premieres),
  otherwise TMDB (popular shows airing + regional theatrical/digital releases).

### Test the player without the GUI

```bash
cargo run -p rmh-player --example play -- /path/to/video.mkv
# then type: p (pause), s 60 (seek), v 50 (volume), q (stop)
```

### Experimental: embed mpv into the app window

`AURIVON_EMBED_PLAYER=1 WINIT_UNIX_BACKEND=x11 cargo run` embeds mpv via `--wid` (X11/XWayland only).
The video then covers the app window; press `q` in it to return.

## Supported Formats

The scanner recognizes these extensions (mpv plays almost anything):

- Video: `mkv`, `mp4`, `avi`, `mov`, `wmv`, `flv`, `webm`, `m4v`, `mpg`, `mpeg`, `ts`, `m2ts`
- Audio: `mp3`, `flac`, `ogg`, `opus`, `m4a`, `aac`, `wav`, `wma`, `alac`, `aiff`

## Development

### Project Structure

| Crate | Purpose |
|-------|---------|
| `rmh-core` | Shared types (`MediaItem`, `Library`), error handling |
| `rmh-database` | SQLite database with SQLx, migrations, repository pattern |
| `rmh-scanner` | Async directory traversal using `walkdir` |
| `rmh-metadata` | TMDB API client with `reqwest` |
| `rmh-player` | UI-agnostic mpv controller over JSON IPC (play/pause/seek/volume/stop, state snapshot) |
| `rmh-addon` | Stremio-compatible addon system (catalogs, streams, metadata, subtitles) |
| `rmh-ui` | Iced application, views, components |

### Running Tests

```bash
cargo test --workspace
```

With `mpv` and `ffmpeg` installed this also runs end-to-end playback tests: an 8 s
test clip is generated and played through a headless mpv (`--vo=null --ao=null`),
checking load, progress, pause, seek, volume, stop, resume position and error
reporting, both at the `rmh-player` level and through the UI's `update()` logic.
Without them those tests skip themselves.

### Database Migrations

Migrations are embedded at compile time. To add a new migration:

```bash
# Create a new migration file
touch crates/rmh-database/migrations/002_new_feature.sql

# Rebuild to include the migration
cargo build
```

## Troubleshooting

### MPV Not Found

Settings → Status shows whether mpv was found. Install it and make sure it is on PATH:

```bash
sudo apt install mpv
mpv --version
```

### Playback logs

```bash
RUST_LOG=info,rmh_player=debug cargo run
```

### Database Errors

Delete the database to reset:

```bash
rm ~/.local/share/aurivon/library.db
```

### GPU Rendering Issues

If you experience graphics issues, try:

```bash
# Force software rendering (slower but more compatible)
WGPU_BACKEND=vulkan cargo run
# or
WGPU_BACKEND=gl cargo run
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions welcome! Please open an issue or PR on GitHub.
