//! "Are my tests and builds passing?", worked out from Claude Code's tool results.
//!
//! Claude Code reports a shell command that exits with a non-zero code as a
//! `PostToolUseFailure` event (`error: "Exit code 1 …"`), and a successful one
//! as `PostToolUse` with `tool_response.stdout/stderr`. We only care about
//! commands that look like a test run or a build.

use crate::sessions::shorten;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum CheckKind {
    Tests,
    Build,
}

impl CheckKind {
    pub fn noun(self) -> &'static str {
        match self {
            CheckKind::Tests => "tests",
            CheckKind::Build => "build",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: CheckKind,
    pub command: String,
    pub project_dir: String,
    pub since: String,
}

/// Saved to health.json so "sick until they pass" survives a restart.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Health {
    pub failures: BTreeMap<String, Failure>,
}

pub struct Check {
    pub kind: CheckKind,
    pub command: String,
    pub passed: bool,
    pub output: Option<String>,
}

pub enum Change {
    NowFailing(Failure),
    NowPassing(CheckKind),
}

const TEST_COMMANDS: &[&str] = &[
    "cargo test", "cargo nextest", "npm test", "npm run test", "npm t", "pnpm test", "pnpm run test", "yarn test",
    "bun test", "deno test", "jest", "vitest", "mocha", "pytest", "python -m unittest", "go test", "dotnet test",
    "mvn test", "mvnw test", "gradle test", "gradlew test", "rspec", "phpunit", "ctest", "mix test", "flutter test",
    "swift test",
];
const BUILD_COMMANDS: &[&str] = &[
    "cargo build", "cargo check", "npm run build", "pnpm build", "pnpm run build", "yarn build", "bun run build",
    "tsc", "vite build", "next build", "go build", "dotnet build", "msbuild", "mvn package", "mvn compile",
    "mvn install", "gradle build", "gradlew build", "make", "cmake --build", "javac", "gcc", "g++", "clang", "ninja",
];
/// Commands that merely *mention* a tool aren't test runs.
const NOT_A_RUN: &[&str] = &["echo", "which", "where", "type", "cat", "ls", "dir", "man", "get-command", "grep", "rg"];

/// Splits a command into simple tokens: lowercase, no `./` or `.exe/.cmd/.bat`.
fn tokens(command: &str) -> Vec<String> {
    command
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '"' | '\''))
        .filter(|t| !t.is_empty())
        .map(|t| {
            let t = t.trim_start_matches("./").trim_start_matches(".\\");
            let t = t.rsplit(['/', '\\']).next().unwrap_or(t);
            t.trim_end_matches(".exe").trim_end_matches(".cmd").trim_end_matches(".bat").to_string()
        })
        .collect()
}

fn contains_sequence(tokens: &[String], pattern: &str) -> bool {
    let wanted: Vec<&str> = pattern.split(' ').collect();
    tokens.windows(wanted.len()).any(|w| w.iter().zip(&wanted).all(|(a, b)| a == b))
}

pub fn classify_command(command: &str) -> Option<CheckKind> {
    let t = tokens(command);
    if t.iter().any(|x| x == "--help" || x == "--version") || t.first().is_some_and(|f| NOT_A_RUN.contains(&f.as_str())) {
        return None;
    }
    if TEST_COMMANDS.iter().any(|p| contains_sequence(&t, p)) {
        return Some(CheckKind::Tests);
    }
    if BUILD_COMMANDS.iter().any(|p| contains_sequence(&t, p)) {
        return Some(CheckKind::Build);
    }
    None
}

/// Some runners exit with 0 even when tests fail; catch their summaries.
fn output_reports_failure(text: &str) -> bool {
    const MARKERS: &[&str] = &["test result: FAILED", "FAILURES!", "Build FAILED", "error: could not compile", "npm ERR!"];
    if MARKERS.iter().any(|m| text.contains(m)) {
        return true;
    }
    // "3 failed" (pytest, jest, …) with a number above zero.
    let lower = text.to_lowercase();
    lower.match_indices(" failed").any(|(i, _)| {
        let digits: String = lower[..i].chars().rev().take_while(|c| c.is_ascii_digit()).collect();
        digits.chars().rev().collect::<String>().parse::<u32>().is_ok_and(|n| n > 0)
    })
}

/// Turns a PostToolUse / PostToolUseFailure event into a test/build result, if it is one.
pub fn evaluate(event: &str, p: &Value) -> Option<Check> {
    let tool = p.get("tool_name")?.as_str()?;
    if tool != "Bash" && tool != "PowerShell" {
        return None;
    }
    let command = p.pointer("/tool_input/command")?.as_str()?;
    let kind = classify_command(command)?;
    match event {
        "PostToolUseFailure" => {
            if p.get("is_interrupt").and_then(Value::as_bool).unwrap_or(false) {
                return None; // you stopped it; that's not a failure
            }
            let error = p.get("error").and_then(Value::as_str).map(str::to_string);
            Some(Check { kind, command: command.into(), passed: false, output: error })
        }
        "PostToolUse" => {
            let response = p.get("tool_response");
            if response.and_then(|r| r.get("interrupted")).and_then(Value::as_bool).unwrap_or(false) {
                return None;
            }
            let field = |k: &str| response.and_then(|r| r.get(k)).and_then(Value::as_str).unwrap_or("");
            let text = format!("{}\n{}", field("stdout"), field("stderr"));
            let failed = output_reports_failure(&text);
            Some(Check { kind, command: command.into(), passed: !failed, output: failed.then(|| text.trim().to_string()) })
        }
        _ => None,
    }
}

impl Health {
    fn key(project_dir: &str, kind: CheckKind) -> String {
        format!("{}|{}", project_dir.to_lowercase(), kind.noun())
    }

    /// Records a result. Returns a change only when the state flips.
    pub fn record(&mut self, project_dir: &str, check: &Check) -> Option<Change> {
        if check.passed {
            let mut cleared = self.failures.remove(&Self::key(project_dir, check.kind)).is_some();
            if check.kind == CheckKind::Tests {
                // Tests ran, so it builds too.
                cleared |= self.failures.remove(&Self::key(project_dir, CheckKind::Build)).is_some();
            }
            return cleared.then_some(Change::NowPassing(check.kind));
        }
        let key = Self::key(project_dir, check.kind);
        let since = self
            .failures
            .get(&key)
            .map(|f| f.since.clone())
            .unwrap_or_else(|| chrono::Local::now().to_rfc3339());
        let was_failing = self.failures.contains_key(&key);
        let failure = Failure { kind: check.kind, command: shorten(&check.command, 120), project_dir: project_dir.into(), since };
        self.failures.insert(key, failure.clone());
        (!was_failing).then_some(Change::NowFailing(failure))
    }

    /// The most recent failure, if anything is failing.
    pub fn latest(&self) -> Option<&Failure> {
        self.failures.values().max_by(|a, b| a.since.cmp(&b.since))
    }

    pub fn clear(&mut self) {
        self.failures.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recognises_test_and_build_commands() {
        assert_eq!(classify_command("cargo test --workspace"), Some(CheckKind::Tests));
        assert_eq!(classify_command("cd web && npm run test -- --watch=false"), Some(CheckKind::Tests));
        assert_eq!(classify_command("python -m pytest -q tests/"), Some(CheckKind::Tests));
        assert_eq!(classify_command("./gradlew test"), Some(CheckKind::Tests));
        assert_eq!(classify_command("npx tsc --noEmit"), Some(CheckKind::Build));
        assert_eq!(classify_command("cargo build --release"), Some(CheckKind::Build));
        assert_eq!(classify_command("make -j8"), Some(CheckKind::Build));
    }

    #[test]
    fn ignores_everything_else() {
        assert_eq!(classify_command("git status"), None);
        assert_eq!(classify_command("npm install"), None);
        assert_eq!(classify_command("echo cargo test"), None);
        assert_eq!(classify_command("cargo test --help"), None);
        assert_eq!(classify_command("python makedirs.py"), None);
        assert_eq!(classify_command("cat testing.md"), None);
    }

    #[test]
    fn failure_event_makes_it_fail_and_success_makes_it_pass() {
        let fail = json!({ "tool_name": "Bash", "tool_input": { "command": "cargo test" }, "error": "Exit code 101\ntest result: FAILED" });
        let check = evaluate("PostToolUseFailure", &fail).unwrap();
        assert!(!check.passed);
        let mut health = Health::default();
        assert!(matches!(health.record("C:\\code\\app", &check), Some(Change::NowFailing(_))));
        assert!(health.record("C:\\code\\app", &check).is_none(), "still failing is not a new change");
        assert!(health.latest().is_some());

        let ok = json!({ "tool_name": "Bash", "tool_input": { "command": "cargo test" }, "tool_response": { "stdout": "test result: ok. 8 passed; 0 failed", "stderr": "", "interrupted": false } });
        let check = evaluate("PostToolUse", &ok).unwrap();
        assert!(check.passed, "'0 failed' is not a failure");
        assert!(matches!(health.record("c:\\code\\APP", &check), Some(Change::NowPassing(CheckKind::Tests))));
        assert!(health.latest().is_none());
    }

    #[test]
    fn exit_zero_with_failed_summary_still_fails() {
        let p = json!({ "tool_name": "Bash", "tool_input": { "command": "npx jest" }, "tool_response": { "stdout": "Tests: 2 failed, 10 passed", "stderr": "" } });
        assert!(!evaluate("PostToolUse", &p).unwrap().passed);
    }

    #[test]
    fn interrupted_runs_are_ignored() {
        let p = json!({ "tool_name": "Bash", "tool_input": { "command": "pytest" }, "error": "Interrupted", "is_interrupt": true });
        assert!(evaluate("PostToolUseFailure", &p).is_none());
    }
}
