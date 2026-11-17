//! Browsing & discovery on top of TMDB: curated lists, the /discover filter
//! engine, genres, streaming providers, people (cast / crew filmographies),
//! cast, trailers and similar / recommended titles for a details page.

use crate::catalog::{Kind, Page, RawTitle, Title};
use crate::TmdbClient;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Genre {
    pub id: i32,
    pub name: String,
}

/// Curated rows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListKind {
    TrendingDay,
    TrendingWeek,
    Popular,
    TopRated,
    /// Movies in cinemas / shows currently airing
    NowPlaying,
    /// Upcoming movies / shows airing today
    Upcoming,
}

impl ListKind {
    pub fn label(self, kind: Kind) -> &'static str {
        match (self, kind) {
            (ListKind::TrendingDay, _) => "Trending today",
            (ListKind::TrendingWeek, _) => "Trending this week",
            (ListKind::Popular, _) => "Popular",
            (ListKind::TopRated, _) => "Top rated",
            (ListKind::NowPlaying, Kind::Movie) => "New releases",
            (ListKind::NowPlaying, Kind::Tv) => "On the air",
            (ListKind::Upcoming, Kind::Movie) => "Upcoming",
            (ListKind::Upcoming, Kind::Tv) => "Airing today",
        }
    }

    fn path(self, kind: Kind) -> String {
        let k = kind.as_str();
        match (self, kind) {
            (ListKind::TrendingDay, _) => format!("/trending/{k}/day"),
            (ListKind::TrendingWeek, _) => format!("/trending/{k}/week"),
            (ListKind::Popular, _) => format!("/{k}/popular"),
            (ListKind::TopRated, _) => format!("/{k}/top_rated"),
            (ListKind::NowPlaying, Kind::Movie) => "/movie/now_playing".into(),
            (ListKind::NowPlaying, Kind::Tv) => "/tv/on_the_air".into(),
            (ListKind::Upcoming, Kind::Movie) => "/movie/upcoming".into(),
            (ListKind::Upcoming, Kind::Tv) => "/tv/airing_today".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Sort {
    #[default]
    Popularity,
    Rating,
    Newest,
    Oldest,
    Title,
    Votes,
}

impl Sort {
    pub const ALL: [Sort; 6] = [Sort::Popularity, Sort::Rating, Sort::Votes, Sort::Newest, Sort::Oldest, Sort::Title];
    pub fn label(self) -> &'static str {
        match self {
            Sort::Popularity => "Popular",
            Sort::Rating => "Top rated",
            Sort::Votes => "Most voted",
            Sort::Newest => "Newest",
            Sort::Oldest => "Oldest",
            Sort::Title => "A–Z",
        }
    }
    fn param(self, kind: Kind) -> &'static str {
        match (self, kind) {
            (Sort::Popularity, _) => "popularity.desc",
            (Sort::Rating, _) => "vote_average.desc",
            (Sort::Votes, _) => "vote_count.desc",
            (Sort::Newest, Kind::Movie) => "primary_release_date.desc",
            (Sort::Newest, Kind::Tv) => "first_air_date.desc",
            (Sort::Oldest, Kind::Movie) => "primary_release_date.asc",
            (Sort::Oldest, Kind::Tv) => "first_air_date.asc",
            (Sort::Title, Kind::Movie) => "title.asc",
            (Sort::Title, Kind::Tv) => "name.asc",
        }
    }
}

/// Everything the Browse page can filter on
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoverQuery {
    pub kind: Kind,
    pub genres: Vec<i32>,
    pub year_from: Option<u16>,
    pub year_to: Option<u16>,
    pub min_rating: Option<f32>,
    pub runtime_min: Option<u32>,
    pub runtime_max: Option<u32>,
    /// ISO 639-1 original language ("en", "cs", "ja", …)
    pub language: Option<String>,
    /// Streaming providers (flatrate) in `region`
    pub providers: Vec<i32>,
    pub region: String,
    pub sort: Sort,
    /// People (cast or crew)
    pub people: Vec<i32>,
    pub page: u32,
}

impl Default for DiscoverQuery {
    fn default() -> Self {
        Self {
            kind: Kind::Movie,
            genres: vec![],
            year_from: None,
            year_to: None,
            min_rating: None,
            runtime_min: None,
            runtime_max: None,
            language: None,
            providers: vec![],
            region: "CZ".into(),
            sort: Sort::Popularity,
            people: vec![],
            page: 1,
        }
    }
}

impl DiscoverQuery {
    /// Query parameters for /discover/{movie,tv}
    pub fn params(&self) -> Vec<(&'static str, String)> {
        let movie = self.kind == Kind::Movie;
        let mut p: Vec<(&'static str, String)> = vec![
            ("sort_by", self.sort.param(self.kind).to_string()),
            ("page", self.page.max(1).to_string()),
            ("include_adult", "false".into()),
        ];
        let join = |v: &[i32], sep: &str| v.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(sep);
        if !self.genres.is_empty() {
            p.push(("with_genres", join(&self.genres, ",")));
        }
        if let Some(y) = self.year_from {
            p.push((if movie { "primary_release_date.gte" } else { "first_air_date.gte" }, format!("{y}-01-01")));
        }
        if let Some(y) = self.year_to {
            p.push((if movie { "primary_release_date.lte" } else { "first_air_date.lte" }, format!("{y}-12-31")));
        }
        if let Some(r) = self.min_rating {
            p.push(("vote_average.gte", format!("{r:.1}")));
        }
        // rating / vote sorts are meaningless for titles with a handful of votes
        let min_votes = match self.sort {
            Sort::Rating => 300,
            _ if self.min_rating.is_some() => 50,
            _ => 0,
        };
        if min_votes > 0 {
            p.push(("vote_count.gte", min_votes.to_string()));
        }
        if let Some(r) = self.runtime_min {
            p.push(("with_runtime.gte", r.to_string()));
        }
        if let Some(r) = self.runtime_max {
            p.push(("with_runtime.lte", r.to_string()));
        }
        if let Some(l) = self.language.as_deref().filter(|l| !l.is_empty()) {
            p.push(("with_original_language", l.to_string()));
        }
        if !self.providers.is_empty() {
            p.push(("with_watch_providers", join(&self.providers, "|")));
            p.push(("watch_region", self.region.clone()));
            p.push(("with_watch_monetization_types", "flatrate".into()));
        }
        if !self.people.is_empty() && movie {
            p.push(("with_people", join(&self.people, "|")));
        }
        if matches!(self.sort, Sort::Newest) {
            // don't list unreleased titles first
            let today = chrono_today();
            p.push((if movie { "primary_release_date.lte" } else { "first_air_date.lte" }, today));
        }
        p
    }

    pub fn is_filtered(&self) -> bool {
        !self.genres.is_empty()
            || self.year_from.is_some()
            || self.year_to.is_some()
            || self.min_rating.is_some()
            || self.runtime_min.is_some()
            || self.runtime_max.is_some()
            || self.language.is_some()
            || !self.providers.is_empty()
            || !self.people.is_empty()
    }
}

fn chrono_today() -> String {
    // days since epoch -> civil date (no chrono dependency here)
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86400).unwrap_or(0) as i64;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

/// One page of results
#[derive(Debug, Clone, Default)]
pub struct ResultPage {
    pub results: Vec<Title>,
    pub page: u32,
    pub total_pages: u32,
    pub total_results: u32,
}

#[derive(Deserialize)]
struct RawPage {
    #[serde(default)]
    results: Vec<RawTitle>,
    #[serde(default)]
    page: u32,
    #[serde(default)]
    total_pages: u32,
    #[serde(default)]
    total_results: u32,
}

/// Streaming service
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    #[serde(rename = "provider_id")]
    pub id: i32,
    #[serde(rename = "provider_name")]
    pub name: String,
    pub logo_path: Option<String>,
    #[serde(default)]
    pub display_priority: Option<i32>,
}

/// A person as a search hit / cast card
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonCard {
    pub id: i32,
    pub name: String,
    pub profile_path: Option<String>,
    pub department: Option<String>,
    /// "Interstellar, Inception, …"
    pub known_for: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub titles: Vec<Title>,
    pub people: Vec<PersonCard>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastMember {
    pub id: i32,
    pub name: String,
    pub role: String,
    pub profile_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Video {
    pub name: String,
    pub key: String,
    pub site: String,
    pub kind: String,
    pub official: bool,
}

impl Video {
    pub fn url(&self) -> Option<String> {
        match self.site.as_str() {
            "YouTube" => Some(format!("https://www.youtube.com/watch?v={}", self.key)),
            "Vimeo" => Some(format!("https://vimeo.com/{}", self.key)),
            _ => None,
        }
    }
}

/// Details-page extras
#[derive(Debug, Clone, Default)]
pub struct Extras {
    pub cast: Vec<CastMember>,
    /// Directors (movies) / creators (shows)
    pub crew: Vec<CastMember>,
    pub trailers: Vec<Video>,
    pub recommendations: Vec<Title>,
    pub similar: Vec<Title>,
    /// Flatrate streaming in the requested region
    pub providers: Vec<Provider>,
    pub keywords: Vec<String>,
}

impl Extras {
    /// The trailer to open: official YouTube trailers first
    pub fn best_trailer(&self) -> Option<&Video> {
        self.trailers.iter().filter(|v| v.url().is_some()).max_by_key(|v| (v.kind == "Trailer", v.official, v.site == "YouTube"))
    }
}

/// A person page
#[derive(Debug, Clone)]
pub struct Person {
    pub id: i32,
    pub name: String,
    pub biography: Option<String>,
    pub birthday: Option<String>,
    pub deathday: Option<String>,
    pub place_of_birth: Option<String>,
    pub profile_path: Option<String>,
    pub department: Option<String>,
    pub imdb_id: Option<String>,
    /// Acting + crew credits, de-duplicated, newest first
    pub credits: Vec<Credit>,
}

#[derive(Debug, Clone)]
pub struct Credit {
    pub title: Title,
    /// "Cooper" / "Director"
    pub role: String,
    /// "Acting" / "Directing" / "Writing" …
    pub department: String,
}

// ------------------------------------------------------------------ raw

#[derive(Deserialize)]
struct RawGenres {
    #[serde(default)]
    genres: Vec<Genre>,
}

#[derive(Deserialize)]
struct RawProviders {
    #[serde(default)]
    results: Vec<Provider>,
}

#[derive(Deserialize)]
struct RawMulti {
    #[serde(default)]
    results: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawExtras {
    credits: Option<RawCredits>,
    videos: Option<RawVideos>,
    recommendations: Option<Page<RawTitle>>,
    similar: Option<Page<RawTitle>>,
    #[serde(rename = "watch/providers")]
    watch_providers: Option<RawWatch>,
    #[serde(default)]
    created_by: Vec<RawCreator>,
    keywords: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawCreator {
    id: i32,
    name: String,
    profile_path: Option<String>,
}

#[derive(Deserialize)]
struct RawCredits {
    #[serde(default)]
    cast: Vec<RawCastCredit>,
    #[serde(default)]
    crew: Vec<RawCastCredit>,
}

#[derive(Deserialize)]
struct RawCastCredit {
    id: i32,
    name: String,
    character: Option<String>,
    job: Option<String>,
    profile_path: Option<String>,
}

#[derive(Deserialize)]
struct RawVideos {
    #[serde(default)]
    results: Vec<RawVideo>,
}

#[derive(Deserialize)]
struct RawVideo {
    name: String,
    key: String,
    site: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    official: bool,
}

#[derive(Deserialize)]
struct RawWatch {
    #[serde(default)]
    results: std::collections::HashMap<String, RawWatchRegion>,
}

#[derive(Deserialize)]
struct RawWatchRegion {
    #[serde(default)]
    flatrate: Vec<Provider>,
}

#[derive(Deserialize)]
struct RawPerson {
    id: i32,
    name: String,
    biography: Option<String>,
    birthday: Option<String>,
    deathday: Option<String>,
    place_of_birth: Option<String>,
    profile_path: Option<String>,
    known_for_department: Option<String>,
    imdb_id: Option<String>,
    combined_credits: Option<RawCombined>,
}

#[derive(Deserialize)]
struct RawCombined {
    #[serde(default)]
    cast: Vec<serde_json::Value>,
    #[serde(default)]
    crew: Vec<serde_json::Value>,
}

fn person_card(v: &serde_json::Value) -> Option<PersonCard> {
    Some(PersonCard {
        id: v.get("id")?.as_i64()? as i32,
        name: v.get("name")?.as_str()?.to_string(),
        profile_path: v.get("profile_path").and_then(|p| p.as_str()).map(str::to_string),
        department: v.get("known_for_department").and_then(|p| p.as_str()).map(str::to_string),
        known_for: v
            .get("known_for")
            .and_then(|k| k.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.get("title").or(t.get("name")).and_then(|n| n.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn title_of(v: serde_json::Value) -> Option<Title> {
    serde_json::from_value::<RawTitle>(v).ok()?.into_title(None)
}

impl TmdbClient {
    pub async fn genres(&self, kind: Kind) -> Result<Vec<Genre>> {
        let r: RawGenres = self.get_json(&format!("/genre/{}/list", kind.as_str()), &[]).await?;
        Ok(r.genres)
    }

    /// Streaming services available in `region`, most relevant first
    pub async fn providers(&self, kind: Kind, region: &str) -> Result<Vec<Provider>> {
        let r: RawProviders = self.get_json(&format!("/watch/providers/{}", kind.as_str()), &[("watch_region", region)]).await?;
        let mut v = r.results;
        v.sort_by_key(|p| p.display_priority.unwrap_or(i32::MAX));
        Ok(v)
    }

    pub async fn list(&self, kind: Kind, list: ListKind, page: u32) -> Result<ResultPage> {
        let page_s = page.max(1).to_string();
        let r: RawPage = self.get_json(&list.path(kind), &[("page", page_s.as_str())]).await?;
        Ok(to_page(r, Some(kind)))
    }

    pub async fn discover(&self, q: &DiscoverQuery) -> Result<ResultPage> {
        let params = q.params();
        let p: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let r: RawPage = self.get_json(&format!("/discover/{}", q.kind.as_str()), &p).await?;
        Ok(to_page(r, Some(q.kind)))
    }

    /// Movies, shows and people for a query (as-you-type search)
    pub async fn search_all(&self, query: &str) -> Result<SearchResults> {
        let r: RawMulti = self.get_json("/search/multi", &[("query", query), ("include_adult", "false")]).await?;
        let mut out = SearchResults::default();
        for v in r.results {
            match v.get("media_type").and_then(|m| m.as_str()) {
                Some("person") => out.people.extend(person_card(&v)),
                Some("movie") | Some("tv") => out.titles.extend(title_of(v)),
                _ => {}
            }
        }
        Ok(out)
    }

    pub async fn search_people(&self, query: &str) -> Result<Vec<PersonCard>> {
        let r: RawMulti = self.get_json("/search/person", &[("query", query), ("include_adult", "false")]).await?;
        Ok(r.results.iter().filter_map(person_card).collect())
    }

    /// Cast, crew, trailers, similar & recommended titles, providers for `region`
    pub async fn extras(&self, kind: Kind, tmdb_id: i32, region: &str) -> Result<Extras> {
        let r: RawExtras = self
            .get_json(
                &format!("/{}/{tmdb_id}", kind.as_str()),
                &[("append_to_response", "credits,videos,recommendations,similar,watch/providers,keywords")],
            )
            .await?;
        let credits = r.credits.unwrap_or(RawCredits { cast: vec![], crew: vec![] });
        let cast = credits
            .cast
            .into_iter()
            .take(20)
            .map(|c| CastMember { id: c.id, name: c.name, role: c.character.unwrap_or_default(), profile_path: c.profile_path })
            .collect();
        let mut crew: Vec<CastMember> = credits
            .crew
            .into_iter()
            .filter(|c| matches!(c.job.as_deref(), Some("Director") | Some("Screenplay") | Some("Writer") | Some("Original Music Composer")))
            .map(|c| CastMember { id: c.id, name: c.name, role: c.job.unwrap_or_default(), profile_path: c.profile_path })
            .collect();
        crew.sort_by_key(|c| match c.role.as_str() {
            "Director" => 0,
            "Screenplay" | "Writer" => 1,
            _ => 2,
        });
        let mut seen = std::collections::HashSet::new();
        crew.retain(|c| seen.insert((c.id, c.role.clone())));
        let mut creators: Vec<CastMember> = r
            .created_by
            .into_iter()
            .map(|c| CastMember { id: c.id, name: c.name, role: "Creator".into(), profile_path: c.profile_path })
            .collect();
        creators.extend(crew);
        let mut trailers: Vec<Video> = r
            .videos
            .map(|v| v.results)
            .unwrap_or_default()
            .into_iter()
            .filter(|v| matches!(v.kind.as_str(), "Trailer" | "Teaser"))
            .map(|v| Video { name: v.name, key: v.key, site: v.site, kind: v.kind, official: v.official })
            .collect();
        trailers.sort_by_key(|v| (v.kind != "Trailer", !v.official));
        let titles = |p: Option<Page<RawTitle>>| -> Vec<Title> {
            p.map(|p| p.results).unwrap_or_default().into_iter().filter_map(|t| t.into_title(Some(kind))).collect()
        };
        let providers = r
            .watch_providers
            .and_then(|mut w| w.results.remove(region))
            .map(|w| w.flatrate)
            .unwrap_or_default();
        let keywords = r
            .keywords
            .and_then(|k| k.get("keywords").or(k.get("results")).and_then(|a| a.as_array()).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|k| k.get("name").and_then(|n| n.as_str()).map(str::to_string))
            .take(12)
            .collect();
        Ok(Extras {
            cast,
            crew: creators,
            trailers,
            recommendations: titles(r.recommendations),
            similar: titles(r.similar),
            providers,
            keywords,
        })
    }

    pub async fn person(&self, id: i32) -> Result<Person> {
        let r: RawPerson =
            self.get_json(&format!("/person/{id}"), &[("append_to_response", "combined_credits")]).await?;
        let mut credits: Vec<Credit> = vec![];
        let mut seen = std::collections::HashMap::<(Kind, i32), usize>::new();
        let combined = r.combined_credits.unwrap_or(RawCombined { cast: vec![], crew: vec![] });
        let push = |v: serde_json::Value, role: String, department: String, credits: &mut Vec<Credit>, seen: &mut std::collections::HashMap<(Kind, i32), usize>| {
            // talk shows & award ceremonies drown out the real filmography
            let genre_ids: Vec<i64> = v.get("genre_ids").and_then(|g| g.as_array()).map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
            if genre_ids.iter().any(|g| matches!(g, 10767 | 10763)) {
                return;
            }
            let Some(title) = title_of(v) else { return };
            let key = (title.kind, title.tmdb_id);
            if let Some(&i) = seen.get(&key) {
                if !role.is_empty() && !credits[i].role.contains(&role) {
                    credits[i].role = format!("{} · {}", credits[i].role, role).trim_matches(|c| c == ' ' || c == '·').to_string();
                }
                return;
            }
            seen.insert(key, credits.len());
            credits.push(Credit { title, role, department });
        };
        for v in combined.cast {
            let role = v.get("character").and_then(|c| c.as_str()).unwrap_or_default().to_string();
            push(v, role, "Acting".into(), &mut credits, &mut seen);
        }
        for v in combined.crew {
            let role = v.get("job").and_then(|c| c.as_str()).unwrap_or_default().to_string();
            let dept = v.get("department").and_then(|c| c.as_str()).unwrap_or("Crew").to_string();
            push(v, role, dept, &mut credits, &mut seen);
        }
        credits.sort_by(|a, b| b.title.year.unwrap_or(9999).cmp(&a.title.year.unwrap_or(9999)));
        Ok(Person {
            id: r.id,
            name: r.name,
            biography: r.biography.filter(|b| !b.is_empty()),
            birthday: r.birthday,
            deathday: r.deathday,
            place_of_birth: r.place_of_birth,
            profile_path: r.profile_path,
            department: r.known_for_department,
            imdb_id: r.imdb_id,
            credits,
        })
    }
}

fn to_page(r: RawPage, kind: Option<Kind>) -> ResultPage {
    ResultPage {
        results: r.results.into_iter().filter_map(|t| t.into_title(kind)).collect(),
        page: r.page,
        total_pages: r.total_pages,
        total_results: r.total_results,
    }
}

// ------------------------------------------------------------ calendar

/// Regional release of a movie (TMDB release types: 1 premiere, 2 limited,
/// 3 theatrical, 4 digital, 5 physical, 6 TV)
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub kind: u8,
    /// YYYY-MM-DD
    pub date: String,
}

/// An episode airing on a date
#[derive(Debug, Clone)]
pub struct Airing {
    pub season: u32,
    pub episode: u32,
    pub name: String,
    /// YYYY-MM-DD
    pub date: String,
}

impl TmdbClient {
    /// Movies released in `region` between two dates. `types` like "2|3" (theatrical) or "4|5" (digital/physical)
    pub async fn movie_releases(&self, region: &str, from: &str, to: &str, types: &str, page: u32) -> Result<ResultPage> {
        let page = page.max(1).to_string();
        let r: RawPage = self
            .get_json(
                "/discover/movie",
                &[
                    ("region", region),
                    ("release_date.gte", from),
                    ("release_date.lte", to),
                    ("with_release_type", types),
                    ("sort_by", "popularity.desc"),
                    ("include_adult", "false"),
                    ("page", page.as_str()),
                ],
            )
            .await?;
        Ok(to_page(r, Some(Kind::Movie)))
    }

    /// Release dates of a movie in `region` (falls back to the US list)
    pub async fn release_dates(&self, movie_id: i32, region: &str) -> Result<Vec<Release>> {
        let v: serde_json::Value = self.get_json(&format!("/movie/{movie_id}/release_dates"), &[]).await?;
        let list = v.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
        let pick = |cc: &str| {
            list.iter()
                .find(|r| r.get("iso_3166_1").and_then(|c| c.as_str()) == Some(cc))
                .and_then(|r| r.get("release_dates").and_then(|d| d.as_array()).cloned())
        };
        let dates = pick(region).or_else(|| pick("US")).unwrap_or_default();
        let mut out: Vec<Release> = dates
            .iter()
            .filter_map(|d| {
                Some(Release {
                    kind: d.get("type")?.as_u64()? as u8,
                    date: d.get("release_date")?.as_str()?.get(..10)?.to_string(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.date.cmp(&b.date));
        out.dedup_by(|a, b| a.kind == b.kind);
        Ok(out)
    }

    /// Shows with episodes airing between two dates (most popular first)
    pub async fn shows_airing(&self, from: &str, to: &str, page: u32) -> Result<ResultPage> {
        let page = page.max(1).to_string();
        let r: RawPage = self
            .get_json(
                "/discover/tv",
                &[
                    ("air_date.gte", from),
                    ("air_date.lte", to),
                    ("sort_by", "popularity.desc"),
                    ("include_adult", "false"),
                    ("with_type", "2|4"),
                    ("page", page.as_str()),
                ],
            )
            .await?;
        Ok(to_page(r, Some(Kind::Tv)))
    }

    /// Episodes of a show airing in [from, to] (dates as YYYY-MM-DD)
    pub async fn episodes_airing(&self, show_id: i32, from: &str, to: &str) -> Result<Vec<Airing>> {
        // same request as `details` so it shares the cache
        let v: serde_json::Value = self.get_json(&format!("/tv/{show_id}"), &[("append_to_response", "external_ids")]).await?;
        let mut seasons: Vec<u32> = ["last_episode_to_air", "next_episode_to_air"]
            .iter()
            .filter_map(|k| v.get(*k).and_then(|e| e.get("season_number")).and_then(|s| s.as_u64()).map(|s| s as u32))
            .collect();
        seasons.sort();
        seasons.dedup();
        let mut out = vec![];
        for s in seasons {
            for e in self.season(show_id, s).await.unwrap_or_default() {
                if let Some(d) = e.air_date.as_deref().filter(|d| *d >= from && *d <= to) {
                    out.push(Airing { season: e.season, episode: e.number, name: e.name.clone(), date: d.to_string() });
                }
            }
        }
        Ok(out)
    }
}

impl TmdbClient {
    pub fn profile_url(path: &str) -> String {
        format!("https://image.tmdb.org/t/p/w185{path}")
    }
    pub fn logo_url(path: &str) -> String {
        format!("https://image.tmdb.org/t/p/w92{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_params() {
        let q = DiscoverQuery {
            kind: Kind::Tv,
            genres: vec![18, 9648],
            year_from: Some(2010),
            year_to: Some(2019),
            min_rating: Some(7.5),
            providers: vec![8, 1899],
            language: Some("ko".into()),
            sort: Sort::Rating,
            ..Default::default()
        };
        let p = q.params();
        let get = |k: &str| p.iter().find(|(kk, _)| *kk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("sort_by"), Some("vote_average.desc"));
        assert_eq!(get("with_genres"), Some("18,9648"));
        assert_eq!(get("first_air_date.gte"), Some("2010-01-01"));
        assert_eq!(get("first_air_date.lte"), Some("2019-12-31"));
        assert_eq!(get("with_watch_providers"), Some("8|1899"));
        assert_eq!(get("watch_region"), Some("CZ"));
        assert_eq!(get("with_original_language"), Some("ko"));
        assert_eq!(get("vote_count.gte"), Some("300"));
        assert!(q.is_filtered());
        assert!(!DiscoverQuery::default().is_filtered());
        assert_eq!(chrono_today().len(), 10);
    }

    #[test]
    fn parses_multi_search_people() {
        let v = serde_json::json!({"id": 525, "name": "Christopher Nolan", "known_for_department": "Directing",
            "known_for": [{"title": "Inception"}, {"name": "Westworld"}]});
        let p = person_card(&v).unwrap();
        assert_eq!(p.known_for, vec!["Inception", "Westworld"]);
        assert_eq!(p.department.as_deref(), Some("Directing"));
    }
}
