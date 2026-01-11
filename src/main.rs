//! RustMediaHub - A lightweight, performant media center application
//!
//! This is the main entry point that initializes logging, configuration,
//! and launches the Iced GUI application.

use anyhow::Result;
use iced::Size;
use rmh_ui::RustMediaHub;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn main() -> Result<()> {
    // Initialize logging
    init_logging();

    info!("Starting RustMediaHub");

    // Load environment variables from .env file (if present)
    if let Err(e) = dotenvy::dotenv() {
        tracing::debug!("No .env file found: {}", e);
    }

    // Run the Iced application
    iced::application("RustMediaHub", RustMediaHub::update, RustMediaHub::view)
        .window_size(Size::new(1280.0, 800.0))
        .centered()
        .run_with(RustMediaHub::new)?;

    info!("RustMediaHub shutdown");
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

