//! Prints Glowby's case report for your Claude Code logs, as plain text.
//!
//!     detective-report [--days N]     (default: the last 7 days)
//!
//! Reads ~/.claude/projects on this PC. Nothing is sent anywhere.

fn main() {
    let mut days = 7i64;
    let args: Vec<String> = std::env::args().collect();
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
