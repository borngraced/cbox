use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use colored::Colorize;

use cbox_core::{CboxConfig, SessionStore};
use cbox_diff::{DiffRenderer, FilePicker};
use cbox_overlay::OverlayFs;

use crate::filter;

pub fn execute(pick: bool, force: bool, dry_run: bool, session_query: Option<String>) -> Result<()> {
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
        println!("{} No changes to merge.", "cbox".green().bold());
        return Ok(());
    }

    print!("{}", DiffRenderer::render_stat(&changes));

    let changes_to_merge = if pick {
        FilePicker::pick(&changes)
    } else {
        changes
    };

    if changes_to_merge.is_empty() {
        println!("No changes selected.");
        return Ok(());
    }

    if dry_run {
        println!("\n{} Dry run — {} changes would be applied.", "cbox".green().bold(), changes_to_merge.len());
        return Ok(());
    }

    if !force {
        print!(
            "\nApply {} changes to {}? [y/N] ",
            changes_to_merge.len(),
            session.project_dir.display()
        );
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    overlay
        .merge(&changes_to_merge)
        .context("merge failed")?;

    println!(
        "\n{} {} changes merged into {}",
        "cbox".green().bold(),
        changes_to_merge.len(),
        session.project_dir.display()
    );

    Ok(())
}
