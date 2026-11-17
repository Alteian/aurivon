//! Addon/Plugin system for Aurivon
//! 
//! This module provides a Stremio-compatible addon architecture that allows
//! extending the media center with custom content sources, metadata providers,
//! stream resolvers, and subtitle providers.
//!
//! ## Addon Types
//! 
//! - **Catalog**: Browse and discover content (movies, series, channels)
//! - **Meta**: Fetch detailed metadata for content items
//! - **Stream**: Resolve playable stream URLs for content
//! - **Subtitles**: Fetch subtitles for content
//!
//! ## Example Addon Manifest
//! 
//! ```json
//! {
//!   "id": "org.example.myaddon",
//!   "name": "My Addon",
//!   "version": "1.0.0",
//!   "description": "Example addon",
//!   "resources": ["catalog", "stream"],
//!   "types": ["movie", "series"],
//!   "catalogs": [
//!     { "type": "movie", "id": "top", "name": "Top Movies" }
//!   ]
//! }
//! ```

pub mod manifest;
pub mod resources;
pub mod traits;
pub mod registry;
pub mod builtin;

pub use manifest::{AddonManifest, CatalogDescriptor, ContentType};
pub use resources::{MetaItem, Stream, Subtitle, CatalogResponse};
pub use traits::{Addon, CatalogProvider, MetaProvider, StreamProvider, SubtitleProvider};
pub use registry::AddonRegistry;
