use std::io::{self, BufRead, Write};

use cbox_overlay::OverlayChange;
use colored::Colorize;

/// Interactive file picker for `cbox merge --pick`.
pub struct FilePicker;

impl FilePicker {
    /// Present each change to the user and ask whether to accept it.
    /// Returns the list of accepted changes.
    pub fn pick(changes: &[OverlayChange]) -> Vec<OverlayChange> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        let mut accepted = Vec::new();

        println!(
            "\n{}\n",
            "Interactive merge — accept or reject each change:".bold()
        );

        for (i, change) in changes.iter().enumerate() {
            let indicator = match change.kind {
                cbox_overlay::ChangeKind::Added => "A".green(),
                cbox_overlay::ChangeKind::Modified => "M".yellow(),
                cbox_overlay::ChangeKind::Deleted => "D".red(),
            };

            print!(
                "[{}/{}] {} {} — accept? [y/n/q] ",
                i + 1,
                changes.len(),
                indicator,
                change.path.display()
            );
            stdout.flush().unwrap();

            let mut input = String::new();
            stdin.lock().read_line(&mut input).unwrap();
            let input = input.trim().to_lowercase();

            match input.as_str() {
                "y" | "yes" => {
                    accepted.push(change.clone());
                }
                "q" | "quit" => {
                    println!("{}", "Aborted.".yellow());
                    break;
                }
                _ => {
                    // Skip this change
                }
            }
        }

        println!(
            "\n{} of {} changes accepted.\n",
            accepted.len(),
            changes.len()
        );
        accepted
    }
}
