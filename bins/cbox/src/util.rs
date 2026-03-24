use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};

use cbox_core::{Session, SessionStore};

/// Resolve a session from an optional query string.
/// If a query is given, looks it up by ID/name. Otherwise returns the most recent session.
pub fn resolve_session(query: Option<String>) -> Result<Session> {
    match query {
        Some(q) => SessionStore::find(&q).context("session not found"),
        None => {
            let sessions = SessionStore::list_all()?;
            sessions
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("no sessions found"))
        }
    }
}

/// Prompt the user for confirmation and return true if they confirmed.
pub fn confirm(prompt: &str) -> Result<bool> {
    print!("{} [y/N] ", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}
