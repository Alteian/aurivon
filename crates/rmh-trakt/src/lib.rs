//! Trakt.tv integration.
//!
//! - Login with **OAuth Authorization Code + PKCE** (no `client_secret`):
//!   [`TraktClient::start_pkce_login`] → open `auth_url` in a browser →
//!   [`TraktClient::wait_for_pkce_login`] (HTTPS callback forwards to a local
//!   listener) or [`TraktClient::complete_pkce_login`] with a pasted code.
//! - Tokens are refreshed automatically and stored in
//!   `~/.config/aurivon/trakt.json` (mode 0600). Tokens are never logged.
//! - Scrobbling (`/scrobble/start|pause|stop`), watched history, playback
//!   progress and watchlist.
//!
//! Requires a Trakt API app (client id only) from
//! <https://trakt.tv/oauth/applications> with redirect URI
//! [`REDIRECT_URI`].

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};

const API: &str = "https://api.trakt.tv";
const AUTH: &str = "https://auth.trakt.tv";
/// Must match the redirect URI registered on the Trakt API app (HTTPS only).
pub const REDIRECT_URI: &str = "https://alteian.github.io/aurivon/oauth/trakt.html";
/// Official Aurivon Trakt API app (public client id; PKCE, no secret).
pub const DEFAULT_CLIENT_ID: &str = "dWeb1RYO6eR5BRe5STAgs1_UvvxGtPK_tvHG6B-NabI";
/// Loopback port the GitHub Pages callback forwards to (not registered with Trakt).
const LOOPBACK: &str = "127.0.0.1:18787";

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
        dirs::config_dir().map(|d| d.join("aurivon").join("trakt.json"))
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

/// Browser step of a PKCE login (verifier stays inside the client).
#[derive(Debug, Clone)]
pub struct PkceLogin {
    /// Open this URL in the system browser
    pub auth_url: String,
    pub redirect_uri: String,
}

struct PendingPkce {
    verifier: String,
    state: String,
    /// Bound in [`TraktClient::start_pkce_login`] so the browser can return immediately.
    listener: Option<TcpListener>,
}

fn pkce_verifier() -> Result<String> {
    // 64 chars from the unreserved set (43–128 required)
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut raw = [0u8; 64];
    getrandom::getrandom(&mut raw).map_err(|e| anyhow!("RNG failed for PKCE verifier: {e}"))?;
    Ok(raw.iter().map(|b| CHARS[(*b as usize) % CHARS.len()] as char).collect())
}

fn pkce_challenge_s256(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn random_state() -> Result<String> {
    let mut raw = [0u8; 16];
    getrandom::getrandom(&mut raw).map_err(|e| anyhow!("RNG failed for OAuth state: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(raw))
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

/// A watchlist / history / playback / list entry
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Entry {
    #[serde(rename = "type")]
    pub kind: String,
    pub movie: Option<MediaRef>,
    pub show: Option<MediaRef>,
    pub episode: Option<EpisodeRef>,
    /// Playback progress percent (sync/playback)
    pub progress: Option<f64>,
    /// Playback id for DELETE /sync/playback/{id}
    pub id: Option<i64>,
    pub rank: Option<i32>,
    pub listed_at: Option<String>,
    pub watched_at: Option<String>,
    pub paused_at: Option<String>,
}

/// Custom user list metadata (`GET /users/me/lists`)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserList {
    pub name: String,
    pub description: Option<String>,
    pub privacy: Option<String>,
    pub display_numbers: Option<bool>,
    pub allow_comments: Option<bool>,
    pub sort_by: Option<String>,
    pub sort_how: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub item_count: Option<i32>,
    pub comment_count: Option<i32>,
    pub likes: Option<i32>,
    #[serde(default)]
    pub ids: Ids,
}

/// Next episode + counts from show watched progress
#[derive(Debug, Clone, Default)]
pub struct ShowProgress {
    pub aired: u32,
    pub completed: u32,
    pub next_season: Option<u32>,
    pub next_number: Option<u32>,
    pub next_title: Option<String>,
}

/// Last sync timestamps (`GET /sync/last_activities`)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LastActivities {
    pub all: Option<String>,
    pub watched: Option<String>,
    pub watchlist: Option<String>,
    pub lists: Option<String>,
    pub recommendations: Option<String>,
    pub favorites: Option<String>,
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

/// Which Trakt calendar
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalendarKind {
    /// Every new episode
    Shows,
    /// New shows (season 1 premieres)
    NewShows,
    /// Season premieres
    Premieres,
    /// Theatrical movie releases
    Movies,
    /// Digital / physical releases
    Dvd,
}

impl CalendarKind {
    fn path(self) -> &'static str {
        match self {
            CalendarKind::Shows => "shows",
            CalendarKind::NewShows => "shows/new",
            CalendarKind::Premieres => "shows/premieres",
            CalendarKind::Movies => "movies",
            CalendarKind::Dvd => "dvd",
        }
    }
}

/// One calendar item
#[derive(Debug, Clone)]
pub struct CalendarEntry {
    pub kind: CalendarKind,
    /// RFC 3339 air time (episodes) or a plain date (movies)
    pub when: String,
    pub title: String,
    pub year: Option<i32>,
    pub tmdb_id: Option<i64>,
    pub imdb_id: Option<String>,
    pub episode: Option<(u32, u32, Option<String>)>,
}

impl CalendarEntry {
    fn from_value(v: Value, kind: CalendarKind) -> Option<Self> {
        let media: MediaRef = serde_json::from_value(v.get("show").or(v.get("movie"))?.clone()).ok()?;
        let when = v.get("first_aired").or(v.get("released")).and_then(|w| w.as_str())?.to_string();
        let episode = v.get("episode").and_then(|e| serde_json::from_value::<EpisodeRef>(e.clone()).ok()).map(|e| (e.season, e.number, e.title));
        Some(Self {
            kind,
            when,
            title: media.title.unwrap_or_default(),
            year: media.year,
            tmdb_id: media.ids.tmdb,
            imdb_id: media.ids.imdb,
            episode,
        })
    }
    pub fn is_movie(&self) -> bool {
        matches!(self.kind, CalendarKind::Movies | CalendarKind::Dvd)
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
    token_path: PathBuf,
    token: Arc<Mutex<Option<TraktToken>>>,
    pending: Arc<Mutex<Option<PendingPkce>>>,
    /// Abort [`Self::wait_for_pkce_login`] and drop the loopback listener.
    cancel_wait: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl TraktClient {
    /// Public-client constructor (PKCE). No `client_secret`.
    pub fn new(client_id: impl Into<String>, token_path: PathBuf) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Aurivon/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()?;
        let token = TraktToken::load(&token_path);
        Ok(Self {
            http,
            client_id: client_id.into(),
            token_path,
            token: Arc::new(Mutex::new(token)),
            pending: Arc::new(Mutex::new(None)),
            cancel_wait: Arc::new(Mutex::new(None)),
        })
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

    // ------------------------------------------------------------- login (PKCE)

    /// Start Authorization Code + PKCE. Binds the loopback callback first, then return
    /// [`PkceLogin::auth_url`] to open in a browser.
    pub async fn start_pkce_login(&self) -> Result<PkceLogin> {
        self.cancel_pkce_login().await;
        let listener = TcpListener::bind(LOOPBACK)
            .await
            .with_context(|| format!("Could not bind {LOOPBACK} (is another Aurivon login open?)"))?;
        let verifier = pkce_verifier()?;
        let challenge = pkce_challenge_s256(&verifier);
        let state = random_state()?;
        let auth_url = format!(
            "{AUTH}/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
            urlencoding_encode(&self.client_id),
            urlencoding_encode(REDIRECT_URI),
            urlencoding_encode(&state),
            urlencoding_encode(&challenge),
        );
        *self.pending.lock().await = Some(PendingPkce { verifier, state, listener: Some(listener) });
        info!("Trakt PKCE: listening on http://{LOOPBACK}/callback");
        Ok(PkceLogin { auth_url, redirect_uri: REDIRECT_URI.into() })
    }

    /// Stop a pending login and release `127.0.0.1:18787`.
    pub async fn cancel_pkce_login(&self) {
        self.stop_pkce_listener().await;
        *self.pending.lock().await = None;
    }

    /// Drop the loopback waiter but keep the PKCE verifier (for paste-code completion).
    pub async fn stop_pkce_listener(&self) {
        if let Some(tx) = self.cancel_wait.lock().await.take() {
            let _ = tx.send(());
        }
    }

    /// Wait for the HTTPS callback page to forward `?code=` to `http://127.0.0.1:18787/callback`.
    pub async fn wait_for_pkce_login(&self) -> Result<Option<String>> {
        let listener = {
            let mut guard = self.pending.lock().await;
            guard
                .as_mut()
                .and_then(|p| p.listener.take())
                .ok_or_else(|| anyhow!("No Trakt login in progress — click Connect again"))?
        };
        let (tx, rx) = oneshot::channel();
        *self.cancel_wait.lock().await = Some(tx);
        let accepted = tokio::select! {
            biased;
            _ = rx => {
                drop(listener);
                *self.cancel_wait.lock().await = None;
                // Keep `pending` so paste-code can still finish with the same verifier.
                bail!("Trakt login cancelled");
            }
            result = tokio::time::timeout(Duration::from_secs(15 * 60), accept_oauth_code(&listener)) => {
                *self.cancel_wait.lock().await = None;
                result.context("Trakt login timed out (15 minutes)")?
            }
        };
        let (code, state) = accepted?;
        self.finish_with_code_and_state(&code, Some(&state)).await
    }

    /// Paste the `code` from the callback page if the automatic forward failed.
    pub async fn complete_pkce_login(&self, code: &str) -> Result<Option<String>> {
        // Free the port so we don't leave a waiter racing the same one-time code.
        self.stop_pkce_listener().await;
        // Brief yield so `wait_for_pkce_login`'s cancel arm can drop the listener.
        tokio::task::yield_now().await;
        self.finish_with_code_and_state(code.trim(), None).await
    }

    async fn finish_with_code_and_state(&self, code: &str, state: Option<&str>) -> Result<Option<String>> {
        let pending = self
            .pending
            .lock()
            .await
            .take()
            .ok_or_else(|| anyhow!("No Trakt login in progress — click Connect again"))?;
        if let Some(s) = state {
            if s != pending.state {
                bail!("OAuth state mismatch — start login again");
            }
        }
        if code.is_empty() {
            bail!("Empty authorization code");
        }
        let resp = self
            .request(reqwest::Method::POST, "/oauth/token")
            .json(&json!({
                "code": code,
                "client_id": self.client_id,
                "grant_type": "authorization_code",
                "redirect_uri": REDIRECT_URI,
                "code_verifier": pending.verifier,
            }))
            .send()
            .await
            .context("Trakt token exchange failed")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("{}", format_oauth_token_error(&body));
        }
        let mut token: TraktToken = resp.json().await?;
        token.username = self.fetch_username_with(&token.access_token).await.ok();
        token.save(&self.token_path)?;
        let name = token.username.clone();
        *self.token.lock().await = Some(token);
        info!("Trakt PKCE login complete");
        Ok(name)
    }

    pub async fn logout(&self) -> Result<()> {
        self.cancel_pkce_login().await;
        if let Some(t) = self.token.lock().await.take() {
            let _ = self
                .request(reqwest::Method::POST, "/oauth/revoke")
                .json(&json!({ "token": t.access_token, "client_id": self.client_id }))
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
        // Public client refresh: client_id only (no secret), refresh tokens rotate
        let resp = self
            .request(reqwest::Method::POST, "/oauth/token")
            .json(&json!({
                "refresh_token": token.refresh_token,
                "client_id": self.client_id,
                "grant_type": "refresh_token",
                "redirect_uri": REDIRECT_URI,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!("Trakt token refresh failed: {body}");
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

    async fn delete(&self, path: &str) -> Result<()> {
        let resp = self.authed(reqwest::Method::DELETE, path).await?.send().await?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(());
        }
        if !status.is_success() {
            bail!("Trakt {path} failed: {status}");
        }
        Ok(())
    }

    /// POST with an arbitrary JSON body (season/show history helpers).
    pub async fn post_raw(&self, path: &str, body: Value) -> Result<Value> {
        self.post(path, body).await
    }

    async fn fetch_username_with(&self, access: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Me {
            username: String,
        }
        let resp = self.request(reqwest::Method::GET, "/users/me").bearer_auth(access).send().await?;
        if !resp.status().is_success() {
            bail!("Trakt /users/me failed: {}", resp.status());
        }
        Ok(resp.json::<Me>().await?.username)
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

    /// Personalized recommendations (movies + shows), as Entry-shaped items.
    pub async fn recommendations(&self, limit: u32) -> Result<Vec<Entry>> {
        #[derive(Deserialize)]
        struct RecMovie {
            movie: MediaRef,
        }
        #[derive(Deserialize)]
        struct RecShow {
            show: MediaRef,
        }
        let lim = limit.clamp(1, 100);
        let movies: Vec<RecMovie> = self.get(&format!("/recommendations/movies?limit={lim}")).await.unwrap_or_default();
        let shows: Vec<RecShow> = self.get(&format!("/recommendations/shows?limit={lim}")).await.unwrap_or_default();
        let mut out = Vec::with_capacity(movies.len() + shows.len());
        for m in movies {
            out.push(entry_movie(m.movie));
        }
        for s in shows {
            out.push(entry_show(s.show));
        }
        Ok(out)
    }

    /// Hide a recommendation so it stops appearing.
    pub async fn hide_recommendation(&self, movie: bool, trakt_or_slug: &str) -> Result<()> {
        let kind = if movie { "movies" } else { "shows" };
        self.delete(&format!("/recommendations/{kind}/{trakt_or_slug}")).await
    }

    /// Related titles for a movie or show (TMDB id or slug/trakt id string).
    pub async fn related(&self, movie: bool, id: &str, limit: u32) -> Result<Vec<Entry>> {
        let kind = if movie { "movies" } else { "shows" };
        let lim = limit.clamp(1, 100);
        let path = format!("/{kind}/{id}/related?limit={lim}");
        let refs: Vec<MediaRef> = self.get(&path).await?;
        Ok(refs
            .into_iter()
            .map(|m| if movie { entry_movie(m) } else { entry_show(m) })
            .collect())
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

    /// Remove watched history (unwatch)
    pub async fn remove_from_history(&self, items: &[Item]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        self.post("/sync/history/remove", sync_body(items)).await.map(|_| ())
    }

    pub async fn add_to_watchlist(&self, item: &Item) -> Result<()> {
        self.post("/sync/watchlist", sync_body(std::slice::from_ref(item))).await.map(|_| ())
    }

    pub async fn remove_from_watchlist(&self, item: &Item) -> Result<()> {
        self.post("/sync/watchlist/remove", sync_body(std::slice::from_ref(item)))
            .await
            .map(|_| ())
    }

    pub async fn remove_show_from_watchlist(&self, show_tmdb: i64) -> Result<()> {
        self.post(
            "/sync/watchlist/remove",
            json!({ "shows": [{ "ids": { "tmdb": show_tmdb } }] }),
        )
        .await
        .map(|_| ())
    }

    pub async fn add_shows_to_watchlist(&self, show_tmdb_ids: &[i64]) -> Result<()> {
        if show_tmdb_ids.is_empty() {
            return Ok(());
        }
        let shows: Vec<Value> = show_tmdb_ids.iter().map(|id| json!({ "ids": { "tmdb": id } })).collect();
        self.post("/sync/watchlist", json!({ "shows": shows })).await.map(|_| ())
    }

    /// Clear a playback progress entry (`DELETE /sync/playback/{id}`).
    pub async fn remove_playback(&self, playback_id: i64) -> Result<()> {
        self.delete(&format!("/sync/playback/{playback_id}")).await
    }

    pub async fn last_activities(&self) -> Result<LastActivities> {
        self.get("/sync/last_activities").await
    }

    /// Watched progress for a show (next episode + counts). `id` is slug, trakt, or `tmdb:123`.
    pub async fn show_watched_progress(&self, id: &str) -> Result<ShowProgress> {
        #[derive(Deserialize)]
        struct NextEp {
            season: u32,
            number: u32,
            title: Option<String>,
        }
        #[derive(Deserialize)]
        struct Prog {
            aired: Option<u32>,
            completed: Option<u32>,
            next_episode: Option<NextEp>,
        }
        let p: Prog = self.get(&format!("/shows/{id}/progress/watched")).await?;
        Ok(ShowProgress {
            aired: p.aired.unwrap_or(0),
            completed: p.completed.unwrap_or(0),
            next_season: p.next_episode.as_ref().map(|e| e.season),
            next_number: p.next_episode.as_ref().map(|e| e.number),
            next_title: p.next_episode.and_then(|e| e.title),
        })
    }

    // --------------------------------------------------------------- lists

    pub async fn user_lists(&self) -> Result<Vec<UserList>> {
        self.get("/users/me/lists").await
    }

    pub async fn list_items(&self, list_id: &str) -> Result<Vec<Entry>> {
        self.get(&format!("/users/me/lists/{list_id}/items")).await
    }

    pub async fn add_to_list(&self, list_id: &str, movies: &[i64], shows: &[i64]) -> Result<()> {
        if movies.is_empty() && shows.is_empty() {
            return Ok(());
        }
        let body = json!({
            "movies": movies.iter().map(|id| json!({ "ids": { "tmdb": id } })).collect::<Vec<_>>(),
            "shows": shows.iter().map(|id| json!({ "ids": { "tmdb": id } })).collect::<Vec<_>>(),
        });
        self.post(&format!("/users/me/lists/{list_id}/items"), body).await.map(|_| ())
    }

    pub async fn remove_from_list(&self, list_id: &str, movies: &[i64], shows: &[i64]) -> Result<()> {
        if movies.is_empty() && shows.is_empty() {
            return Ok(());
        }
        let body = json!({
            "movies": movies.iter().map(|id| json!({ "ids": { "tmdb": id } })).collect::<Vec<_>>(),
            "shows": shows.iter().map(|id| json!({ "ids": { "tmdb": id } })).collect::<Vec<_>>(),
        });
        self.post(&format!("/users/me/lists/{list_id}/items/remove"), body)
            .await
            .map(|_| ())
    }

    pub async fn create_list(&self, name: &str, description: Option<&str>) -> Result<UserList> {
        let body = json!({
            "name": name,
            "description": description.unwrap_or(""),
            "privacy": "private",
            "display_numbers": false,
            "allow_comments": false,
        });
        let resp = self
            .authed(reqwest::Method::POST, "/users/me/lists")
            .await?
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("Trakt create list failed: {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn update_list(&self, list_id: &str, name: &str, description: Option<&str>) -> Result<UserList> {
        let body = json!({
            "name": name,
            "description": description.unwrap_or(""),
        });
        let resp = self
            .authed(reqwest::Method::PUT, &format!("/users/me/lists/{list_id}"))
            .await?
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("Trakt update list failed: {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn delete_list(&self, list_id: &str) -> Result<()> {
        self.delete(&format!("/users/me/lists/{list_id}")).await
    }

    // ------------------------------------------------------------- ratings

    /// Rate 1–10. `movie` selects movies vs shows body.
    pub async fn add_rating(&self, movie: bool, tmdb: i64, rating: u8) -> Result<()> {
        let rating = rating.clamp(1, 10);
        let body = if movie {
            json!({ "movies": [{ "rating": rating, "ids": { "tmdb": tmdb } }] })
        } else {
            json!({ "shows": [{ "rating": rating, "ids": { "tmdb": tmdb } }] })
        };
        self.post("/sync/ratings", body).await.map(|_| ())
    }

    pub async fn remove_rating(&self, movie: bool, tmdb: i64) -> Result<()> {
        let body = if movie {
            json!({ "movies": [{ "ids": { "tmdb": tmdb } }] })
        } else {
            json!({ "shows": [{ "ids": { "tmdb": tmdb } }] })
        };
        self.post("/sync/ratings/remove", body).await.map(|_| ())
    }

    pub async fn ratings(&self) -> Result<Vec<Entry>> {
        self.get("/sync/ratings").await
    }

    // ----------------------------------------------------------- favorites

    pub async fn favorites(&self) -> Result<Vec<Entry>> {
        // Newer Trakt: /sync/favorites; fall back to empty on 404
        match self.get::<Vec<Entry>>("/sync/favorites").await {
            Ok(v) => Ok(v),
            Err(_) => Ok(Vec::new()),
        }
    }

    pub async fn add_favorite(&self, movie: bool, tmdb: i64) -> Result<()> {
        let body = if movie {
            json!({ "movies": [{ "ids": { "tmdb": tmdb } }] })
        } else {
            json!({ "shows": [{ "ids": { "tmdb": tmdb } }] })
        };
        self.post("/sync/favorites", body).await.map(|_| ())
    }

    pub async fn remove_favorite(&self, movie: bool, tmdb: i64) -> Result<()> {
        let body = if movie {
            json!({ "movies": [{ "ids": { "tmdb": tmdb } }] })
        } else {
            json!({ "shows": [{ "ids": { "tmdb": tmdb } }] })
        };
        self.post("/sync/favorites/remove", body).await.map(|_| ())
    }

    // ----------------------------------------------------------- collection

    pub async fn collection(&self, movies: bool) -> Result<Vec<Entry>> {
        let kind = if movies { "movies" } else { "shows" };
        self.get(&format!("/sync/collection/{kind}")).await
    }

    pub async fn add_to_collection(&self, movie: bool, tmdb: i64) -> Result<()> {
        let body = if movie {
            json!({ "movies": [{ "ids": { "tmdb": tmdb } }] })
        } else {
            json!({ "shows": [{ "ids": { "tmdb": tmdb } }] })
        };
        self.post("/sync/collection", body).await.map(|_| ())
    }

    pub async fn remove_from_collection(&self, movie: bool, tmdb: i64) -> Result<()> {
        let body = if movie {
            json!({ "movies": [{ "ids": { "tmdb": tmdb } }] })
        } else {
            json!({ "shows": [{ "ids": { "tmdb": tmdb } }] })
        };
        self.post("/sync/collection/remove", body).await.map(|_| ())
    }

    // --------------------------------------------------------------- hide

    /// Hide from calendar (`section` = `calendar`) or recommendations (`recommendations`).
    pub async fn hide_from(&self, section: &str, movie: bool, tmdb: i64) -> Result<()> {
        let body = if movie {
            json!({ "movies": [{ "ids": { "tmdb": tmdb } }] })
        } else {
            json!({ "shows": [{ "ids": { "tmdb": tmdb } }] })
        };
        self.post(&format!("/users/hidden/{section}"), body).await.map(|_| ())
    }

    pub async fn unhide_from(&self, section: &str, movie: bool, tmdb: i64) -> Result<()> {
        let body = if movie {
            json!({ "movies": [{ "ids": { "tmdb": tmdb } }] })
        } else {
            json!({ "shows": [{ "ids": { "tmdb": tmdb } }] })
        };
        self.post(&format!("/users/hidden/{section}/remove"), body).await.map(|_| ())
    }

    /// Release calendar. `my` = shows/movies the user follows (needs login),
    /// otherwise everything on Trakt. Dates are UTC; `start` is YYYY-MM-DD.
    pub async fn calendar(&self, my: bool, what: CalendarKind, start: &str, days: u32) -> Result<Vec<CalendarEntry>> {
        let scope = if my { "my" } else { "all" };
        let path = format!("/calendars/{scope}/{}/{start}/{}", what.path(), days.clamp(1, 33));
        let req = if my || self.is_logged_in().await {
            self.authed(reqwest::Method::GET, &path).await?
        } else {
            self.request(reqwest::Method::GET, &path)
        };
        let resp = req.send().await?;
        if !resp.status().is_success() {
            bail!("Trakt calendar failed: {}", resp.status());
        }
        let raw: Vec<Value> = resp.json().await?;
        Ok(raw.into_iter().filter_map(|v| CalendarEntry::from_value(v, what)).collect())
    }

    pub async fn add_show_to_watchlist(&self, show_tmdb: i64) -> Result<()> {
        self.post("/sync/watchlist", json!({ "shows": [{ "ids": { "tmdb": show_tmdb } }] })).await.map(|_| ())
    }
}

/// Turn Trakt's OAuth token error JSON into a short, actionable message.
fn format_oauth_token_error(body: &str) -> String {
    let (error, description) = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .map(|v| {
            (
                v.get("error").and_then(|e| e.as_str()).unwrap_or("").to_string(),
                v.get("error_description").and_then(|e| e.as_str()).unwrap_or("").to_string(),
            )
        })
        .unwrap_or_default();
    match error.as_str() {
        "invalid_grant" => {
            let detail = if description.is_empty() {
                String::new()
            } else {
                format!(" ({description})")
            };
            format!(
                "Trakt rejected the login code{detail}. Codes are single-use and expire quickly. \
                 In Aurivon: Cancel if still waiting, clear any Client id override, click Connect once, \
                 approve in the browser, and return here — only paste the code if the app is still Waiting."
            )
        }
        "invalid_client" | "unauthorized_client" => {
            format!(
                "Trakt rejected this app’s client id{}. Clear the Client id override on Addons (use the built-in id) and Connect again.",
                if description.is_empty() { String::new() } else { format!(" ({description})") }
            )
        }
        _ if !error.is_empty() => {
            format!(
                "Trakt login failed ({error}{}). Cancel, Connect once, and try again.",
                if description.is_empty() { String::new() } else { format!(": {description}") }
            )
        }
        _ => format!("Trakt rejected the authorization code: {body}"),
    }
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Minimal HTTP/1.1 GET parser for the loopback OAuth callback.
async fn accept_oauth_code(listener: &TcpListener) -> Result<(String, String)> {
    loop {
        let (mut socket, _) = listener.accept().await?;
        let mut buf = vec![0u8; 4096];
        let n = socket.read(&mut buf).await.unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let line = req.lines().next().unwrap_or("");
        let path = line.split_whitespace().nth(1).unwrap_or("");
        let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let mut code = None;
        let mut state = None;
        let mut error = None;
        for part in query.split('&') {
            let mut kv = part.splitn(2, '=');
            let k = kv.next().unwrap_or("");
            let v = urlencoding_decode(kv.next().unwrap_or(""));
            match k {
                "code" => code = Some(v),
                "state" => state = Some(v),
                "error" => error = Some(v),
                _ => {}
            }
        }
        let body = if code.is_some() {
            "<!doctype html><title>Aurivon</title><p>Connected — you can close this tab and return to Aurivon.</p>"
        } else {
            "<!doctype html><title>Aurivon</title><p>Login failed. Return to Aurivon and try again.</p>"
        };
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(resp.as_bytes()).await;
        let _ = socket.shutdown().await;
        if let Some(err) = error {
            bail!("Trakt authorization denied: {err}");
        }
        if let (Some(c), Some(s)) = (code, state) {
            return Ok((c, s));
        }
    }
}

fn urlencoding_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn entry_movie(movie: MediaRef) -> Entry {
    Entry {
        kind: "movie".into(),
        movie: Some(movie),
        show: None,
        episode: None,
        progress: None,
        id: None,
        rank: None,
        listed_at: None,
        watched_at: None,
        paused_at: None,
    }
}

fn entry_show(show: MediaRef) -> Entry {
    Entry {
        kind: "show".into(),
        movie: None,
        show: Some(show),
        episode: None,
        progress: None,
        id: None,
        rank: None,
        listed_at: None,
        watched_at: None,
        paused_at: None,
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
    fn parses_calendar_entries() {
        let ep = json!({"first_aired": "2026-11-20T02:00:00.000Z", "episode": {"season": 2, "number": 5, "title": "X", "ids": {}},
            "show": {"title": "Severance", "year": 2022, "ids": {"tmdb": 95396}}});
        let e = CalendarEntry::from_value(ep, CalendarKind::Shows).unwrap();
        assert_eq!(e.tmdb_id, Some(95396));
        assert_eq!(e.episode.as_ref().map(|e| (e.0, e.1)), Some((2, 5)));
        assert!(!e.is_movie());
        let mv = json!({"released": "2026-11-21", "movie": {"title": "Dune 3", "year": 2026, "ids": {"tmdb": 1}}});
        let m = CalendarEntry::from_value(mv, CalendarKind::Dvd).unwrap();
        assert!(m.is_movie() && m.when == "2026-11-21");
    }

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
