//! Queue of permission questions waiting for your Allow / Deny on Glowby.
//!
//! Each question holds a one-shot channel back to the pipe connection whose
//! glowby-hook.exe is waiting. Answering sends exactly one reply down it.

use crate::sessions::{describe_tool, project_name, shorten, str_field};
use glowby_protocol::HookReply;
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GateKind {
    /// A real Claude Code permission prompt (from your terminal sessions).
    Permission,
    /// Glowby's own chat asking before it edits or runs something.
    ChatGate,
}

pub struct Pending {
    pub id: u64,
    pub kind: GateKind,
    pub session_id: String,
    pub project: String,
    pub title: String,
    pub detail: String,
    pub deadline: Instant,
    reply: oneshot::Sender<HookReply>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PermView {
    pub id: u64,
    pub kind: GateKind,
    pub project: String,
    pub title: String,
    pub detail: String,
    /// Milliseconds left before the question goes back to the terminal.
    pub expires_in_ms: u64,
    /// More questions waiting behind this one.
    pub queued: usize,
}

#[derive(Default)]
pub struct Queue {
    items: VecDeque<Pending>,
    next_id: u64,
}

impl Queue {
    pub fn push(&mut self, kind: GateKind, payload: &Value, timeout: Duration, reply: oneshot::Sender<HookReply>) -> u64 {
        self.next_id += 1;
        let (title, detail) = describe_request(payload);
        self.items.push_back(Pending {
            id: self.next_id,
            kind,
            session_id: str_field(payload, "session_id").unwrap_or("").to_string(),
            project: project_name(str_field(payload, "cwd").unwrap_or("")),
            title,
            detail,
            deadline: Instant::now() + timeout,
            reply,
        });
        self.next_id
    }

    /// Sends the answer to the waiting hook. Returns the session id if found.
    pub fn resolve(&mut self, id: u64, answer: HookReply) -> Option<String> {
        let index = self.items.iter().position(|p| p.id == id)?;
        let pending = self.items.remove(index)?;
        let _ = pending.reply.send(answer);
        Some(pending.session_id)
    }

    /// The hook went away (Claude Code was closed or answered elsewhere).
    /// Returns the session id if the question was still waiting.
    pub fn forget(&mut self, id: u64) -> Option<String> {
        let index = self.items.iter().position(|p| p.id == id)?;
        self.items.remove(index).map(|p| p.session_id)
    }

    /// Answers everything with "no opinion" (used on quit and when game mode starts).
    pub fn pass_all(&mut self) {
        for pending in self.items.drain(..) {
            let _ = pending.reply.send(HookReply::Pass);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn front_view(&self) -> Option<PermView> {
        let front = self.items.front()?;
        Some(PermView {
            id: front.id,
            kind: front.kind,
            project: front.project.clone(),
            title: front.title.clone(),
            detail: front.detail.clone(),
            expires_in_ms: front.deadline.saturating_duration_since(Instant::now()).as_millis() as u64,
            queued: self.items.len() - 1,
        })
    }
}

/// Human-friendly question for the bubble: title + the important detail.
fn describe_request(p: &Value) -> (String, String) {
    let tool = str_field(p, "tool_name").unwrap_or("a tool");
    let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
    let field = |k: &str| str_field(&input, k).unwrap_or("").to_string();
    match tool {
        "Bash" | "PowerShell" => ("Run this command?".into(), shorten(&field("command"), 600)),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let path = ["file_path", "notebook_path"].iter().map(|k| field(k)).find(|s| !s.is_empty()).unwrap_or_default();
            let preview = ["new_string", "content", "new_source"].iter().map(|k| field(k)).find(|s| !s.is_empty()).unwrap_or_default();
            let detail = if preview.is_empty() { path } else { format!("{path}\n\n{}", shorten(&preview, 400)) };
            (format!("{}?", describe_tool(p)), detail)
        }
        "WebFetch" => ("Open this web page?".into(), field("url")),
        "WebSearch" => ("Search the web?".into(), field("query")),
        _ => {
            let detail = serde_json::to_string_pretty(&input).unwrap_or_default();
            (format!("Use {tool}?"), shorten(&detail, 500))
        }
    }
}
