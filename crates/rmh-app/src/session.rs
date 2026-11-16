//! Playback session bookkeeping, independent of any UI toolkit.
//!
//! A front end creates a [`NowPlaying`] when it starts playback, feeds it a
//! [`PlaybackSnapshot`] on every poll and acts on the returned [`TickOutcome`].

use rmh_player::PlaybackSnapshot;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Save progress at least this often while playing (seconds of media time)
pub const SAVE_INTERVAL_SECS: f64 = 30.0;
/// Fraction of the duration after which an item counts as watched
pub const WATCHED_FRACTION: f64 = 0.9;
/// How long to wait for mpv to come up before treating playback as failed/ended
pub const STARTUP_GRACE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct NowPlaying {
    pub title: String,
    /// Library item being played (None for streams)
    pub media_id: Option<Uuid>,
    pub started_at: Instant,
    pub seen_active: bool,
    pub last_saved: f64,
    pub position: f64,
    pub duration: Option<f64>,
    pub paused: bool,
    pub volume: f64,
    pub eof: bool,
}

/// Progress to persist for a library item
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressUpdate {
    pub media_id: Uuid,
    /// Seconds to resume from (0 when watched)
    pub position: i64,
    pub watched: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TickOutcome {
    /// Nothing to do besides redrawing
    Continue,
    /// Still playing; persist this progress
    Save(ProgressUpdate),
    /// Playback ended (mpv closed or finished); persist and clear the bar
    Ended(Option<ProgressUpdate>),
    /// mpv reported an error; show it and clear the bar
    Failed(String),
}

impl NowPlaying {
    pub fn new(title: impl Into<String>, media_id: Option<Uuid>, volume: f64) -> Self {
        Self {
            title: title.into(),
            media_id,
            started_at: Instant::now(),
            seen_active: false,
            last_saved: 0.0,
            position: 0.0,
            duration: None,
            paused: false,
            volume,
            eof: false,
        }
    }

    /// Progress update for the current position (None for streams)
    pub fn progress(&mut self, finished: bool) -> Option<ProgressUpdate> {
        let id = self.media_id?;
        let watched = finished
            || self.eof
            || self.duration.is_some_and(|d| d > 0.0 && self.position / d >= WATCHED_FRACTION);
        self.last_saved = self.position;
        Some(ProgressUpdate {
            media_id: id,
            position: if watched { 0 } else { self.position as i64 },
            watched,
        })
    }

    /// Fold a player snapshot into the session
    pub fn tick(&mut self, snap: &PlaybackSnapshot) -> TickOutcome {
        if let Some(err) = &snap.last_error {
            return TickOutcome::Failed(err.clone());
        }
        if snap.active {
            self.seen_active = true;
            self.position = snap.position;
            self.duration = snap.duration;
            self.paused = snap.paused;
            self.volume = snap.volume;
            self.eof = snap.eof;
            if (self.position - self.last_saved).abs() >= SAVE_INTERVAL_SECS {
                if let Some(update) = self.progress(false) {
                    return TickOutcome::Save(update);
                }
            }
            return TickOutcome::Continue;
        }
        if self.seen_active || self.started_at.elapsed() > STARTUP_GRACE {
            let finished = snap.eof || self.duration.is_some_and(|d| d - self.position < 5.0);
            return TickOutcome::Ended(self.progress(finished));
        }
        TickOutcome::Continue
    }
}

/// Resume position for an item with saved progress
pub fn resume_position(watch_progress: i64) -> Option<u64> {
    (watch_progress > 5).then_some(watch_progress as u64)
}

/// "1:05:03" / "04:09"
pub fn format_time(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(active: bool, pos: f64, dur: f64) -> PlaybackSnapshot {
        PlaybackSnapshot { active, loaded: active, position: pos, duration: Some(dur), volume: 80.0, ..Default::default() }
    }

    #[test]
    fn saves_periodically_and_marks_watched() {
        let id = Uuid::new_v4();
        let mut np = NowPlaying::new("x", Some(id), 100.0);
        assert_eq!(np.tick(&snap(false, 0.0, 0.0)), TickOutcome::Continue); // still starting
        assert_eq!(np.tick(&snap(true, 10.0, 100.0)), TickOutcome::Continue);
        assert_eq!(np.tick(&snap(true, 31.0, 100.0)), TickOutcome::Save(ProgressUpdate { media_id: id, position: 31, watched: false }));
        assert_eq!(np.tick(&snap(true, 40.0, 100.0)), TickOutcome::Continue);
        assert_eq!(np.volume, 80.0);
        // mpv closed at 95% -> watched
        np.position = 95.0;
        assert_eq!(np.tick(&snap(false, 0.0, 0.0)), TickOutcome::Ended(Some(ProgressUpdate { media_id: id, position: 0, watched: true })));
    }

    #[test]
    fn stream_has_no_progress_and_errors_surface() {
        let mut np = NowPlaying::new("stream", None, 100.0);
        assert_eq!(np.tick(&snap(true, 50.0, 100.0)), TickOutcome::Continue);
        assert_eq!(np.tick(&snap(false, 0.0, 0.0)), TickOutcome::Ended(None));
        let bad = PlaybackSnapshot { last_error: Some("boom".into()), ..Default::default() };
        assert_eq!(np.tick(&bad), TickOutcome::Failed("boom".into()));
    }

    #[test]
    fn helpers() {
        assert_eq!(format_time(65.0), "01:05");
        assert_eq!(format_time(3903.0), "1:05:03");
        assert_eq!(resume_position(3), None);
        assert_eq!(resume_position(120), Some(120));
    }
}
