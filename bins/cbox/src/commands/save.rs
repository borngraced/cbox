use anyhow::Result;
use colored::Colorize;

use cbox_core::{SessionStatus, SessionStore};

use crate::util;

pub fn execute(name: Option<String>, session_query: Option<String>) -> Result<()> {
    let mut session = util::resolve_session(session_query)?;

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
