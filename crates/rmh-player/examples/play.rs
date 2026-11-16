//! Minimal CLI to test the player without the GUI:
//!   cargo run -p rmh-player --example play -- /path/to/video.mkv
//! Commands on stdin: p (pause toggle), s <sec> (seek), v <0-100> (volume), q (stop)

use rmh_player::{Player, PlayerConfig};
use std::io::BufRead;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("usage: play <file>");
    let mut player = Player::new(PlayerConfig::default());
    player.play(std::path::Path::new(&path))?;
    println!("commands: p | s <sec> | v <vol> | i (info) | q");
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next().and_then(|v| v.parse::<f64>().ok())) {
            (Some("p"), _) => player.toggle_pause()?,
            (Some("s"), Some(t)) => player.seek(t)?,
            (Some("v"), Some(v)) => player.set_volume(v)?,
            (Some("q"), _) => { player.stop()?; break; }
            _ => {}
        }
        println!("{:?}", player.snapshot());
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    Ok(())
}
