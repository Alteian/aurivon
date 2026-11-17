//! Aurivon - A lightweight, performant media center application
//!
//! This is the main entry point that initializes logging, configuration,
//! and launches the Iced GUI application.

use anyhow::Result;
use iced::Size;
use rmh_ui::Aurivon;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn main() -> Result<()> {
    // Initialize logging
    init_logging();

    // Video plays in mpv's own window, controlled from the app's now-playing bar.
    // Experimental: RMH_EMBED_PLAYER=1 embeds mpv into the app window (X11/XWayland only).
    if std::env::var("RMH_EMBED_PLAYER").as_deref() == Ok("1") {
        info!("RMH_EMBED_PLAYER=1: mpv will be embedded into the main window (X11 only)");
    }

    info!("Starting Aurivon");

    // Load environment variables from .env file (if present)
    if let Err(e) = dotenvy::dotenv() {
        tracing::debug!("No .env file found: {}", e);
    }

    // Run the Iced application
    iced::application("Aurivon", Aurivon::update, Aurivon::view)
        .window_size(Size::new(1280.0, 800.0))
        .centered()
        .subscription(Aurivon::subscription)
        .run_with(Aurivon::new)?;

    info!("Aurivon shutdown");
    Ok(())
}

/// Initialize tracing/logging
fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,rmh_database=debug,rmh_scanner=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

