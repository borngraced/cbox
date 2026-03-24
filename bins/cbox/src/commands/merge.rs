use anyhow::{Context, Result};
use colored::Colorize;

use cbox_core::CboxConfig;
use cbox_diff::{DiffRenderer, FilePicker};
use cbox_overlay::OverlayFs;

use crate::filter;
use crate::util;

pub fn execute(
    pick: bool,
    force: bool,
    dry_run: bool,
    session_query: Option<String>,
) -> Result<()> {
    let session = util::resolve_session(session_query)?;

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
        println!(
            "\n{} Dry run — {} changes would be applied.",
            "cbox".green().bold(),
            changes_to_merge.len()
        );
        return Ok(());
    }

    if !force {
        let prompt = format!(
            "\nApply {} changes to {}?",
            changes_to_merge.len(),
            session.project_dir.display()
        );
        if !util::confirm(&prompt)? {
            println!("Aborted.");
            return Ok(());
        }
    }

    overlay.merge(&changes_to_merge).context("merge failed")?;

    println!(
        "\n{} {} changes merged into {}",
        "cbox".green().bold(),
        changes_to_merge.len(),
        session.project_dir.display()
    );

    Ok(())
}
