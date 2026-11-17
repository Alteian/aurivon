//! RustMediaHub - GPUI desktop front end (default).
//!
//! The legacy iced UI is still available: `cargo run --features iced --bin rustmediahub-iced`.

use rmh_app::update::{self, UpdateCheck};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const HELP: &str = "RustMediaHub - media center

Usage: rustmediahub [options]

  --search <query>   start with the search open
  --check-update     check GitHub releases for a newer version
  --update           download, verify and install the newest version
  --pre              with --check-update/--update: include pre-releases
  --version          print the version
  --help             this help

API keys are read from the environment, ./.env (development) and
~/.config/rustmediahub/.env (installed app), then Settings.";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--version") || has("-V") {
        println!("rustmediahub {}", update::CURRENT_VERSION);
        return Ok(());
    }
    if has("--help") || has("-h") {
        println!("{HELP}");
        return Ok(());
    }

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,rmh_player=info"));
    tracing_subscriber::registry().with(filter).with(tracing_subscriber::fmt::layer()).init();

    // API keys: process env wins, then ./.env (dev checkout), then the user's
    // ~/.config/rustmediahub/.env (installed app). Values are never logged.
    let _ = dotenvy::dotenv();
    if let Some(path) = rmh_app::AppConfig::user_env_path().filter(|p| p.is_file()) {
        let _ = dotenvy::from_path(&path);
    }

    if has("--check-update") || has("--update") {
        return update_cli(has("--update"), has("--pre"));
    }

    tracing::info!("Starting RustMediaHub {} (GPUI)", update::CURRENT_VERSION);
    rmh_gpui::run().map_err(anyhow::Error::msg)
}

fn update_cli(install: bool, pre: bool) -> anyhow::Result<()> {
    let pre = pre || rmh_app::AppConfig::load().map(|c| c.update_prereleases).unwrap_or(false);
    println!("Installed: {}", update::CURRENT_VERSION);
    match update::cli::check(pre)? {
        UpdateCheck::UpToDate { latest } => {
            println!("Up to date (latest release: {})", latest.as_deref().unwrap_or("none"));
            Ok(())
        }
        UpdateCheck::Available(rel) => {
            println!("Update available: {} ({})", rel.version, rel.html_url);
            if !rel.notes.trim().is_empty() {
                println!("\n{}\n", rel.notes.trim());
            }
            if !install {
                println!("Run `rustmediahub --update` to install it.");
                return Ok(());
            }
            let exe = update::cli::install(&rel)?;
            println!("Installed {} -> {}", rel.version, exe.display());
            println!("Restart RustMediaHub to use the new version.");
            Ok(())
        }
    }
}
