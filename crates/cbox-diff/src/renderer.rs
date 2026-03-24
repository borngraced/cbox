use std::fs;
use std::path::Path;

use colored::Colorize;
use similar::{ChangeTag, TextDiff};

use crate::error::DiffError;

pub struct DiffRenderer;

impl DiffRenderer {
    /// Render a stat summary (like git diff --stat).
    pub fn render_stat(
        changes: &[cbox_overlay::OverlayChange],
    ) -> String {
        let mut output = String::new();
        let mut added_files = 0;
        let mut modified_files = 0;
        let mut deleted_files = 0;

        for change in changes {
            let indicator = match change.kind {
                cbox_overlay::ChangeKind::Added => {
                    added_files += 1;
                    "A".green()
                }
                cbox_overlay::ChangeKind::Modified => {
                    modified_files += 1;
                    "M".yellow()
                }
                cbox_overlay::ChangeKind::Deleted => {
                    deleted_files += 1;
                    "D".red()
                }
            };
            output.push_str(&format!(" {} {}\n", indicator, change.path.display()));
        }

        output.push_str(&format!(
            "\n {} file(s) changed: {} added, {} modified, {} deleted\n",
            changes.len(),
            added_files,
            modified_files,
            deleted_files
        ));
        output
    }

    /// Render name-only output.
    pub fn render_names_only(changes: &[cbox_overlay::OverlayChange]) -> String {
        changes
            .iter()
            .map(|c| format!("{}", c.path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Render full colored unified diff.
    pub fn render_full_diff(
        changes: &[cbox_overlay::OverlayChange],
        lower_dir: &Path,
    ) -> Result<String, DiffError> {
        let mut output = String::new();

        for change in changes {
            match change.kind {
                cbox_overlay::ChangeKind::Added => {
                    output.push_str(&format!(
                        "{}\n",
                        format!("--- /dev/null").red()
                    ));
                    output.push_str(&format!(
                        "{}\n",
                        format!("+++ b/{}", change.path.display()).green()
                    ));
                    if let Ok(content) = fs::read_to_string(&change.upper_path) {
                        for line in content.lines() {
                            output.push_str(&format!("{}\n", format!("+{}", line).green()));
                        }
                    } else {
                        output.push_str(&format!(
                            "{}\n",
                            "[binary file]".dimmed()
                        ));
                    }
                    output.push('\n');
                }
                cbox_overlay::ChangeKind::Modified => {
                    let lower_path = lower_dir.join(&change.path);
                    let old = fs::read_to_string(&lower_path).unwrap_or_default();
                    let new = fs::read_to_string(&change.upper_path).unwrap_or_default();

                    if old == new {
                        // Metadata-only change
                        output.push_str(&format!(
                            "{}\n",
                            format!("  {} (metadata change)", change.path.display()).dimmed()
                        ));
                        continue;
                    }

                    output.push_str(&format!(
                        "{}\n",
                        format!("--- a/{}", change.path.display()).red()
                    ));
                    output.push_str(&format!(
                        "{}\n",
                        format!("+++ b/{}", change.path.display()).green()
                    ));

                    let diff = TextDiff::from_lines(&old, &new);
                    for group in diff.grouped_ops(3) {
                        for op in &group {
                            for change in diff.iter_changes(op) {
                                let line = match change.tag() {
                                    ChangeTag::Delete => {
                                        format!("-{}", change.value().trim_end_matches('\n')).red().to_string()
                                    }
                                    ChangeTag::Insert => {
                                        format!("+{}", change.value().trim_end_matches('\n'))
                                            .green()
                                            .to_string()
                                    }
                                    ChangeTag::Equal => {
                                        format!(" {}", change.value().trim_end_matches('\n'))
                                    }
                                };
                                output.push_str(&line);
                                output.push('\n');
                            }
                        }
                    }
                    output.push('\n');
                }
                cbox_overlay::ChangeKind::Deleted => {
                    let lower_path = lower_dir.join(&change.path);
                    output.push_str(&format!(
                        "{}\n",
                        format!("--- a/{}", change.path.display()).red()
                    ));
                    output.push_str(&format!(
                        "{}\n",
                        "+++ /dev/null".green()
                    ));
                    if let Ok(content) = fs::read_to_string(&lower_path) {
                        for line in content.lines() {
                            output.push_str(&format!("{}\n", format!("-{}", line).red()));
                        }
                    }
                    output.push('\n');
                }
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbox_overlay::{OverlayChange, ChangeKind};
    use std::path::PathBuf;

    #[test]
    fn test_render_stat() {
        let changes = vec![
            OverlayChange {
                kind: ChangeKind::Added,
                path: PathBuf::from("new.txt"),
                upper_path: PathBuf::from("/tmp/upper/new.txt"),
            },
            OverlayChange {
                kind: ChangeKind::Deleted,
                path: PathBuf::from("old.txt"),
                upper_path: PathBuf::from("/tmp/upper/.wh.old.txt"),
            },
        ];
        let output = DiffRenderer::render_stat(&changes);
        assert!(output.contains("2 file(s) changed"));
    }

    #[test]
    fn test_render_names_only() {
        let changes = vec![
            OverlayChange {
                kind: ChangeKind::Added,
                path: PathBuf::from("a.txt"),
                upper_path: PathBuf::from("/tmp/a.txt"),
            },
            OverlayChange {
                kind: ChangeKind::Modified,
                path: PathBuf::from("b.txt"),
                upper_path: PathBuf::from("/tmp/b.txt"),
            },
        ];
        let output = DiffRenderer::render_names_only(&changes);
        assert_eq!(output, "a.txt\nb.txt");
    }
}
