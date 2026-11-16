# RustMediaHub

A lightweight, high-performance media center application built entirely in Rust. Designed as a modern, extensible alternative to Stremio and Kodi.

## Features

- 🎬 **Native GPU-accelerated UI** - Uses Iced with wgpu (Vulkan/Metal/DX12)
- 🔌 **Extensible Addon System** - Stremio-compatible plugin architecture
- 📂 **Library Management** - Scan directories for video and audio files
- ⏯️ **In-app playback controls** - Play/pause, seek, ±10s, volume, stop, resume where you left off
- 🎯 **Metadata Fetching** - Automatic metadata from TMDB (The Movie Database)
- ▶️ **Hardware-Accelerated Playback** - Uses MPV with `--hwdec=auto`
- 💾 **SQLite Database** - Fast, embedded storage with compile-time checked queries
- 🔍 **Full-Text Search** - SQLite FTS5 for instant search

## Architecture

```
rustmediahub/
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

RustMediaHub features a Stremio-compatible addon architecture for extensibility:

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

**Windows:**
1. Install [MSVC Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. Install [MPV](https://mpv.io/installation/) and add to PATH
   (note: the IPC player currently uses Unix sockets, so playback is Linux/macOS only for now)

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
git clone https://github.com/Alteian/needmediacenter.git
cd needmediacenter

# Build in debug mode
cargo build

# Run with debug logging
RUST_LOG=debug cargo run
```

### Release Build (Optimized)

```bash
cargo build --release

# Binary will be at: target/release/rustmediahub
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

- **Linux:** `~/.local/share/rustmediahub/`
- **macOS:** `~/Library/Application Support/rustmediahub/`
- **Windows:** `%LOCALAPPDATA%\rustmediahub\`

## Usage

1. **Launch the application:**
   ```bash
   cargo run --release
   ```

2. **Add a library:** On the Library page (or Settings → Libraries) type a folder path
   such as `~/Videos` and press Enter / **Add**, or click **Browse...** to pick one.

3. **Scan:** the folder is scanned recursively right after adding it. Use **Scan** in
   Settings to pick up new files later, and **Refresh** to reload the grid.

4. **Play:** click a card. mpv opens in its own window; the app shows a
   **Now playing** bar with ⏪10s / Play-Pause / 10s⏩ / Stop, a seek slider, time and
   volume. Closing the mpv window also ends playback. Progress is saved and playback
   resumes from there next time; items watched past 90% are marked ✓.

### App shortcuts (while something plays)

- `Space` - Pause/Play
- `←/→` - Seek 10 seconds

### mpv window shortcuts

- `Space` - Pause/Play, `←/→` - Seek 5 s, `↑/↓` - Seek 1 min, `9/0` - Volume
- `f` - Toggle fullscreen, `s` - Screenshot, `q` - Quit

### Test the player without the GUI

```bash
cargo run -p rmh-player --example play -- /path/to/video.mkv
# then type: p (pause), s 60 (seek), v 50 (volume), q (stop)
```

### Experimental: embed mpv into the app window

`RMH_EMBED_PLAYER=1 WINIT_UNIX_BACKEND=x11 cargo run` embeds mpv via `--wid` (X11/XWayland only).
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
rm ~/.local/share/rustmediahub/library.db
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
