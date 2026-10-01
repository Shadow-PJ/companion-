//! Learn mode: after Claude changes your code, Glowby asks one short
//! multiple-choice question about what changed and why. Right answers give XP.
//!
//! The question is written by Claude's small model (Haiku) through `claude -p`,
//! with tools AND hooks switched off for that request, and it only sees the
//! edits of that turn. At most one question per interval (Settings).

use crate::sessions::{shorten, str_field};
use crate::settings;
use crate::state::{self, AppState, lock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::io::AsyncWriteExt;

pub const LEARN_XP: u32 = 15;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_EDIT_CHARS: usize = 5000;
const MIN_EDIT_CHARS: usize = 80;
const QUESTION_STAYS: Duration = Duration::from_secs(15 * 60);
const RESULT_STAYS: Duration = Duration::from_secs(12);

#[derive(Default)]
struct TurnLog {
    prompt: String,
    edits: String,
    project: String,
}

pub struct Quiz {
    pub question: String,
    pub options: Vec<String>,
    pub answer: usize,
    pub explain: String,
    pub project: String,
    pub chosen: Option<usize>,
    pub until: Instant,
}

#[derive(Default)]
pub struct Learn {
    turns: HashMap<String, TurnLog>,
    last_quiz: Option<Instant>,
    generating: bool,
    pub quiz: Option<Quiz>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuizView {
    pub question: String,
    pub options: Vec<String>,
    pub project: String,
    /// Filled in after you answer.
    pub result: Option<QuizResult>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuizResult {
    pub chosen: usize,
    pub correct: usize,
    pub right: bool,
    pub explain: String,
}

#[derive(Deserialize)]
struct RawQuiz {
    question: String,
    options: Vec<String>,
    answer: usize,
    #[serde(default)]
    explain: String,
}

impl Learn {
    pub fn view(&self, now: Instant) -> Option<QuizView> {
        let q = self.quiz.as_ref().filter(|q| q.until > now)?;
        Some(QuizView {
            question: q.question.clone(),
            options: q.options.clone(),
            project: q.project.clone(),
            result: q.chosen.map(|chosen| QuizResult { chosen, correct: q.answer, right: chosen == q.answer, explain: q.explain.clone() }),
        })
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.quiz.as_ref().map(|q| q.until)
    }

    pub fn expire(&mut self, now: Instant) {
        if self.quiz.as_ref().is_some_and(|q| q.until <= now) {
            self.quiz = None;
        }
    }
}

/// What a code edit looked like, compactly, for the question writer.
fn describe_edit(payload: &Value) -> Option<String> {
    let input = payload.get("tool_input")?;
    let file = input.get("file_path").or_else(|| input.get("notebook_path"))?.as_str()?;
    let field = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut out = format!("--- {file}\n");
    match payload.get("tool_name")?.as_str()? {
        "Edit" => out.push_str(&format!("- {}\n+ {}\n", shorten(&field(input, "old_string"), 700), shorten(&field(input, "new_string"), 700))),
        "MultiEdit" => {
            for e in input.get("edits")?.as_array()?.iter().take(4) {
                out.push_str(&format!("- {}\n+ {}\n", shorten(&field(e, "old_string"), 400), shorten(&field(e, "new_string"), 400)));
            }
        }
        "Write" => out.push_str(&format!("(new file content)\n+ {}\n", shorten(&field(input, "content"), 1200))),
        "NotebookEdit" => out.push_str(&format!("+ {}\n", shorten(&field(input, "new_source"), 900))),
        _ => return None,
    }
    Some(out)
}

/// Pulls the JSON object out of the model's reply (it may wrap it in ``` fences)
/// and checks it makes sense.
pub fn parse_quiz(text: &str) -> Option<(String, Vec<String>, usize, String)> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let raw: RawQuiz = serde_json::from_str(text.get(start..=end)?).ok()?;
    let options: Vec<String> = raw.options.into_iter().map(|o| shorten(o.trim(), 120)).filter(|o| !o.is_empty()).collect();
    let ok = !raw.question.trim().is_empty() && (2..=4).contains(&options.len()) && raw.answer < options.len();
    ok.then(|| (shorten(raw.question.trim(), 220), options, raw.answer, shorten(raw.explain.trim(), 260)))
}

/// Shuffles the options so the right answer isn't always first (models like
/// putting it first). Returns the new options and where the answer moved.
fn shuffle(options: Vec<String>, answer: usize, seed: u64) -> (Vec<String>, usize) {
    let mut order: Vec<usize> = (0..options.len()).collect();
    let mut s = seed | 1;
    for i in (1..order.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        order.swap(i, (s % (i as u64 + 1)) as usize);
    }
    let new_answer = order.iter().position(|&i| i == answer).unwrap_or(0);
    (order.into_iter().map(|i| options[i].clone()).collect(), new_answer)
}

/// Collects the turn's edits; after the turn, maybe writes a question.
pub fn on_event(app: &AppHandle, event: &str, p: &Value) {
    let state = app.state::<AppState>();
    let session = str_field(p, "session_id").unwrap_or("").to_string();
    match event {
        "UserPromptSubmit" => {
            let prompt = str_field(p, "prompt").or_else(|| str_field(p, "prompt_text")).unwrap_or("");
            lock(&state.learn).turns.insert(
                session,
                TurnLog { prompt: shorten(prompt, 500), edits: String::new(), project: crate::sessions::project_name(str_field(p, "cwd").unwrap_or("")) },
            );
        }
        "PostToolUse" => {
            if let Some(edit) = describe_edit(p) {
                let mut learn = lock(&state.learn);
                let turn = learn.turns.entry(session).or_default();
                if turn.edits.len() < MAX_EDIT_CHARS {
                    turn.edits.push_str(&edit);
                }
            }
        }
        "Stop" => {
            let turn = lock(&state.learn).turns.remove(&session);
            if let Some(turn) = turn {
                maybe_ask(app, turn, str_field(p, "last_assistant_message").unwrap_or("").to_string());
            }
        }
        "SessionEnd" => {
            lock(&state.learn).turns.remove(&session);
        }
        _ => {}
    }
}

fn maybe_ask(app: &AppHandle, turn: TurnLog, summary: String) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    if !settings.learn.enabled || turn.edits.len() < MIN_EDIT_CHARS || lock(&state.ui).game_active {
        return;
    }
    {
        let mut learn = lock(&state.learn);
        let cooldown = Duration::from_secs(settings.learn.every_mins as u64 * 60);
        let recent = learn.last_quiz.is_some_and(|t| t.elapsed() < cooldown);
        if recent || learn.generating || learn.quiz.is_some() {
            return;
        }
        learn.generating = true;
        learn.last_quiz = Some(Instant::now());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = generate(&app, &turn, &summary).await;
        let state = app.state::<AppState>();
        let mut learn = lock(&state.learn);
        learn.generating = false;
        match result {
            Ok((question, options, answer, explain)) => {
                let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
                let (options, answer) = shuffle(options, answer, seed);
                learn.quiz = Some(Quiz { question, options, answer, explain, project: turn.project, chosen: None, until: Instant::now() + QUESTION_STAYS });
                drop(learn);
                state::hold_out(&app, 60);
                crate::sounds::play(&app, crate::sounds::Sound::Notice);
                crate::pet_window::show(&app);
                state::publish(&app);
            }
            Err(e) => {
                drop(learn);
                crate::applog::line(format!("learn mode: couldn't write a question ({e})"));
            }
        }
    });
}

async fn generate(app: &AppHandle, turn: &TurnLog, summary: &str) -> Result<(String, Vec<String>, usize, String), String> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let claude = crate::chat::find_claude(&settings.chat.claude_path).ok_or("claude.exe not found")?;
    let no_hooks = state.paths.config_dir.join("learn-settings.json");
    settings::save_json(&no_hooks, &serde_json::json!({ "disableAllHooks": true })).map_err(|e| e.to_string())?;
    let prompt = format!(
        "You are Glowby, a friendly coding companion. A student developer just watched an AI assistant change their code. \
Write ONE short multiple-choice question that checks whether they understood WHAT changed or WHY. \
Rules: the question has at most 25 words; exactly 3 short options; only one is correct; the others are plausible; \
base it only on the change below; no trick questions.\n\
Reply with ONLY a JSON object: {{\"question\": \"...\", \"options\": [\"...\", \"...\", \"...\"], \"answer\": <index of the correct option>, \"explain\": \"<one short sentence>\"}}\n\n\
What they asked: {}\n\nWhat the assistant said it did: {}\n\nThe edits:\n{}",
        turn.prompt,
        shorten(summary, 600),
        turn.edits
    );
    let mut cmd = tokio::process::Command::new(&claude);
    cmd.args(["-p", "--output-format", "json", "--model", "haiku", "--tools", "", "--no-session-persistence", "--settings"])
        .arg(&no_hooks)
        .current_dir(&state.paths.config_dir) // no project files or CLAUDE.md needed
        .env_remove("ANTHROPIC_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| e.to_string())?;
    }
    let out = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
        .await
        .map_err(|_| "timed out".to_string())?
        .map_err(|e| e.to_string())?;
    let reply: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("bad output: {e}"))?;
    if reply.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
        return Err(reply.get("result").and_then(Value::as_str).unwrap_or("error").to_string());
    }
    let text = reply.get("result").and_then(Value::as_str).ok_or("no result")?;
    parse_quiz(text).ok_or_else(|| "the reply wasn't a valid question".to_string())
}

/// You picked an option.
pub fn answer(app: &AppHandle, index: usize) {
    let state = app.state::<AppState>();
    let right = {
        let mut learn = lock(&state.learn);
        let Some(q) = learn.quiz.as_mut() else { return };
        if q.chosen.is_some() || index >= q.options.len() {
            return;
        }
        q.chosen = Some(index);
        q.until = Instant::now() + RESULT_STAYS;
        index == q.answer
    };
    state::hold_out(app, RESULT_STAYS.as_secs());
    if right {
        crate::progress::award(app, LEARN_XP, "learned");
        crate::quests::progress(app, "learn", 1);
    }
    state::publish(app);
}

pub fn skip(app: &AppHandle) {
    lock(&app.state::<AppState>().learn).quiz = None;
    state::publish(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_fenced_json_replies() {
        let reply = "```json\n{\"question\": \"What did add() return before?\", \"options\": [\"a - b\", \"a + b\", \"a * b\"], \"answer\": 0, \"explain\": \"It subtracted.\"}\n```";
        let (q, options, answer, explain) = parse_quiz(reply).unwrap();
        assert!(q.contains("add()"));
        assert_eq!(options.len(), 3);
        assert_eq!(answer, 0);
        assert_eq!(explain, "It subtracted.");
    }

    #[test]
    fn rejects_broken_questions() {
        assert!(parse_quiz("no json here").is_none());
        assert!(parse_quiz(r#"{"question": "Q?", "options": ["only one"], "answer": 0}"#).is_none());
        assert!(parse_quiz(r#"{"question": "Q?", "options": ["a", "b"], "answer": 5}"#).is_none());
    }

    #[test]
    fn shuffle_keeps_the_right_answer() {
        let opts = vec!["right".to_string(), "wrong 1".into(), "wrong 2".into()];
        for seed in 1..50u64 {
            let (shuffled, answer) = shuffle(opts.clone(), 0, seed.wrapping_mul(2654435761));
            assert_eq!(shuffled[answer], "right");
            assert_eq!(shuffled.len(), 3);
        }
    }

    #[test]
    fn describes_edits_compactly() {
        let p = json!({ "tool_name": "Edit", "tool_input": { "file_path": "src/math.rs", "old_string": "a - b", "new_string": "a + b" } });
        let d = describe_edit(&p).unwrap();
        assert!(d.contains("src/math.rs") && d.contains("- a - b") && d.contains("+ a + b"));
        assert!(describe_edit(&json!({ "tool_name": "Read", "tool_input": { "file_path": "x" } })).is_none());
    }
}
