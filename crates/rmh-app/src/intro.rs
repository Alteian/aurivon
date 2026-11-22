//! Skip-intro timestamps from TheIntroDB (when chapters are missing).

use crate::service::{MediaService, ServiceResult};
use serde::Deserialize;
use std::future::Future;

#[derive(Debug, Clone, Default)]
pub struct IntroSegments {
    pub intro_end_ms: Option<u64>,
    pub credits_start_ms: Option<u64>,
    pub recap_end_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct IntroResp {
    #[serde(default)]
    intro: Vec<Seg>,
    #[serde(default)]
    credits: Vec<Seg>,
    #[serde(default)]
    recap: Vec<Seg>,
}

#[derive(Debug, Deserialize)]
struct Seg {
    start_ms: Option<u64>,
    end_ms: Option<u64>,
}

impl MediaService {
    /// Look up intro/credits for a TVDb id + duration. Returns empty when disabled or missing.
    pub fn intro_segments(&self, tvdb_id: i32, duration_ms: u64) -> impl Future<Output = ServiceResult<IntroSegments>> + 'static {
        let enabled = self.config().introdb_skip;
        let http = self.inner.http.clone();
        self.run(async move {
            if !enabled || tvdb_id <= 0 || duration_ms == 0 {
                return Ok(IntroSegments::default());
            }
            let url = format!("https://api.theintrodb.org/v4/media?tvdb_id={tvdb_id}&duration_ms={duration_ms}");
            let resp = http.get(&url).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Ok(IntroSegments::default());
            }
            let body: IntroResp = resp.json().await.map_err(|e| e.to_string())?;
            Ok(IntroSegments {
                intro_end_ms: body.intro.first().and_then(|s| s.end_ms),
                credits_start_ms: body.credits.first().and_then(|s| s.start_ms),
                recap_end_ms: body.recap.first().and_then(|s| s.end_ms),
            })
        })
    }
}
