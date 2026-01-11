# RustMediaHub

A lightweight, high-performance media center application built entirely in Rust. Designed as a modern, extensible alternative to Stremio and Kodi.

## Features

- 🎬 **Native GPU-accelerated UI** - Uses Iced with wgpu (Vulkan/Metal/DX12)
- 🔌 **Extensible Addon System** - Stremio-compatible plugin architecture
- 📂 **Library Management** - Scan directories for video files
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
│   ├── rmh-player/        # MPV process management
│   ├── rmh-addon/         # Stremio-compatible addon system
│   └── rmh-ui/            # Iced GUI components
```

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

**Ubuntu/Debian:**
```bash
sudo apt update
sudo apt install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    mpv \
    libmpv-dev \
    libgtk-3-dev \
    libxkbcommon-dev \
    libwayland-dev \
    libvulkan-dev
```

**Fedora:**
```bash
sudo dnf install -y \
    gcc gcc-c++ \
    pkg-config \
    openssl-devel \
    sqlite-devel \
    mpv mpv-libs-devel \
    gtk3-devel \
    libxkbcommon-devel \
    wayland-devel \
    vulkan-loader-devel
```

**Arch Linux:**
```bash
sudo pacman -S --needed \
    base-devel \
    pkgconf \
    openssl \
    sqlite \
    mpv \
    gtk3 \
    libxkbcommon \
    wayland \
    vulkan-icd-loader
```

**macOS:**
```bash
brew install mpv openssl sqlite pkg-config
```

**Windows:**
1. Install [MSVC Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. Install [MPV](https://mpv.io/installation/) and add to PATH

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

2. **Add a library:** Click "Add Library Folder" and select a directory containing video files.

3. **Scan for media:** The scanner will recursively find all supported video files:
   - `.mkv`, `.mp4`, `.avi`, `.mov`, `.wmv`, `.flv`, `.webm`
   - `.m4v`, `.mpg`, `.mpeg`, `.ts`, `.m2ts`

4. **Play a video:** Click on any media card to launch MPV with hardware acceleration.

## Keyboard Shortcuts (MPV)

When a video is playing:
- `Space` - Pause/Play
- `←/→` - Seek 5 seconds
- `↑/↓` - Volume
- `f` - Toggle fullscreen
- `s` - Screenshot
- `q` - Quit

## Supported Video Formats

The scanner recognizes these extensions:
`mkv`, `mp4`, `avi`, `mov`, `wmv`, `flv`, `webm`, `m4v`, `mpg`, `mpeg`, `ts`, `m2ts`

## Development

### Project Structure

| Crate | Purpose |
|-------|---------|
| `rmh-core` | Shared types (`MediaItem`, `Library`), error handling |
| `rmh-database` | SQLite database with SQLx, migrations, repository pattern |
| `rmh-scanner` | Async directory traversal using `walkdir` |
| `rmh-metadata` | TMDB API client with `reqwest` |
| `rmh-player` | MPV process spawning with HW acceleration |
| `rmh-addon` | Stremio-compatible addon system (catalogs, streams, metadata, subtitles) |
| `rmh-ui` | Iced application, views, components |

### Running Tests

```bash
cargo test --workspace
```

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

Ensure MPV is installed and in your PATH:

```bash
mpv --version
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
