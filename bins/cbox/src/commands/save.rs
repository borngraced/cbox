use anyhow::{Context, Result};
use colored::Colorize;

use cbox_core::{SessionStatus, SessionStore};

pub fn execute(name: Option<String>, session_query: Option<String>) -> Result<()> {
    let mut session = match session_query {
        Some(q) => SessionStore::find(&q).context("session not found")?,
        None => {
            let sessions = SessionStore::list_all()?;
            sessions
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("no sessions found"))?
        }
    };

    if let Some(n) = name {
        session.name = Some(n);
    }

    session.status = SessionStatus::Saved;
    session.persist = true;
    SessionStore::save(&session)?;

    println!(
        "{} Session {} saved.",
        "cbox".green().bold(),
        session.display_name().cyan()
    );

    Ok(())
}
