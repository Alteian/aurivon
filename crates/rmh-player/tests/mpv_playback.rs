//! End-to-end test: drives a real (headless) mpv through the Player API.
//! Skips gracefully when `mpv` or `ffmpeg` is not installed.

use rmh_player::{Player, PlayerConfig};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn have(bin: &str) -> bool {
    std::process::Command::new(bin).arg("-version").output().is_ok()
        || std::process::Command::new(bin).arg("--version").output().is_ok()
}

fn make_clip(dir: &Path) -> Option<PathBuf> {
    let out = dir.join("clip.mp4");
    let ok = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error",
            "-f", "lavfi", "-i", "testsrc=duration=8:size=320x240:rate=25",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=8",
            "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
        .arg(&out)
        .status()
        .ok()?
        .success();
    ok.then_some(out)
}

fn wait_for(p: &Player, what: &str, secs: u64, f: impl Fn(&rmh_player::PlaybackSnapshot) -> bool) {
    let start = Instant::now();
    loop {
        let s = p.snapshot();
        if f(&s) {
            return;
        }
        if start.elapsed() > Duration::from_secs(secs) {
            panic!("timed out waiting for {what}; last state: {s:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn plays_pauses_seeks_sets_volume_and_stops() {
    if !have("mpv") || !have("ffmpeg") {
        eprintln!("skipping: mpv or ffmpeg not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("rmh-player-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let clip = make_clip(&dir).expect("ffmpeg failed to create test clip");

    let mut player = Player::new(PlayerConfig { headless: true, hwdec: "no".into(), ..Default::default() });
    player.play(&clip).unwrap();

    wait_for(&player, "duration", 10, |s| s.active && s.duration.map_or(false, |d| d > 7.0));
    wait_for(&player, "playback progress", 10, |s| s.position > 0.3);

    player.set_paused(true).unwrap();
    wait_for(&player, "pause", 5, |s| s.paused);
    let paused_at = player.snapshot().position;
    std::thread::sleep(Duration::from_millis(500));
    assert!((player.snapshot().position - paused_at).abs() < 0.05, "position advanced while paused");

    player.seek(5.0).unwrap();
    wait_for(&player, "seek", 5, |s| (s.position - 5.0).abs() < 0.5);

    player.set_paused(false).unwrap();
    wait_for(&player, "resume", 5, |s| !s.paused);

    player.set_volume(35.0).unwrap();
    wait_for(&player, "volume", 5, |s| (s.volume - 35.0).abs() < 0.01);

    player.stop().unwrap();
    wait_for(&player, "stop", 5, |s| !s.active);

    // Start position + restart after stop
    player.play_at_position(&clip, Some(4)).unwrap();
    wait_for(&player, "restart at 4s", 10, |s| s.active && s.position >= 3.9);
    player.stop().unwrap();
    wait_for(&player, "stop", 5, |s| !s.active);

    // Broken file reports an error instead of hanging
    let bad = dir.join("bad.mkv");
    std::fs::write(&bad, b"not a video").unwrap();
    player.play(&bad).unwrap();
    wait_for(&player, "error for bad file", 10, |s| s.last_error.is_some());
    player.stop().unwrap();

    let _ = std::fs::remove_dir_all(&dir);
}
