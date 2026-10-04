//! Prints Glowby's case report for your Claude Code logs, as plain text.
//!
//!     detective-report [--days N]     (default: the last 7 days)
//!     detective-report --chat FILE    one chat: its size, when its cache goes
//!                                     cold, a limit hit, and the handoff note
//!
//! Reads ~/.claude/projects on this PC. Nothing is sent anywhere.

fn main() {
    let mut days = 7i64;
    let args: Vec<String> = std::env::args().collect();
    if let Some(file) = args.iter().position(|a| a == "--chat").and_then(|i| args.get(i + 1)) {
        let text = std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("Couldn't read {file}: {e}");
            std::process::exit(1);
        });
        let s = glowby_detective::live::chat_state(&text);
        let when = |t: i64| chrono::DateTime::from_timestamp(t, 0).map(|d| d.with_timezone(&chrono::Local).format("%a %H:%M:%S").to_string()).unwrap_or_default();
        println!("size: {} tokens", s.context);
        println!("last reply: {}  cache lifetime: {:?} s  cold at: {}", when(s.last_reply_at), s.ttl, when(s.expires_at()));
        if let Some(hit) = &s.limit_hit {
            println!("limit hit: {} ({}), resets {}", when(hit.at), hit.kind, when(hit.resets_at));
        }
        println!("\n--- handoff note ---\n{}", glowby_detective::live::handoff_note(&text, ""));
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--days") {
        days = args.get(i + 1).and_then(|d| d.parse().ok()).unwrap_or(7).clamp(1, 60);
    }
    let Some(root) = glowby_detective::claude_projects_dir() else {
        eprintln!("Couldn't find your user folder.");
        std::process::exit(1);
    };
    let now = chrono::Local::now().timestamp();
    let report = glowby_detective::analyze_dir(&root, now - days * 86_400, now);
    println!("{}", glowby_detective::render_text(&report));
}
