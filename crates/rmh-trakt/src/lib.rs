//! Trakt.tv integration.
//!
//! - Login with the OAuth **device code** flow (no browser redirect needed):
//!   [`TraktClient::start_device_login`] → show `user_code` + `verification_url`
//!   → [`TraktClient::poll_device_login`] until approved.
//! - Tokens are refreshed automatically and stored in
//!   `~/.config/rustmediahub/trakt.json` (mode 0600). Secrets are never logged.
//! - Scrobbling (`/scrobble/start|pause|stop`), watched history, playback
//!   progress and watchlist.
//!
//! Requires a Trakt API app (client id + secret) from
//! <https://trakt.tv/oauth/applications>.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

const API: &str = "https://api.trakt.tv";
const OOB: &str = "urn:ietf:wg:oauth:2.0:oob";

/// Stored OAuth tokens
#[derive(Clone, Serialize, Deserialize)]
pub struct TraktToken {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds
    pub created_at: i64,
    pub expires_in: i64,
    #[serde(default)]
    pub username: Option<String>,
}

impl std::fmt::Debug for TraktToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TraktToken").field("username", &self.username).field("expires_at", &self.expires_at()).finish_non_exhaustive()
    }
}

impl TraktToken {
    pub fn expires_at(&self) -> i64 {
        self.created_at + self.expires_in
    }
    fn needs_refresh(&self) -> bool {
        chrono::Utc::now().timestamp() > self.expires_at() - 24 * 3600
    }
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("rustmediahub").join("trakt.json"))
    }
    pub fn load(path: &PathBuf) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let data = serde_json::to_vec_pretty(self)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(path)?;
            f.write_all(&data)?;
        }
        #[cfg(not(unix))]
        std::fs::write(path, data)?;
        Ok(())
    }
}

/// Shown to the user while logging in
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PollStatus {
    /// User hasn't approved yet; poll again after `interval` seconds
    Pending,
    /// Polling too fast
    SlowDown,
    /// Logged in as this user
    Approved(Option<String>),
    /// Expired / denied / already used. Start over.
    Failed(String),
}

/// What is being watched, in Trakt terms (TMDB ids are accepted by Trakt)
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Movie { tmdb: i64 },
    Episode { show_tmdb: i64, season: u32, number: u32 },
}

impl Item {
    fn scrobble_body(&self, progress: f64) -> Value {
        let progress = (progress * 100.0).round() / 100.0;
        match self {
            Item::Movie { tmdb } => json!({ "movie": { "ids": { "tmdb": tmdb } }, "progress": progress }),
            Item::Episode { show_tmdb, season, number } => json!({
                "show": { "ids": { "tmdb": show_tmdb } },
                "episode": { "season": season, "number": number },
                "progress": progress
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrobbleAction {
    Start,
    Pause,
    Stop,
}

/// Ids as returned by Trakt
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Ids {
    pub trakt: Option<i64>,
    pub slug: Option<String>,
    pub imdb: Option<String>,
    pub tmdb: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MediaRef {
    pub title: Option<String>,
    pub year: Option<i32>,
    #[serde(default)]
    pub ids: Ids,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EpisodeRef {
    pub season: u32,
    pub number: u32,
    pub title: Option<String>,
    #[serde(default)]
    pub ids: Ids,
}

/// A watchlist / history / playback entry
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Entry {
    #[serde(rename = "type")]
    pub kind: String,
    pub movie: Option<MediaRef>,
    pub show: Option<MediaRef>,
    pub episode: Option<EpisodeRef>,
    /// Playback progress percent (sync/playback)
    pub progress: Option<f64>,
    pub listed_at: Option<String>,
    pub watched_at: Option<String>,
    pub paused_at: Option<String>,
}

impl Entry {
    /// "Dune (2021)" / "The Expanse S02E05"
    pub fn label(&self) -> String {
        match (&self.movie, &self.show, &self.episode) {
            (Some(m), _, _) => format!("{}{}", m.title.clone().unwrap_or_default(), m.year.map(|y| format!(" ({y})")).unwrap_or_default()),
            (_, Some(s), Some(e)) => format!("{} S{:02}E{:02}", s.title.clone().unwrap_or_default(), e.season, e.number),
            (_, Some(s), None) => format!("{}{}", s.title.clone().unwrap_or_default(), s.year.map(|y| format!(" ({y})")).unwrap_or_default()),
            _ => "?".into(),
        }
    }
    pub fn tmdb_id(&self) -> Option<i64> {
        self.movie.as_ref().or(self.show.as_ref()).and_then(|m| m.ids.tmdb)
    }
    pub fn imdb_id(&self) -> Option<String> {
        self.movie.as_ref().or(self.show.as_ref()).and_then(|m| m.ids.imdb.clone())
    }
}

/// Everything the user has watched, by TMDB id
#[derive(Debug, Clone, Default)]
pub struct Watched {
    pub movies: std::collections::HashSet<i64>,
    /// (show tmdb, season, episode)
    pub episodes: std::collections::HashSet<(i64, u32, u32)>,
}

/// Trakt API client. Cheap to clone.
#[derive(Clone)]
pub struct TraktClient {
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    token_path: PathBuf,
    token: Arc<Mutex<Option<TraktToken>>>,
}

impl TraktClient {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>, token_path: PathBuf) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("RustMediaHub/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()?;
        let token = TraktToken::load(&token_path);
        Ok(Self { http, client_id: client_id.into(), client_secret: client_secret.into(), token_path, token: Arc::new(Mutex::new(token)) })
    }

    pub async fn is_logged_in(&self) -> bool {
        self.token.lock().await.is_some()
    }

    pub async fn username(&self) -> Option<String> {
        self.token.lock().await.as_ref().and_then(|t| t.username.clone())
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{API}{path}"))
            .header("trakt-api-version", "2")
            .header("trakt-api-key", &self.client_id)
    }

    // ------------------------------------------------------------- login

    pub async fn start_device_login(&self) -> Result<DeviceCode> {
        let resp = self
            .request(reqwest::Method::POST, "/oauth/device/code")
            .json(&json!({ "client_id": self.client_id }))
            .send()
            .await
            .context("Could not reach Trakt")?;
        if !resp.status().is_success() {
            bail!("Trakt rejected the client id ({}). Check the Trakt client id in Settings.", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn poll_device_login(&self, code: &DeviceCode) -> Result<PollStatus> {
        let resp = self
            .request(reqwest::Method::POST, "/oauth/device/token")
            .json(&json!({ "code": code.device_code, "client_id": self.client_id, "client_secret": self.client_secret }))
            .send()
            .await?;
        Ok(match resp.status().as_u16() {
            200 => {
                let mut token: TraktToken = resp.json().await?;
                *self.token.lock().await = Some(token.clone());
                token.username = self.fetch_username().await.ok();
                token.save(&self.token_path)?;
                info!("Trakt login complete");
                let name = token.username.clone();
                *self.token.lock().await = Some(token);
                PollStatus::Approved(name)
            }
            400 => PollStatus::Pending,
            429 => PollStatus::SlowDown,
            404 => PollStatus::Failed("Invalid device code".into()),
            409 => PollStatus::Failed("Code already used".into()),
            410 => PollStatus::Failed("Code expired, try again".into()),
            418 => PollStatus::Failed("You denied the request on Trakt".into()),
            401 | 403 => PollStatus::Failed("Trakt rejected the client secret. Check it in Settings.".into()),
            s => PollStatus::Failed(format!("Trakt error {s}")),
        })
    }

    /// Poll until approved, failed or expired
    pub async fn wait_for_device_login(&self, code: &DeviceCode) -> Result<Option<String>> {
        let mut interval = code.interval.max(1);
        let deadline = std::time::Instant::now() + Duration::from_secs(code.expires_in);
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_secs(interval)).await;
            match self.poll_device_login(code).await? {
                PollStatus::Pending => {}
                PollStatus::SlowDown => interval += 1,
                PollStatus::Approved(user) => return Ok(user),
                PollStatus::Failed(msg) => bail!(msg),
            }
        }
        bail!("Trakt login timed out")
    }

    pub async fn logout(&self) -> Result<()> {
        if let Some(t) = self.token.lock().await.take() {
            let _ = self
                .request(reqwest::Method::POST, "/oauth/revoke")
                .json(&json!({ "token": t.access_token, "client_id": self.client_id, "client_secret": self.client_secret }))
                .send()
                .await;
        }
        let _ = std::fs::remove_file(&self.token_path);
        Ok(())
    }

    async fn access_token(&self) -> Result<String> {
        let mut guard = self.token.lock().await;
        let token = guard.as_ref().ok_or_else(|| anyhow!("Not logged in to Trakt"))?.clone();
        if !token.needs_refresh() {
            return Ok(token.access_token);
        }
        let resp = self
            .request(reqwest::Method::POST, "/oauth/token")
            .json(&json!({
                "refresh_token": token.refresh_token,
                "client_id": self.client_id,
                "client_secret": self.client_secret,
                "redirect_uri": OOB,
                "grant_type": "refresh_token"
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            warn!("Trakt token refresh failed: {}", resp.status());
            *guard = None;
            let _ = std::fs::remove_file(&self.token_path);
            bail!("Trakt session expired, please log in again");
        }
        let mut fresh: TraktToken = resp.json().await?;
        fresh.username = token.username;
        fresh.save(&self.token_path)?;
        let access = fresh.access_token.clone();
        *guard = Some(fresh);
        Ok(access)
    }

    async fn authed(&self, method: reqwest::Method, path: &str) -> Result<reqwest::RequestBuilder> {
        Ok(self.request(method, path).bearer_auth(self.access_token().await?))
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self.authed(reqwest::Method::GET, path).await?.send().await?;
        if !resp.status().is_success() {
            bail!("Trakt {path} failed: {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let resp = self.authed(reqwest::Method::POST, path).await?.json(&body).send().await?;
        let status = resp.status();
        // 409 on scrobble = same item scrobbled moments ago; harmless
        if status.as_u16() == 409 {
            return Ok(Value::Null);
        }
        if !status.is_success() {
            bail!("Trakt {path} failed: {status}");
        }
        Ok(resp.json().await.unwrap_or(Value::Null))
    }

    async fn fetch_username(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Me {
            username: String,
        }
        Ok(self.get::<Me>("/users/me").await?.username)
    }

    // ---------------------------------------------------------- scrobble

    /// `progress` is percent (0-100)
    pub async fn scrobble(&self, action: ScrobbleAction, item: &Item, progress: f64) -> Result<()> {
        let path = match action {
            ScrobbleAction::Start => "/scrobble/start",
            ScrobbleAction::Pause => "/scrobble/pause",
            ScrobbleAction::Stop => "/scrobble/stop",
        };
        self.post(path, item.scrobble_body(progress.clamp(0.0, 100.0))).await.map(|_| ())
    }

    // -------------------------------------------------------------- sync

    pub async fn watchlist(&self) -> Result<Vec<Entry>> {
        let mut all: Vec<Entry> = self.get("/sync/watchlist/movies?sort=added").await?;
        all.extend(self.get::<Vec<Entry>>("/sync/watchlist/shows?sort=added").await?);
        all.sort_by(|a, b| b.listed_at.cmp(&a.listed_at));
        Ok(all)
    }

    /// Unfinished playback ("continue watching")
    pub async fn playback(&self) -> Result<Vec<Entry>> {
        self.get("/sync/playback?limit=30").await
    }

    pub async fn history(&self, limit: u32) -> Result<Vec<Entry>> {
        self.get(&format!("/sync/history?limit={limit}")).await
    }

    pub async fn watched(&self) -> Result<Watched> {
        #[derive(Deserialize)]
        struct WMovie {
            movie: MediaRef,
        }
        #[derive(Deserialize)]
        struct WEp {
            number: u32,
        }
        #[derive(Deserialize)]
        struct WSeason {
            number: u32,
            #[serde(default)]
            episodes: Vec<WEp>,
        }
        #[derive(Deserialize)]
        struct WShow {
            show: MediaRef,
            #[serde(default)]
            seasons: Vec<WSeason>,
        }
        let mut w = Watched::default();
        for m in self.get::<Vec<WMovie>>("/sync/watched/movies").await? {
            if let Some(id) = m.movie.ids.tmdb {
                w.movies.insert(id);
            }
        }
        for s in self.get::<Vec<WShow>>("/sync/watched/shows").await? {
            if let Some(id) = s.show.ids.tmdb {
                for season in s.seasons {
                    for e in season.episodes {
                        w.episodes.insert((id, season.number, e.number));
                    }
                }
            }
        }
        Ok(w)
    }

    /// Mark items as watched on Trakt
    pub async fn add_to_history(&self, items: &[Item]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        self.post("/sync/history", sync_body(items)).await.map(|_| ())
    }

    pub async fn add_to_watchlist(&self, item: &Item) -> Result<()> {
        self.post("/sync/watchlist", sync_body(std::slice::from_ref(item))).await.map(|_| ())
    }

    pub async fn add_show_to_watchlist(&self, show_tmdb: i64) -> Result<()> {
        self.post("/sync/watchlist", json!({ "shows": [{ "ids": { "tmdb": show_tmdb } }] })).await.map(|_| ())
    }
}

/// Body for /sync/history and /sync/watchlist
pub fn sync_body(items: &[Item]) -> Value {
    let mut movies = vec![];
    let mut shows: std::collections::BTreeMap<i64, std::collections::BTreeMap<u32, Vec<u32>>> = Default::default();
    for it in items {
        match it {
            Item::Movie { tmdb } => movies.push(json!({ "ids": { "tmdb": tmdb } })),
            Item::Episode { show_tmdb, season, number } => {
                shows.entry(*show_tmdb).or_default().entry(*season).or_default().push(*number)
            }
        }
    }
    let shows: Vec<Value> = shows
        .into_iter()
        .map(|(id, seasons)| {
            json!({
                "ids": { "tmdb": id },
                "seasons": seasons.into_iter().map(|(n, eps)| json!({
                    "number": n,
                    "episodes": eps.into_iter().map(|e| json!({ "number": e })).collect::<Vec<_>>()
                })).collect::<Vec<_>>()
            })
        })
        .collect();
    json!({ "movies": movies, "shows": shows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        let ep = Item::Episode { show_tmdb: 63639, season: 2, number: 5 };
        assert_eq!(
            ep.scrobble_body(42.123),
            json!({"show":{"ids":{"tmdb":63639}},"episode":{"season":2,"number":5},"progress":42.12})
        );
        let body = sync_body(&[Item::Movie { tmdb: 603 }, ep.clone(), Item::Episode { show_tmdb: 63639, season: 2, number: 6 }]);
        assert_eq!(body["movies"][0]["ids"]["tmdb"], 603);
        assert_eq!(body["shows"][0]["seasons"][0]["episodes"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn token_debug_hides_secrets_and_file_is_private() {
        let t = TraktToken { access_token: "SECRET-A".into(), refresh_token: "SECRET-R".into(), created_at: 0, expires_in: 10, username: Some("d".into()) };
        assert!(!format!("{t:?}").contains("SECRET"));
        let dir = std::env::temp_dir().join(format!("rmh-trakt-test-{}", std::process::id()));
        let path = dir.join("trakt.json");
        t.save(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(TraktToken::load(&path).unwrap().username.as_deref(), Some("d"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn entry_labels() {
        let e: Entry = serde_json::from_value(json!({"type":"episode","show":{"title":"Dark","year":2017,"ids":{"tmdb":70523}},
            "episode":{"season":1,"number":3,"title":"Past and Present","ids":{}},"progress":33.0})).unwrap();
        assert_eq!(e.label(), "Dark S01E03");
        assert_eq!(e.tmdb_id(), Some(70523));
    }
}
