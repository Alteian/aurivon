//! Simkl (<https://simkl.docs.apiary.io>): PIN login, scrobbling, history and
//! "plan to watch" list. Needs a Simkl app client id from
//! <https://simkl.com/settings/developer/>.

use crate::Secret;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const API: &str = "https://api.simkl.com";

/// Movie, or an episode of a show (TMDB ids)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Item {
    Movie { tmdb: i32 },
    Episode { show_tmdb: i32, season: u32, number: u32 },
    Show { tmdb: i32 },
}

#[derive(Debug, Clone, Copy)]
pub enum Scrobble {
    Start,
    Pause,
    Stop,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pin {
    pub user_code: String,
    pub verification_url: String,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default = "default_expires")]
    pub expires_in: u64,
}
fn default_interval() -> u64 {
    5
}
fn default_expires() -> u64 {
    900
}

#[derive(Clone, Serialize, Deserialize)]
struct Token {
    access_token: String,
}

/// A title on a Simkl list
#[derive(Debug, Clone)]
pub struct ListItem {
    pub title: String,
    pub year: Option<u16>,
    pub tmdb_id: Option<i32>,
    pub is_show: bool,
}

pub struct SimklClient {
    http: reqwest::Client,
    client_id: String,
    token: Secret,
}

impl SimklClient {
    pub fn new(client_id: String, token_path: std::path::PathBuf) -> Result<Self> {
        Ok(Self { http: crate::http()?, client_id: client_id.trim().to_string(), token: Secret::new(token_path) })
    }

    pub fn is_logged_in(&self) -> bool {
        self.token.load::<Token>().is_some()
    }

    fn req(&self, method: reqwest::Method, path: &str) -> Result<reqwest::RequestBuilder> {
        let t: Token = self.token.load().context("Not connected to Simkl")?;
        Ok(self
            .http
            .request(method, format!("{API}{path}"))
            .header("simkl-api-key", &self.client_id)
            .bearer_auth(t.access_token))
    }

    pub async fn start_login(&self) -> Result<Pin> {
        let v: Value = self
            .http
            .get(format!("{API}/oauth/pin"))
            .query(&[("client_id", self.client_id.as_str())])
            .send()
            .await?
            .error_for_status()
            .context("Simkl rejected the client id")?
            .json()
            .await?;
        let mut pin: Pin = serde_json::from_value(v)?;
        if pin.verification_url.is_empty() {
            pin.verification_url = "https://simkl.com/pin".into();
        }
        Ok(pin)
    }

    /// Ok(true) once approved (token saved), Ok(false) while pending
    pub async fn poll_login(&self, pin: &Pin) -> Result<bool> {
        let v: Value = self
            .http
            .get(format!("{API}/oauth/pin/{}", pin.user_code))
            .query(&[("client_id", self.client_id.as_str())])
            .send()
            .await?
            .json()
            .await?;
        if let Some(t) = v.get("access_token").and_then(|t| t.as_str()) {
            self.token.save(&Token { access_token: t.to_string() })?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn wait_for_login(&self, pin: &Pin) -> Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(pin.expires_in);
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_secs(pin.interval.max(3))).await;
            if self.poll_login(pin).await? {
                return Ok(());
            }
        }
        bail!("Simkl code expired")
    }

    pub fn logout(&self) {
        self.token.clear();
    }

    pub async fn username(&self) -> Result<String> {
        let v: Value = self.req(reqwest::Method::POST, "/users/settings")?.send().await?.error_for_status()?.json().await?;
        Ok(v.pointer("/user/name").and_then(|n| n.as_str()).unwrap_or("Simkl user").to_string())
    }

    pub async fn scrobble(&self, action: Scrobble, item: Item, progress: f64) -> Result<()> {
        let path = match action {
            Scrobble::Start => "/scrobble/start",
            Scrobble::Pause => "/scrobble/pause",
            Scrobble::Stop => "/scrobble/stop",
        };
        let mut body = scrobble_body(item);
        body["progress"] = json!((progress.clamp(0.0, 100.0) * 100.0).round() / 100.0);
        let resp = self.req(reqwest::Method::POST, path)?.json(&body).send().await?;
        if !resp.status().is_success() && resp.status().as_u16() != 409 {
            bail!("Simkl scrobble error {}", resp.status());
        }
        Ok(())
    }

    pub async fn add_to_history(&self, items: &[Item]) -> Result<()> {
        let body = sync_body(items, None);
        self.req(reqwest::Method::POST, "/sync/history")?.json(&body).send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn add_to_watchlist(&self, items: &[Item]) -> Result<()> {
        let body = sync_body(items, Some("plantowatch"));
        self.req(reqwest::Method::POST, "/sync/add-to-list")?.json(&body).send().await?.error_for_status()?;
        Ok(())
    }

    /// "Plan to watch" movies and shows
    pub async fn watchlist(&self) -> Result<Vec<ListItem>> {
        let mut out = vec![];
        for (kind, key, is_show) in [("movies", "movie", false), ("shows", "show", true)] {
            let v: Value = self
                .req(reqwest::Method::GET, &format!("/sync/all-items/{kind}/plantowatch"))?
                .send()
                .await?
                .error_for_status()?
                .json()
                .await
                .unwrap_or(Value::Null);
            for it in v.get(kind).and_then(|a| a.as_array()).cloned().unwrap_or_default() {
                let Some(m) = it.get(key) else { continue };
                let tmdb = m.pointer("/ids/tmdb").and_then(|t| t.as_i64().or_else(|| t.as_str()?.parse().ok())).map(|t| t as i32);
                out.push(ListItem {
                    title: m.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_string(),
                    year: m.get("year").and_then(|y| y.as_u64()).map(|y| y as u16),
                    tmdb_id: tmdb,
                    is_show,
                });
            }
        }
        Ok(out)
    }
}

pub(crate) fn scrobble_body(item: Item) -> Value {
    match item {
        Item::Movie { tmdb } => json!({ "movie": { "ids": { "tmdb": tmdb } } }),
        Item::Show { tmdb } => json!({ "show": { "ids": { "tmdb": tmdb } } }),
        Item::Episode { show_tmdb, season, number } => {
            json!({ "show": { "ids": { "tmdb": show_tmdb } }, "episode": { "season": season, "number": number } })
        }
    }
}

pub(crate) fn sync_body(items: &[Item], list: Option<&str>) -> Value {
    let mut movies = vec![];
    let mut shows: Vec<Value> = vec![];
    for it in items {
        match *it {
            Item::Movie { tmdb } => {
                let mut m = json!({ "ids": { "tmdb": tmdb } });
                if let Some(l) = list {
                    m["to"] = json!(l);
                }
                movies.push(m);
            }
            Item::Show { tmdb } => {
                let mut s = json!({ "ids": { "tmdb": tmdb } });
                if let Some(l) = list {
                    s["to"] = json!(l);
                }
                shows.push(s);
            }
            Item::Episode { show_tmdb, season, number } => {
                shows.push(json!({ "ids": { "tmdb": show_tmdb }, "seasons": [{ "number": season, "episodes": [{ "number": number }] }] }));
            }
        }
    }
    json!({ "movies": movies, "shows": shows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        let b = sync_body(&[Item::Movie { tmdb: 1 }, Item::Episode { show_tmdb: 2, season: 1, number: 3 }], None);
        assert_eq!(b["movies"][0]["ids"]["tmdb"], 1);
        assert_eq!(b["shows"][0]["seasons"][0]["episodes"][0]["number"], 3);
        let w = sync_body(&[Item::Show { tmdb: 5 }], Some("plantowatch"));
        assert_eq!(w["shows"][0]["to"], "plantowatch");
        let s = scrobble_body(Item::Episode { show_tmdb: 2, season: 1, number: 3 });
        assert_eq!(s["episode"]["season"], 1);
    }

    #[test]
    fn token_file_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let c = SimklClient::new("id".into(), dir.path().join("simkl.json")).unwrap();
        assert!(!c.is_logged_in());
        c.token.save(&Token { access_token: "secret".into() }).unwrap();
        assert!(c.is_logged_in());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.path().join("simkl.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(!format!("{:?}", c.token).contains("secret"));
        c.logout();
        assert!(!c.is_logged_in());
    }
}
