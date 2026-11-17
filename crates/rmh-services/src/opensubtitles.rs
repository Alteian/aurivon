//! OpenSubtitles.com REST API (v1).
//!
//! Needs an API key (consumer) from <https://www.opensubtitles.com/consumers>.
//! Searching works with the key alone; downloads work anonymously with a small
//! daily quota, or with a (free) account login for more.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

const API: &str = "https://api.opensubtitles.com/api/v1";

/// OpenSubtitles "moviehash": file size + 64-bit sums of the first and last 64 KiB
pub fn movie_hash(path: &Path) -> Result<String> {
    const CHUNK: u64 = 64 * 1024;
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len();
    if size < CHUNK * 2 {
        bail!("file too small for a hash");
    }
    let mut hash = size;
    let mut sum = |f: &mut std::fs::File| -> Result<()> {
        let mut buf = vec![0u8; CHUNK as usize];
        f.read_exact(&mut buf)?;
        for w in buf.chunks_exact(8) {
            hash = hash.wrapping_add(u64::from_le_bytes(w.try_into().unwrap()));
        }
        Ok(())
    };
    sum(&mut f)?;
    f.seek(SeekFrom::Start(size - CHUNK))?;
    sum(&mut f)?;
    Ok(format!("{hash:016x}"))
}

/// What we are looking for subtitles for
#[derive(Debug, Clone, Default)]
pub struct SubQuery {
    /// Local file (for the hash) – optional for streams
    pub file: Option<PathBuf>,
    /// Movie TMDB id, or the show's TMDB id for episodes
    pub tmdb_id: Option<i32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// Fallback free-text query ("Dune 2021")
    pub query: Option<String>,
    /// Preferred languages, best first ("cs", "en")
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtitle {
    pub file_id: i64,
    pub language: String,
    pub release: String,
    pub file_name: Option<String>,
    pub downloads: u64,
    pub hearing_impaired: bool,
    pub hash_match: bool,
    pub trusted: bool,
    pub ai_translated: bool,
    pub rating: f32,
}

impl Subtitle {
    /// "Czech · 12 345 downloads · exact match"
    pub fn label(&self) -> String {
        let mut s = format!("{} · {} downloads", language_name(&self.language), self.downloads);
        if self.hash_match {
            s.push_str(" · exact file match");
        }
        if self.hearing_impaired {
            s.push_str(" · SDH");
        }
        s
    }
}

pub fn language_name(code: &str) -> &str {
    match code {
        "cs" => "Czech",
        "sk" => "Slovak",
        "en" => "English",
        "de" => "German",
        "pl" => "Polish",
        "fr" => "French",
        "es" => "Spanish",
        "it" => "Italian",
        "pt-BR" | "pb" => "Portuguese (BR)",
        "pt-PT" | "pt" => "Portuguese",
        "hu" => "Hungarian",
        "ru" => "Russian",
        "uk" => "Ukrainian",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh-CN" | "zh" => "Chinese",
        other => other,
    }
}

/// Order results: preferred language first, then exact hash, trusted, not
/// machine-translated, downloads.
pub fn rank(subs: &mut [Subtitle], languages: &[String]) {
    let lang_rank = |l: &str| languages.iter().position(|p| p.eq_ignore_ascii_case(l)).unwrap_or(languages.len());
    subs.sort_by(|a, b| {
        lang_rank(&a.language)
            .cmp(&lang_rank(&b.language))
            .then(b.hash_match.cmp(&a.hash_match))
            .then(a.ai_translated.cmp(&b.ai_translated))
            .then(b.trusted.cmp(&a.trusted))
            .then(a.hearing_impaired.cmp(&b.hearing_impaired))
            .then(b.downloads.cmp(&a.downloads))
    });
}

#[derive(Serialize, Deserialize)]
struct Session {
    token: String,
    base_url: Option<String>,
}

pub struct OpenSubtitles {
    http: reqwest::Client,
    api_key: String,
    login: Option<(String, String)>,
    session: Mutex<Option<Session>>,
}

impl std::fmt::Debug for OpenSubtitles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenSubtitles").field("logged_in", &self.login.is_some()).finish_non_exhaustive()
    }
}

impl OpenSubtitles {
    pub fn new(api_key: String, username: Option<String>, password: Option<String>) -> Result<Self> {
        let login = match (username, password) {
            (Some(u), Some(p)) if !u.trim().is_empty() && !p.is_empty() => Some((u.trim().to_string(), p)),
            _ => None,
        };
        Ok(Self { http: crate::http()?, api_key: api_key.trim().to_string(), login, session: Mutex::new(None) })
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{API}{path}"))
            .header("Api-Key", &self.api_key)
            .header("Accept", "application/json")
    }

    pub async fn search(&self, q: &SubQuery) -> Result<Vec<Subtitle>> {
        // the API wants sorted, lower-case parameters (otherwise it redirects)
        let mut params: Vec<(String, String)> = vec![];
        let langs: Vec<String> = q.languages.iter().map(|l| l.trim().to_lowercase()).filter(|l| !l.is_empty()).collect();
        if !langs.is_empty() {
            let mut l = langs.clone();
            l.sort();
            params.push(("languages".into(), l.join(",")));
        }
        if let Some(h) = q.file.as_deref().and_then(|f| movie_hash(f).ok()) {
            params.push(("moviehash".into(), h));
        }
        match (q.tmdb_id, q.season, q.episode) {
            (Some(id), Some(s), Some(e)) => {
                params.push(("parent_tmdb_id".into(), id.to_string()));
                params.push(("season_number".into(), s.to_string()));
                params.push(("episode_number".into(), e.to_string()));
            }
            (Some(id), _, _) => params.push(("tmdb_id".into(), id.to_string())),
            _ => {
                if let Some(t) = q.query.as_deref().filter(|t| !t.trim().is_empty()) {
                    params.push(("query".into(), t.trim().to_lowercase()));
                }
            }
        }
        params.sort();
        let resp = self.req(reqwest::Method::GET, "/subtitles").query(&params).send().await.context("OpenSubtitles request failed")?;
        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            bail!("OpenSubtitles rejected the API key ({status})");
        }
        if !status.is_success() {
            bail!("OpenSubtitles error {status}");
        }
        let v: Value = resp.json().await?;
        let mut subs = parse_search(&v);
        rank(&mut subs, &langs);
        Ok(subs)
    }

    async fn token(&self) -> Result<Option<String>> {
        let Some((user, pass)) = &self.login else { return Ok(None) };
        let mut s = self.session.lock().await;
        if let Some(s) = s.as_ref() {
            return Ok(Some(s.token.clone()));
        }
        let resp = self
            .req(reqwest::Method::POST, "/login")
            .json(&json!({ "username": user, "password": pass }))
            .send()
            .await
            .context("OpenSubtitles login failed")?;
        if !resp.status().is_success() {
            bail!("OpenSubtitles login failed ({}) – check username/password", resp.status());
        }
        let sess: Session = resp.json().await?;
        let t = sess.token.clone();
        *s = Some(sess);
        Ok(Some(t))
    }

    /// Download a subtitle into `dir`; returns the file path
    pub async fn download(&self, sub: &Subtitle, dir: &Path) -> Result<PathBuf> {
        let mut req = self.req(reqwest::Method::POST, "/download").json(&json!({ "file_id": sub.file_id }));
        if let Some(t) = self.token().await? {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.context("OpenSubtitles download failed")?;
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
            if status.as_u16() == 406 {
                bail!("OpenSubtitles daily download limit reached. {msg} Log in with an account for more.");
            }
            bail!("OpenSubtitles download error {status} {msg}");
        }
        let link = v.get("link").and_then(|l| l.as_str()).context("no download link")?;
        let bytes = self.http.get(link).send().await?.error_for_status()?.bytes().await?;
        std::fs::create_dir_all(dir)?;
        let ext = sub
            .file_name
            .as_deref()
            .and_then(|f| Path::new(f).extension())
            .and_then(|e| e.to_str())
            .unwrap_or("srt")
            .to_string();
        let path = dir.join(format!("{}.{}.{ext}", sub.file_id, sub.language));
        std::fs::write(&path, &bytes)?;
        Ok(path)
    }
}

fn parse_search(v: &Value) -> Vec<Subtitle> {
    let empty = vec![];
    v.get("data")
        .and_then(|d| d.as_array())
        .unwrap_or(&empty)
        .iter()
        .filter_map(|d| {
            let a = d.get("attributes")?;
            let file = a.get("files")?.as_array()?.first()?;
            Some(Subtitle {
                file_id: file.get("file_id")?.as_i64()?,
                file_name: file.get("file_name").and_then(|f| f.as_str()).map(str::to_string),
                language: a.get("language").and_then(|l| l.as_str()).unwrap_or("?").to_string(),
                release: a.get("release").and_then(|l| l.as_str()).unwrap_or_default().to_string(),
                downloads: a.get("download_count").and_then(|l| l.as_u64()).unwrap_or(0),
                hearing_impaired: a.get("hearing_impaired").and_then(|l| l.as_bool()).unwrap_or(false),
                hash_match: a.get("moviehash_match").and_then(|l| l.as_bool()).unwrap_or(false),
                trusted: a.get("from_trusted").and_then(|l| l.as_bool()).unwrap_or(false),
                ai_translated: a.get("ai_translated").and_then(|l| l.as_bool()).unwrap_or(false)
                    || a.get("machine_translated").and_then(|l| l.as_bool()).unwrap_or(false),
                rating: a.get("ratings").and_then(|l| l.as_f64()).unwrap_or(0.0) as f32,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_reference_algorithm() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.bin");
        // 200 KiB of a known pattern
        let data: Vec<u8> = (0..200 * 1024u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&p, &data).unwrap();
        let mut h = data.len() as u64;
        for w in data[..65536].chunks_exact(8).chain(data[data.len() - 65536..].chunks_exact(8)) {
            h = h.wrapping_add(u64::from_le_bytes(w.try_into().unwrap()));
        }
        assert_eq!(movie_hash(&p).unwrap(), format!("{h:016x}"));
        std::fs::write(&p, b"tiny").unwrap();
        assert!(movie_hash(&p).is_err());
    }

    #[test]
    fn parses_and_ranks() {
        let v = json!({"data": [
            {"attributes": {"language": "en", "download_count": 9000, "release": "A", "files": [{"file_id": 1, "file_name": "a.srt"}]}},
            {"attributes": {"language": "cs", "download_count": 10, "release": "B", "files": [{"file_id": 2}]}},
            {"attributes": {"language": "cs", "download_count": 5, "moviehash_match": true, "release": "C", "files": [{"file_id": 3}]}},
            {"attributes": {"language": "cs", "download_count": 99999, "ai_translated": true, "release": "D", "files": [{"file_id": 4}]}},
            {"attributes": {"language": "de", "files": []}}
        ]});
        let mut s = parse_search(&v);
        assert_eq!(s.len(), 4);
        rank(&mut s, &["cs".into(), "en".into()]);
        let ids: Vec<i64> = s.iter().map(|x| x.file_id).collect();
        assert_eq!(ids, vec![3, 2, 4, 1]);
        assert!(s[0].label().contains("exact file match"));
    }
}
