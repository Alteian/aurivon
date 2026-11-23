//! AniList (GraphQL) for anime: progress, completed and "planning".
//!
//! Login: create a client at <https://anilist.co/settings/developer> with the
//! redirect URL `https://anilist.co/api/v2/oauth/pin`, open
//! [`authorize_url`], approve, and paste the shown token into the app.

use crate::Secret;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const API: &str = "https://graphql.anilist.co";

pub fn authorize_url(client_id: &str) -> String {
    format!("https://anilist.co/api/v2/oauth/authorize?client_id={}&response_type=token", client_id.trim())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Current,
    Completed,
    Planning,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Current => "CURRENT",
            Status::Completed => "COMPLETED",
            Status::Planning => "PLANNING",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Token {
    access_token: String,
}

#[derive(Debug, Clone)]
pub struct Anime {
    pub id: i64,
    pub title: String,
    pub episodes: Option<u32>,
}

pub struct AniListClient {
    http: reqwest::Client,
    token: Secret,
}

impl AniListClient {
    pub fn new(token_path: std::path::PathBuf) -> Result<Self> {
        Ok(Self { http: crate::http()?, token: Secret::new(token_path) })
    }

    pub fn is_logged_in(&self) -> bool {
        self.token.load::<Token>().is_some()
    }

    /// Store the token pasted from the AniList pin page (validated first)
    pub async fn set_token(&self, token: &str) -> Result<String> {
        let token = token.trim().trim_start_matches("access_token=").split('&').next().unwrap_or("").to_string();
        if token.len() < 20 {
            bail!("That doesn't look like an AniList token");
        }
        let name = self.viewer_with(&token).await?;
        self.token.save(&Token { access_token: token })?;
        Ok(name)
    }

    pub fn logout(&self) {
        self.token.clear();
    }

    async fn gql(&self, token: Option<&str>, query: &str, vars: Value) -> Result<Value> {
        let mut req = self.http.post(API).json(&json!({ "query": query, "variables": vars }));
        let saved = self.token.load::<Token>();
        if let Some(t) = token.or(saved.as_ref().map(|t| t.access_token.as_str())) {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.context("AniList request failed")?;
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        if let Some(e) = v.get("errors").and_then(|e| e.as_array()).and_then(|e| e.first()) {
            bail!("AniList: {}", e.get("message").and_then(|m| m.as_str()).unwrap_or("error"));
        }
        if !status.is_success() {
            bail!("AniList error {status}");
        }
        Ok(v)
    }

    async fn viewer_with(&self, token: &str) -> Result<String> {
        let v = self.gql(Some(token), "query { Viewer { id name } }", json!({})).await?;
        v.pointer("/data/Viewer/name").and_then(|n| n.as_str()).map(str::to_string).context("AniList: no viewer")
    }

    pub async fn username(&self) -> Result<String> {
        let v = self.gql(None, "query { Viewer { id name } }", json!({})).await?;
        v.pointer("/data/Viewer/name").and_then(|n| n.as_str()).map(str::to_string).context("AniList: no viewer")
    }

    /// Best anime match for a title (+ year)
    pub async fn find(&self, title: &str, year: Option<u16>) -> Result<Option<Anime>> {
        let q = "query ($search: String, $year: Int) { Page(perPage: 5) { media(search: $search, type: ANIME, seasonYear: $year) { id episodes title { romaji english } } } }";
        let mut v = self.gql(None, q, json!({ "search": title, "year": year })).await?;
        let mut list = v.pointer_mut("/data/Page/media").and_then(|m| m.as_array_mut()).map(std::mem::take).unwrap_or_default();
        if list.is_empty() && year.is_some() {
            let v2 = self.gql(None, q, json!({ "search": title })).await?;
            list = v2.pointer("/data/Page/media").and_then(|m| m.as_array()).cloned().unwrap_or_default();
        }
        Ok(list.first().map(|m| Anime {
            id: m.get("id").and_then(|i| i.as_i64()).unwrap_or(0),
            title: m
                .pointer("/title/english")
                .and_then(|t| t.as_str())
                .or(m.pointer("/title/romaji").and_then(|t| t.as_str()))
                .unwrap_or(title)
                .to_string(),
            episodes: m.get("episodes").and_then(|e| e.as_u64()).map(|e| e as u32),
        }))
    }

    /// Set list status / episode progress
    pub async fn save(&self, media_id: i64, status: Status, progress: Option<u32>) -> Result<()> {
        let q = "mutation ($id: Int, $status: MediaListStatus, $progress: Int) { SaveMediaListEntry(mediaId: $id, status: $status, progress: $progress) { id status progress } }";
        self.gql(None, q, json!({ "id": media_id, "status": status.as_str(), "progress": progress })).await?;
        Ok(())
    }

    /// Mark episode `n` watched (completes the show on its last episode)
    pub async fn watched_episode(&self, anime: &Anime, n: u32) -> Result<()> {
        let status = if anime.episodes.is_some_and(|e| n >= e) { Status::Completed } else { Status::Current };
        self.save(anime.id, status, Some(n)).await
    }
}

/// TMDB doesn't flag anime; Japanese-language animation is the usual heuristic
/// Same rule as discovery hide-anime: Japanese + TMDB Animation (16).
pub fn looks_like_anime(original_language: Option<&str>, genre_ids: &[i32]) -> bool {
    original_language == Some("ja") && genre_ids.contains(&16)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anime_heuristic_and_url() {
        assert!(looks_like_anime(Some("ja"), &[16, 10759]));
        assert!(!looks_like_anime(Some("en"), &[16]));
        assert!(authorize_url(" 123 ").ends_with("client_id=123&response_type=token"));
    }
}
