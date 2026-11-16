//! Playback session bookkeeping, independent of any UI toolkit.
//!
//! A front end creates a [`NowPlaying`] when it starts playback, feeds it a
//! [`PlaybackSnapshot`] on every poll and acts on the returned [`TickOutcome`].

use rmh_player::PlaybackSnapshot;
use rmh_trakt::{Item as TraktItem, ScrobbleAction};
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
    /// What to scrobble to Trakt (None = not linked / not a library match)
    pub trakt: Option<TraktItem>,
    /// Last scrobble action sent
    pub scrobbled: Option<ScrobbleAction>,
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
            trakt: None,
            scrobbled: None,
        }
    }

    pub fn with_trakt(mut self, item: Option<TraktItem>) -> Self {
        self.trakt = item;
        self
    }

    /// Percent watched (0-100)
    pub fn percent(&self) -> f64 {
        match self.duration {
            Some(d) if d > 0.0 => (self.position / d * 100.0).clamp(0.0, 100.0),
            _ => 0.0,
        }
    }

    /// Scrobble to send after a tick: `start` when (re)playing, `pause` when paused
    pub fn scrobble_update(&mut self) -> Option<(TraktItem, ScrobbleAction, f64)> {
        let item = self.trakt.clone()?;
        if !self.seen_active || self.duration.is_none_or(|d| d <= 0.0) || self.eof {
            return None;
        }
        let want = if self.paused { ScrobbleAction::Pause } else { ScrobbleAction::Start };
        if self.scrobbled == Some(want) || (want == ScrobbleAction::Pause && self.scrobbled.is_none()) {
            return None;
        }
        self.scrobbled = Some(want);
        Some((item, want, self.percent()))
    }

    /// Final `stop` scrobble when playback ends (Trakt marks >= 80% as watched)
    pub fn scrobble_stop(&mut self) -> Option<(TraktItem, ScrobbleAction, f64)> {
        let item = self.trakt.clone()?;
        self.scrobbled.take()?;
        let pct = if self.eof { 100.0 } else { self.percent() };
        Some((item, ScrobbleAction::Stop, pct))
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
    fn scrobble_state_machine() {
        let item = TraktItem::Movie { tmdb: 603 };
        let mut np = NowPlaying::new("m", None, 100.0).with_trakt(Some(item.clone()));
        assert_eq!(np.scrobble_update(), None); // not started yet
        np.tick(&snap(true, 10.0, 100.0));
        assert_eq!(np.scrobble_update(), Some((item.clone(), ScrobbleAction::Start, 10.0)));
        assert_eq!(np.scrobble_update(), None);
        np.tick(&PlaybackSnapshot { paused: true, ..snap(true, 50.0, 100.0) });
        assert_eq!(np.scrobble_update(), Some((item.clone(), ScrobbleAction::Pause, 50.0)));
        np.tick(&snap(true, 85.0, 100.0));
        assert_eq!(np.scrobble_update().map(|s| s.1), Some(ScrobbleAction::Start));
        assert_eq!(np.scrobble_stop(), Some((item, ScrobbleAction::Stop, 85.0)));
        assert_eq!(np.scrobble_stop(), None);
        // no Trakt link -> nothing
        let mut plain = NowPlaying::new("x", None, 100.0);
        plain.tick(&snap(true, 10.0, 100.0));
        assert_eq!(plain.scrobble_update(), None);
    }

    #[test]
    fn helpers() {
        assert_eq!(format_time(65.0), "01:05");
        assert_eq!(format_time(3903.0), "1:05:03");
        assert_eq!(resume_position(3), None);
        assert_eq!(resume_position(120), Some(120));
    }
}
