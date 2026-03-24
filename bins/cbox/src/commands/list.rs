use anyhow::Result;
use colored::Colorize;

use cbox_core::{SessionStatus, SessionStore};

pub fn execute(all: bool, json: bool) -> Result<()> {
    let sessions = SessionStore::list_all()?;

    let filtered: Vec<_> = if all {
        sessions
    } else {
        sessions
            .into_iter()
            .filter(|s| s.status != SessionStatus::Saved)
            .collect()
    };

    if json {
        let json_output = serde_json::to_string_pretty(&filtered)?;
        println!("{}", json_output);
        return Ok(());
    }

    if filtered.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    println!(
        "{:<12} {:<16} {:<10} {:<8} {:<24} PROJECT",
        "ID", "NAME", "STATUS", "ALIVE", "CREATED"
    );
    println!("{}", "-".repeat(90));

    for session in &filtered {
        let alive = if SessionStore::is_alive(session) {
            "yes".green().to_string()
        } else {
            "no".dimmed().to_string()
        };

        let status = match session.status {
            SessionStatus::Running => "running".green().to_string(),
            SessionStatus::Stopped => "stopped".yellow().to_string(),
            SessionStatus::Saved => "saved".cyan().to_string(),
        };

        let name = session.name.as_deref().unwrap_or("-");
        let created = session.created_at.format("%Y-%m-%d %H:%M:%S").to_string();

        println!(
            "{:<12} {:<16} {:<10} {:<8} {:<24} {}",
            session.id,
            name,
            status,
            alive,
            created,
            session.project_dir.display()
        );
    }

    println!("\n{} session(s)", filtered.len());
    Ok(())
}
