//! AI Pulse. Public news + measured benchmarks, cached on this PC.
//! No project files, conversations, search terms or account tokens are uploaded.
//! A window costs memory only while open; background refresh is opt-in, at most hourly.

use crate::settings;
use crate::state::{AppState, lock};
use chrono::{DateTime, NaiveDate, Utc};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::Notify;

pub const LABEL: &str = "pulse";
const MAX_BODY: u64 = 5 * 1024 * 1024;
const BENCHMARK_URL: &str = "https://artificialanalysis.ai/leaderboards/models";

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Article {
    pub id: String,
    pub title: String,
    pub company: String,
    pub source: String,
    pub url: String,
    pub published: String,
    pub summary: String,
    pub why: String,
    pub tags: Vec<String>,
    pub impact: String,
    pub kind: String,
    pub checked: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub name: String,
    pub company: String,
    pub best_for: String,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub context: Option<u64>,
    pub url: String,
    pub checked: String,
    pub released: String,
    pub tags: Vec<String>,
    pub note: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub company: String,
    pub best_for: String,
    pub description: String,
    pub price: String,
    pub free_tier: bool,
    pub categories: Vec<String>,
    pub models: Vec<String>,
    pub similar: Vec<String>,
    pub url: String,
    pub pricing_url: String,
    pub checked: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Benchmark {
    pub name: String,
    pub company: String,
    pub context: String,
    pub intelligence: Option<f64>,
    pub cost: Option<f64>,
    pub speed: Option<f64>,
    pub latency: Option<f64>,
    pub rank: usize,
    pub movement: Option<i64>,
    pub checked: String,
    pub url: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct SourceStatus {
    pub name: String,
    pub url: String,
    pub checked: String,
    pub ok: bool,
    pub error: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Cache {
    pub articles: Vec<Article>,
    pub models: Vec<Model>,
    pub tools: Vec<Tool>,
    pub benchmarks: Vec<Benchmark>,
    pub sources: Vec<SourceStatus>,
    pub last_checked: String,
    pub changes: Vec<Article>,
    pub last_attempt: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub enabled: bool,
    pub background: bool,
    pub alerts: bool,
    pub daily: bool,
    pub every_hours: u32,
    pub companies: Vec<String>,
    pub families: Vec<String>,
    pub topics: Vec<String>,
    pub saved: Vec<String>,
    pub read: Vec<String>,
    pub theme: String,
    pub last_briefing_day: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: true,
            background: false,
            alerts: true,
            daily: true,
            every_hours: 6,
            companies: vec!["OpenAI".into(), "Anthropic".into(), "Google".into()],
            families: vec![],
            topics: vec!["Models".into(), "Coding".into(), "Agents".into()],
            saved: vec![],
            read: vec![],
            theme: "system".into(),
            last_briefing_day: String::new(),
        }
    }
}

impl Preferences {
    fn clean(mut self) -> Self {
        self.every_hours = self.every_hours.clamp(1, 24);
        if !["system", "dark", "light"].contains(&self.theme.as_str()) {
            self.theme = "system".into();
        }
        for list in [
            &mut self.companies,
            &mut self.families,
            &mut self.topics,
            &mut self.saved,
            &mut self.read,
        ] {
            list.retain(|s| s.len() <= 300);
            list.sort();
            list.dedup();
            list.truncate(500);
        }
        self
    }
}

pub struct Pulse {
    pub cache: Mutex<Cache>,
    pub preferences: Mutex<Preferences>,
    pub fetching: AtomicBool,
    cache_file: PathBuf,
    preferences_file: PathBuf,
    wake: Notify,
}

impl Pulse {
    pub fn new(dir: PathBuf) -> Self {
        let cache_file = dir.join("pulse-cache.json");
        let preferences_file = dir.join("pulse-preferences.json");
        let mut cached: Cache = settings::load_json(&cache_file);
        let seed = catalog();
        // Catalog corrections ship with the app; cached news and saved ids survive upgrades.
        cached.models = seed.models;
        cached.tools = seed.tools;
        if cached.articles.is_empty() {
            cached.articles = seed.articles;
        }
        Self {
            cache: Mutex::new(cached),
            preferences: Mutex::new(settings::load_json::<Preferences>(&preferences_file).clean()),
            fetching: AtomicBool::new(false),
            cache_file,
            preferences_file,
            wake: Notify::new(),
        }
    }
}

fn catalog() -> Cache {
    serde_json::from_str(include_str!("../../src/pulse/catalog.json")).expect("bundled AI catalog")
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub cache: Cache,
    pub preferences: Preferences,
    pub refreshing: bool,
}

#[tauri::command]
pub fn pulse_view(app: AppHandle) -> View {
    view(&app)
}

fn view(app: &AppHandle) -> View {
    let p = app.state::<Pulse>();
    View {
        cache: lock(&p.cache).clone(),
        preferences: lock(&p.preferences).clone(),
        refreshing: p.fetching.load(Ordering::Relaxed),
    }
}

fn publish(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "pulse://view", view(app));
}

#[tauri::command]
pub fn pulse_preferences(app: AppHandle, patch: serde_json::Value) -> Result<Preferences, String> {
    let p = app.state::<Pulse>();
    // Merge under the same lock so the pet's settings cannot erase hub bookmarks.
    let mut current = lock(&p.preferences);
    let preferences = merge_preferences(&current, patch)?;
    // Persist first; an IO failure must not falsely look like a saved preference.
    settings::save_json(&p.preferences_file, &preferences)
        .map_err(|e| format!("Couldn't save AI Pulse settings: {e}"))?;
    *current = preferences.clone();
    drop(current);
    p.wake.notify_one();
    publish(&app);
    Ok(preferences)
}

fn merge_preferences(
    current: &Preferences,
    patch: serde_json::Value,
) -> Result<Preferences, String> {
    let Some(patch) = patch.as_object() else {
        return Err("Expected AI Pulse preference fields.".into());
    };
    let mut merged = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let fields = merged
        .as_object_mut()
        .ok_or("Invalid AI Pulse preferences")?;
    for (key, value) in patch {
        if fields.contains_key(key) && key != "lastBriefingDay" {
            fields.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value::<Preferences>(merged)
        .map(Preferences::clean)
        .map_err(|e| format!("Invalid AI Pulse preference: {e}"))
}

#[tauri::command]
pub async fn pulse_refresh(app: AppHandle) -> Result<View, String> {
    refresh(&app, true).await?;
    Ok(view(&app))
}

#[tauri::command]
pub fn pulse_open(app: AppHandle) -> Result<(), String> {
    open_window(&app)
}

pub fn open_window(app: &AppHandle) -> Result<(), String> {
    if !lock(&app.state::<Pulse>().preferences).enabled {
        return Err("Enable AI Pulse in Glowby Settings first.".into());
    }
    if app.state::<AppState>().settings().game_mode && crate::gamemode::fullscreen_app_running() {
        return Err("AI Pulse will stay closed while a fullscreen app is running.".into());
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App("pulse.html".into()))
            .title("Glowby · AI Pulse")
            .inner_size(1280.0, 840.0)
            .min_inner_size(620.0, 480.0)
            .center()
            .additional_browser_args(crate::pet_window::BROWSER_ARGS)
            .build()
        {
            crate::applog::line(format!("AI Pulse window: {e}"));
        }
    });
    Ok(())
}

/// Open only checked https URLs already present in the catalog or fetched feed.
#[tauri::command]
pub fn pulse_open_source(app: AppHandle, url: String) -> Result<(), String> {
    let p = app.state::<Pulse>();
    let c = lock(&p.cache);
    let known = c.articles.iter().chain(&c.changes).any(|x| x.url == url)
        || c.models.iter().any(|x| x.url == url)
        || c.tools.iter().any(|x| x.url == url || x.pricing_url == url)
        || c.sources.iter().any(|x| x.url == url)
        || url == BENCHMARK_URL
        || url == "https://artificialanalysis.ai/methodology/intelligence-benchmarking";
    if !known || !safe_url(&url) {
        return Err("That source isn't in the AI Pulse catalog.".into());
    }
    drop(c);
    std::process::Command::new("explorer.exe")
        .arg(url)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// Source addresses are fixed here; users cannot turn the fetcher into a proxy.
struct Feed {
    name: &'static str,
    company: &'static str,
    url: &'static str,
    kind: &'static str,
}
const FEEDS: &[Feed] = &[
    Feed {
        name: "OpenAI News",
        company: "OpenAI",
        url: "https://openai.com/news/rss.xml",
        kind: "rss",
    },
    Feed {
        name: "Anthropic News",
        company: "Anthropic",
        url: "https://www.anthropic.com/news",
        kind: "anthropic",
    },
    Feed {
        name: "Google AI",
        company: "Google",
        url: "https://blog.google/technology/ai/rss/",
        kind: "rss",
    },
    Feed {
        name: "Codex releases",
        company: "OpenAI",
        url: "https://github.com/openai/codex/releases.atom",
        kind: "atom",
    },
    Feed {
        name: "Claude Code releases",
        company: "Anthropic",
        url: "https://github.com/anthropics/claude-code/releases.atom",
        kind: "atom",
    },
    Feed {
        name: "Hugging Face",
        company: "Hugging Face",
        url: "https://huggingface.co/blog/feed.xml",
        kind: "rss",
    },
    Feed {
        name: "Meta AI",
        company: "Meta",
        url: "https://about.fb.com/news/tag/ai/feed/",
        kind: "rss",
    },
];

fn fetch(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_global(Some(Duration::from_secs(18)))
        .build()
        .into();
    agent
        .get(url)
        .header("User-Agent", "Glowby/0.6 (personal AI news reader)")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .map_err(|e| e.to_string())
}

fn safe_url(url: &str) -> bool {
    // Strict protocol, no shell controls, userinfo, ports, or local/IP hosts.
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !url.chars()
        .any(|c| c.is_control() || ['"', '\\', '<', '>'].contains(&c))
        && host.contains('.')
        && !host.contains(['@', ':'])
        && host.chars().any(|c| c.is_alphabetic())
        && host != "localhost"
        && !host.ends_with(".local")
}

fn id_for(url: &str) -> String {
    // Stable across releases (unlike DefaultHasher); excludes tracking parameters.
    let url = url.split('?').next().unwrap_or(url).trim_end_matches('/');
    let hash = url.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    });
    format!("news-{hash:016x}")
}

fn plain(html: &str, max_words: usize) -> String {
    Html::parse_fragment(html)
        .root_element()
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .take(max_words)
        .collect::<Vec<_>>()
        .join(" ")
}

fn tags_for(text: &str) -> Vec<String> {
    let s = text.to_lowercase();
    let mut tags = vec![];
    for (tag, words) in [
        (
            "Coding",
            &["codex", "code", "coding", "developer", "software", "cursor"][..],
        ),
        ("Agents", &["agent", "computer use", "automation"]),
        (
            "Models",
            &["model", "gpt", "claude", "gemini", "llama", "grok"],
        ),
        ("Image", &["image", "imagen", "diffusion", "visual"]),
        ("Video", &["video", "veo", "sora"]),
        ("Audio", &["audio", "voice", "music", "speech"]),
        ("Research", &["research", "science", "benchmark", "study"]),
        (
            "Business",
            &["enterprise", "business", "partnership", "pricing", "price"],
        ),
    ] {
        if words.iter().any(|w| s.contains(w)) {
            tags.push(tag.into());
        }
    }
    if tags.is_empty() {
        tags.push("Business".into());
    }
    tags
}

fn article(
    feed: &Feed,
    title: String,
    url: String,
    published: String,
    summary: String,
) -> Option<Article> {
    if title.trim().is_empty() || !safe_url(&url) {
        return None;
    }
    let date = DateTime::parse_from_rfc3339(&published).ok()?;
    if date.timestamp() > Utc::now().timestamp() + 86400 {
        return None;
    }
    let tags = tags_for(&format!("{title} {summary} {}", feed.name));
    let lower = title.to_lowercase();
    let model = tags.iter().any(|t| t == "Models")
        && ["introduc", "releas", "launch", "new model", "announc"]
            .iter()
            .any(|w| lower.contains(w));
    let pricing = ["price", "pricing", "cheaper", "cost reduc"]
        .iter()
        .any(|w| lower.contains(w));
    let impact = if model || pricing { "High" } else { "Update" };
    let why = if pricing {
        "Recheck your task budget and the provider's current pricing before changing plans."
    } else if tags.iter().any(|t| t == "Coding") {
        "Check whether this changes your coding workflow, supported tools or agent setup."
    } else if model {
        "Try this on a task you already know; compare quality, speed and cost before switching."
    } else if tags.iter().any(|t| t == "Image" || t == "Video") {
        "Check access, output quality and usage rights before adding it to a creative workflow."
    } else {
        "Read the original announcement to decide whether this is useful for your work."
    };
    Some(Article {
        id: id_for(&url),
        title,
        url,
        company: feed.company.into(),
        source: feed.name.into(),
        published,
        summary,
        why: why.into(),
        tags,
        impact: impact.into(),
        kind: if pricing {
            "pricing"
        } else if model {
            "model"
        } else if feed.kind == "atom" {
            "release"
        } else {
            "news"
        }
        .into(),
        checked: Utc::now().to_rfc3339(),
    })
}

fn parse_feed(text: &str, feed: &Feed) -> Result<Vec<Article>, String> {
    let doc = roxmltree::Document::parse(text)
        .map_err(|_| "Feed format changed or invalid XML".to_string())?;
    let mut articles = vec![];
    for item in doc
        .descendants()
        .filter(|n| n.is_element() && ["item", "entry"].contains(&n.tag_name().name()))
        .take(35)
    {
        let field = |name| {
            item.children()
                .find(|n| n.is_element() && n.tag_name().name() == name)
                .and_then(|n| n.text())
                .unwrap_or("")
        };
        let title = plain(field("title"), 22);
        let prerelease = feed.kind == "atom"
            && ["alpha", "beta", "-rc"]
                .iter()
                .any(|s| title.to_lowercase().contains(s));
        let url = if feed.kind == "atom" {
            item.children()
                .find(|n| {
                    n.is_element()
                        && n.tag_name().name() == "link"
                        && n.attribute("rel").unwrap_or("alternate") == "alternate"
                })
                .and_then(|n| n.attribute("href"))
                .unwrap_or("")
        } else {
            field("link")
        };
        let raw_date = if feed.kind == "atom" {
            field("updated")
        } else {
            field("pubDate")
        };
        let date = DateTime::parse_from_rfc3339(raw_date)
            .or_else(|_| DateTime::parse_from_rfc2822(raw_date))
            .ok()
            .map(|d| d.to_rfc3339());
        let summary = plain(
            if feed.kind == "atom" {
                field("content")
            } else {
                field("description")
            },
            22,
        );
        let title = if feed.kind == "atom" {
            format!("{} · {title}", feed.name.trim_end_matches(" releases"))
        } else {
            title
        };
        if let Some(mut a) = date.and_then(|d| article(feed, title, url.into(), d, summary)) {
            if prerelease {
                a.tags.push("Preview".into());
                a.impact = "Update".into();
                a.why = "This is a prerelease CLI build. Check its release notes before installing; it is not a stable product launch.".into();
            }
            articles.push(a);
        }
    }
    if articles.is_empty() {
        Err("No dated news entries found; keeping the previous cache".into())
    } else {
        Ok(articles)
    }
}

fn parse_anthropic(text: &str, feed: &Feed) -> Result<Vec<Article>, String> {
    let doc = Html::parse_document(text);
    let a = Selector::parse("a[href^='/news/']").unwrap();
    let time = Selector::parse("time").unwrap();
    let title = Selector::parse("[class*='title']").unwrap();
    let mut out = vec![];
    let mut seen = HashSet::new();
    for link in doc.select(&a) {
        let url = format!(
            "https://www.anthropic.com{}",
            link.value().attr("href").unwrap_or("")
        );
        if !seen.insert(url.clone()) {
            continue;
        }
        let Some(raw) = link
            .select(&time)
            .next()
            .map(|t| t.text().collect::<String>())
        else {
            continue;
        };
        let Some(day) = NaiveDate::parse_from_str(raw.trim(), "%b %e, %Y").ok() else {
            continue;
        };
        let heading = link
            .select(&title)
            .next()
            .map(|t| t.text().collect::<String>())
            .unwrap_or_default();
        let published = format!("{day}T00:00:00Z");
        if let Some(news) = article(feed, plain(&heading, 22), url, published, String::new()) {
            out.push(news);
        }
        if out.len() >= 30 {
            break;
        }
    }
    if out.is_empty() {
        Err("Newsroom format changed; keeping the previous cache".into())
    } else {
        Ok(out)
    }
}

fn number(s: &str) -> Option<f64> {
    let n = s.replace(['$', ','], "").trim().parse::<f64>().ok()?;
    (n.is_finite() && n >= 0.0).then_some(n)
}

fn parse_benchmarks(text: &str) -> Result<Vec<Benchmark>, String> {
    let doc = Html::parse_document(text);
    let tables = Selector::parse("table").unwrap();
    let tr = Selector::parse("tr").unwrap();
    let td = Selector::parse("td").unwrap();
    let th = Selector::parse("th").unwrap();
    let now = Utc::now().to_rfc3339();
    for table in doc.select(&tables) {
        let headers: Vec<String> = table
            .select(&th)
            .map(|x| x.text().collect::<Vec<_>>().join(" "))
            .collect();
        if !headers.iter().any(|h| h.contains("Intelligence Index"))
            || !headers.iter().any(|h| h.contains("Cost per Task"))
        {
            continue;
        }
        let expected = ["model", "context", "creator", "intelligence index", "cost per task", "tokens", "latency"];
        let Some(start) = headers.iter().position(|h| h.trim().eq_ignore_ascii_case("model")) else { continue; }; if headers.len() < start + expected.len() || !expected.iter().enumerate().all(|(i, key)| headers[start+i].to_lowercase().contains(key)) { continue; }
        let mut out = vec![];
        for row in table.select(&tr) {
            let cells: Vec<String> = row
                .select(&td)
                .map(|x| {
                    x.text()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            if cells.len() < 7 {
                continue;
            }
            let intelligence = number(&cells[3]);
            if cells[0].is_empty() || intelligence.is_none() {
                continue;
            }
            out.push(Benchmark {
                name: cells[0].clone(),
                company: cells[2].clone(),
                context: cells[1].clone(),
                intelligence,
                cost: number(&cells[4]),
                speed: number(&cells[5]),
                latency: number(&cells[6]),
                rank: out.len() + 1,
                checked: now.clone(),
                url: BENCHMARK_URL.into(),
                movement: None,
            });
        }
        if out.len() >= 3 {
            return Ok(out);
        }
    }
    Err("Benchmark table or methodology changed; keeping the previous snapshot".into())
}

struct Updates {
    news: Vec<Article>,
    benchmarks: Option<Vec<Benchmark>>,
    sources: Vec<SourceStatus>,
}
fn collect() -> Updates {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = FEEDS
            .iter()
            .map(|feed| {
                scope.spawn(move || {
                    let result = fetch(feed.url).and_then(|body| {
                        if feed.kind == "anthropic" {
                            parse_anthropic(&body, feed)
                        } else {
                            parse_feed(&body, feed)
                        }
                    });
                    let status = SourceStatus {
                        name: feed.name.into(),
                        url: feed.url.into(),
                        checked: Utc::now().to_rfc3339(),
                        ok: result.is_ok(),
                        error: result.as_ref().err().cloned().unwrap_or_default(),
                    };
                    (result.unwrap_or_default(), status)
                })
            })
            .collect();
        let benchmark_job =
            scope.spawn(|| fetch(BENCHMARK_URL).and_then(|body| parse_benchmarks(&body)));
        let mut news = vec![];
        let mut sources = vec![];
        for job in jobs {
            if let Ok((mut list, status)) = job.join() {
                news.append(&mut list);
                sources.push(status);
            }
        }
        let result = benchmark_job
            .join()
            .unwrap_or_else(|_| Err("Benchmark fetch failed".into()));
        sources.push(SourceStatus {
            name: "Artificial Analysis".into(),
            url: BENCHMARK_URL.into(),
            checked: Utc::now().to_rfc3339(),
            ok: result.is_ok(),
            error: result.as_ref().err().cloned().unwrap_or_default(),
        });
        Updates {
            news,
            benchmarks: result.ok(),
            sources,
        }
    })
}

/// Events only compare two successful snapshots. First fetch seeds a baseline.
fn benchmark_changes(previous: &[Benchmark], next: &mut [Benchmark]) -> Vec<Article> {
    if previous.is_empty() {
        return vec![];
    }
    let old: HashMap<&str, &Benchmark> = previous.iter().map(|b| (b.name.as_str(), b)).collect();
    let mut changes = vec![];
    for b in next {
        if let Some(before) = old.get(b.name.as_str()) {
            b.movement = Some(before.rank as i64 - b.rank as i64);
            let cheaper = match (before.cost, b.cost) {
                (Some(a), Some(c)) => a > 0.0 && c < a * 0.9,
                _ => false,
            };
            if (b.rank <= 5 && b.movement.unwrap_or(0) > 0) || cheaper {
                let title = if cheaper {
                    format!("{}: observed benchmark task cost fell", b.name)
                } else {
                    format!("{} moved up to #{}", b.name, b.rank)
                };
                changes.push(Article { id: format!("change-{}-{}", id_for(&b.name), b.checked), title, company: b.company.clone(), source: "Artificial Analysis".into(), url: b.url.clone(),
                    published: b.checked.clone(), checked: b.checked.clone(), summary: if cheaper { "A lower cost per benchmark task was observed. This is not an API price-change announcement.".into() } else { "Rank changed between Glowby's last two successful Intelligence Index snapshots.".into() },
                    why: "Reasoning settings and methodology affect results; compare the exact configuration before switching.".into(), tags: vec!["Models".into(), "Research".into()], impact: "Update".into(), kind: "benchmark".into() });
            }
        } else if b.rank <= 5 {
            changes.push(Article { id: format!("new-{}", id_for(&b.name)), title: format!("{} is new to the observed Top 5", b.name), company: b.company.clone(), source: "Artificial Analysis".into(), url: b.url.clone(), published: b.checked.clone(), checked: b.checked.clone(), summary: "This configuration was absent from Glowby's previous snapshot. That does not establish its release date.".into(), why: "Check the measured configuration and try it on your own task.".into(), tags: vec!["Models".into(), "Research".into()], impact: "Update".into(), kind: "benchmark".into() });
        }
    }
    changes
}

fn follows(a: &Article, p: &Preferences) -> bool {
    (p.companies.is_empty() || p.companies.contains(&a.company))
        && (p.topics.is_empty() || a.tags.iter().any(|t| p.topics.contains(t)))
        && (p.families.is_empty()
            || p.families
                .iter()
                .any(|f| a.title.to_lowercase().contains(&f.to_lowercase())))
}

async fn refresh(app: &AppHandle, manual: bool) -> Result<(), String> {
    let p = app.state::<Pulse>();
    if !lock(&p.preferences).enabled {
        return Err("AI Pulse is turned off.".into());
    }
    if p.fetching.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    // Rate limit manual refresh as well; one click cannot flood the sources.
    if Utc::now().timestamp() - lock(&p.cache).last_attempt < 5 * 60 {
        p.fetching.store(false, Ordering::Release);
        return Ok(());
    }
    lock(&p.cache).last_attempt = Utc::now().timestamp();
    publish(app);
    let result = tauri::async_runtime::spawn_blocking(collect).await;
    let Ok(mut updates) = result else {
        p.fetching.store(false, Ordering::Release);
        publish(app);
        return Err("Couldn't finish the news check.".into());
    };
    let mut prefs = lock(&p.preferences).clone();
    let mut notify = None;
    let mut save_error = None;
    {
        let mut cache = lock(&p.cache);
        let baseline = !cache.last_checked.is_empty();
        let old_ids: HashSet<String> = cache
            .articles
            .iter()
            .chain(&cache.changes)
            .map(|a| a.id.clone())
            .collect();
        if let Some(ref mut next) = updates.benchmarks {
            let changes = benchmark_changes(&cache.benchmarks, next);
            cache.changes.extend(changes);
            cache.changes.sort_by(|a, b| b.published.cmp(&a.published));
            cache.changes.truncate(50);
            cache.benchmarks = next.clone();
        }
        let recent = Utc::now().timestamp() - 3 * 86400;
        let new: Vec<_> = updates
            .news
            .iter()
            .chain(&cache.changes)
            .filter(|a| {
                !old_ids.contains(&a.id)
                    && follows(a, &prefs)
                    && (a.impact == "High" || a.kind == "benchmark")
                    && DateTime::parse_from_rfc3339(&a.published)
                        .is_ok_and(|d| d.timestamp() >= recent)
            })
            .collect();
        if baseline && !manual && prefs.alerts && !new.is_empty() {
            notify = Some((
                "AI Pulse · new on your watchlist".to_string(),
                format!(
                    "{}{}",
                    new[0].title,
                    if new.len() > 1 {
                        format!(" · +{} updates", new.len() - 1)
                    } else {
                        String::new()
                    }
                ),
            ));
        }
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        if !manual && prefs.daily && prefs.last_briefing_day != day && !updates.news.is_empty() {
            prefs.last_briefing_day = day;
            if notify.is_none() {
                notify = Some((
                    "Your AI briefing is ready".into(),
                    "Open AI Pulse from Glowby's menu for five things to catch up on.".into(),
                ));
            }
        }
        // Never erase articles from a failed source or saved material.
        let fresh_ids: HashSet<String> = updates.news.iter().map(|a| a.id.clone()).collect();
        cache.articles.retain(|a| !fresh_ids.contains(&a.id));
        cache.articles.append(&mut updates.news);
        cache.articles.sort_by(|a, b| b.published.cmp(&a.published));
        let mut ordinary = 0;
        cache.articles.retain(|a| {
            if prefs.saved.contains(&a.id) {
                true
            } else {
                ordinary += 1;
                ordinary <= 300
            }
        });
        cache.sources = updates.sources;
        if cache.sources.iter().any(|s| s.ok) {
            cache.last_checked = Utc::now().to_rfc3339();
        }
        if let Err(e) = settings::save_json(&p.cache_file, &*cache) {
            save_error = Some(e.to_string());
        }
    }
    // Merge only the bookkeeping field: don't overwrite preferences changed during IO.
    if !prefs.last_briefing_day.is_empty() {
        let mut current = lock(&p.preferences);
        current.last_briefing_day = prefs.last_briefing_day;
        let _ = settings::save_json(&p.preferences_file, &*current);
    }
    p.fetching.store(false, Ordering::Release);
    publish(app);
    if let Some((title, body)) = notify {
        let state = app.state::<AppState>();
        if !lock(&state.ui).game_active {
            if state.settings().alerts.windows_notifications {
                crate::notify::show(None, title.clone(), body);
            }
            // Low priority news never replaces a permission, a limit or detective alert.
            if lock(&state.ui).offer.is_none() && lock(&state.perms).is_empty() {
                crate::state::toast(app, "info", title, "AI Pulse".into(), 12);
                crate::sounds::play(app, crate::sounds::Sound::Notice);
                crate::pet_window::show(app);
                crate::state::publish(app);
            }
        }
    }
    if let Some(error) = save_error {
        return Err(format!(
            "News loaded, but the cache couldn't be saved: {error}"
        ));
    }
    Ok(())
}

/// One sleeping task. No fetches while disabled, in game mode, or opted out.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let p = app.state::<Pulse>();
            let prefs = lock(&p.preferences).clone();
            if !prefs.enabled || !prefs.background {
                p.wake.notified().await;
                continue;
            }
            let last = lock(&p.cache).last_attempt;
            let due =
                (last + prefs.every_hours as i64 * 3600 - Utc::now().timestamp()).max(0) as u64;
            tokio::select! { _ = p.wake.notified() => continue, _ = tokio::time::sleep(Duration::from_secs(due)) => {} }
            if lock(&app.state::<AppState>().ui).game_active {
                tokio::select! { _ = p.wake.notified() => {}, _ = tokio::time::sleep(Duration::from_secs(30 * 60)) => {} }
                continue;
            }
            let _ = refresh(&app, false).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preference_patch_keeps_bookmarks_and_other_window_fields() {
        let mut p = Preferences::default();
        p.saved.push("news-1".into());
        let q = merge_preferences(
            &p,
            serde_json::json!({"background":true,"everyHours":0,"lastBriefingDay":"fake"}),
        )
        .unwrap();
        assert!(q.background);
        assert_eq!(q.every_hours, 1);
        assert_eq!(q.saved, p.saved);
        assert!(q.last_briefing_day.is_empty());
        assert!(merge_preferences(&p, serde_json::json!({"enabled":"no"})).is_err());
    }
    #[test]
    fn prerelease_cli_feed_stays_visible_and_is_not_a_major_alert() {
        let feed = Feed {
            name: "Codex releases",
            company: "OpenAI",
            url: "https://github.com/openai/codex/releases.atom",
            kind: "atom",
        };
        let data = r#"<feed><entry><title>v1.0.0-alpha.1</title><link href="https://github.com/openai/codex/releases/tag/test"/><updated>2026-01-01T00:00:00Z</updated><content>Preview build</content></entry></feed>"#;
        let articles = parse_feed(data, &feed).unwrap();
        assert_eq!(articles[0].impact, "Update");
        assert!(articles[0].tags.contains(&"Preview".into()));
    }
    #[test]
    fn rss_and_atom_are_dated_safe_and_tolerant() {
        let rss = r#"<rss><channel><item><title>Introducing a new coding model</title><link>https://example.com/news</link><pubDate>Tue, 29 Sep 2026 12:00:00 +0000</pubDate><description><![CDATA[<b>Useful</b> &amp; fast]]></description></item><item><title>Broken</title><link>javascript:bad</link></item></channel></rss>"#;
        let list = parse_feed(rss, &FEEDS[0]).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].summary, "Useful & fast");
        assert_eq!(list[0].kind, "model");
        let atom = r#"<feed xmlns='http://www.w3.org/2005/Atom'><entry><title>1.2.3</title><link rel='alternate' href='https://github.com/openai/codex/releases/tag/1.2.3'/><updated>2026-09-29T12:00:00Z</updated><content type='html'>Fixed an issue</content></entry></feed>"#;
        assert_eq!(parse_feed(atom, &FEEDS[3]).unwrap()[0].kind, "release");
        assert!(parse_feed("<broken", &FEEDS[0]).is_err());
    }
    #[test]
    fn unsafe_links_and_tracking_are_handled() {
        for url in [
            "file:///c:/a",
            "https://user@host.com",
            "https://127.0.0.1/x",
            "https://a.com/\"x",
            "https://host.local/x",
        ] {
            assert!(!safe_url(url));
        }
        assert!(safe_url("https://developers.openai.com/api/docs/models"));
        assert_eq!(
            id_for("https://example.com/a?utm_source=foo"),
            id_for("https://example.com/a")
        );
    }
    #[test]
    fn benchmark_columns_and_change_baseline() {
        let html = "<table><thead><tr><th>Model</th><th>Context Window</th><th>Creator</th><th>Artificial Analysis Intelligence Index</th><th>Cost per Task</th><th>Median Tokens/s</th><th>Latency</th></tr></thead><tbody><tr><td>A (max)</td><td>1M</td><td>Provider</td><td>58</td><td>$1.20</td><td>--</td><td>12</td></tr><tr><td>B</td><td>1M</td><td>Provider</td><td>56</td><td>$2</td><td>80</td><td>2</td></tr><tr><td>C</td><td>1M</td><td>Other</td><td>54</td><td>$3</td><td>60</td><td>3</td></tr></tbody></table>";
        let mut b = parse_benchmarks(html).unwrap();
        assert_eq!(b[0].speed, None);
        assert_eq!(b[0].cost, Some(1.2));
        assert!(benchmark_changes(&[], &mut b).is_empty());
        let prev = b.clone();
        b[0].cost = Some(1.0);
        let c = benchmark_changes(&prev, &mut b);
        assert_eq!(c.len(), 1);
        assert!(c[0].summary.contains("not an API"));
        assert!(parse_benchmarks("<html>format changed</html>").is_err());
    }
    #[test]
    fn catalog_and_preferences_are_valid() {
        let c = catalog();
        assert!(c.tools.len() >= 15);
        assert!(c.models.len() >= 6);
        for t in c.tools {
            assert!(safe_url(&t.url));
            assert!(safe_url(&t.pricing_url));
        }
        let p = Preferences {
            every_hours: 0,
            theme: "oops".into(),
            saved: vec!["a".into(), "a".into()],
            ..Default::default()
        }
        .clean();
        assert_eq!(p.every_hours, 1);
        assert_eq!(p.theme, "system");
        assert_eq!(p.saved.len(), 1);
    }
    #[test]
    #[ignore = "contacts public news/benchmark sources; run explicitly"]
    fn live_sources() {
        let updates = collect();
        for s in &updates.sources {
            println!("{}: {} {}", s.name, s.ok, s.error);
        }
        assert!(updates.news.len() >= 10);
        assert!(updates.benchmarks.as_ref().is_some_and(|b| b.len() >= 3));
        if let Ok(path) = std::env::var("GLOWBY_PULSE_PREVIEW") {
            let mut c = catalog();
            c.articles = updates.news;
            c.benchmarks = updates.benchmarks.unwrap();
            c.sources = updates.sources;
            c.last_checked = Utc::now().to_rfc3339();
            settings::save_json(std::path::Path::new(&path), &c).unwrap();
        }
    }
}
