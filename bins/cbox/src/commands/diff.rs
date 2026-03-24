use anyhow::{Context, Result};
use colored::Colorize;

use cbox_core::{CboxConfig, SessionStore};
use cbox_diff::DiffRenderer;
use cbox_overlay::OverlayFs;

use crate::filter;

pub fn execute(stat: bool, name_only: bool, session_query: Option<String>) -> Result<()> {
    let session = match session_query {
        Some(q) => SessionStore::find(&q).context("session not found")?,
        None => {
            let sessions = SessionStore::list_all()?;
            sessions
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("no sessions found"))?
        }
    };

    let config = CboxConfig::find_and_load(&session.project_dir)?;
    let overlay = OverlayFs::from_session(&session);
    let changes = overlay.diff().context("failed to compute diff")?;
    let changes = filter::filter_excluded(changes, &config.sandbox.merge_exclude);

    if changes.is_empty() {
        println!("{} No changes in session {}", "cbox".green().bold(), session.display_name().cyan());
        return Ok(());
    }

    if name_only {
        println!("{}", DiffRenderer::render_names_only(&changes));
    } else if stat {
        print!("{}", DiffRenderer::render_stat(&changes));
    } else {
        let output = DiffRenderer::render_full_diff(&changes, &session.project_dir)?;
        print!("{}", output);
    }

    Ok(())
}
