//! RustMediaHub - GPUI desktop front end (default).
//!
//! The legacy iced UI is still available: `cargo run --features iced --bin rustmediahub-iced`.

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,rmh_player=info"));
    tracing_subscriber::registry().with(filter).with(tracing_subscriber::fmt::layer()).init();

    // Load .env (API keys) if present; values are never logged
    let _ = dotenvy::dotenv();

    tracing::info!("Starting RustMediaHub (GPUI)");
    rmh_gpui::run().map_err(anyhow::Error::msg)
}
