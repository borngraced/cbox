use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use colored::Colorize;

use cbox_core::SessionStore;
use cbox_network::NetworkSetup;
use cbox_overlay::OverlayFs;
use cbox_sandbox::cgroup::CgroupSetup;

pub fn execute(all: bool, force: bool, session_query: Option<String>) -> Result<()> {
    let sessions = if all {
        SessionStore::list_all()?
    } else {
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
        vec![session]
    };

    if sessions.is_empty() {
        println!("No sessions to destroy.");
        return Ok(());
    }

    if !force {
        println!("Sessions to destroy:");
        for s in &sessions {
            let alive = if SessionStore::is_alive(s) {
                " (running)".red().to_string()
            } else {
                String::new()
            };
            println!("  {} [{}]{}", s.display_name(), s.status, alive);
        }

        print!("\nDestroy {} session(s)? [y/N] ", sessions.len());
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    for session in &sessions {
        if let Some(pid) = session.pid {
            if SessionStore::is_alive(session) {
                let npid = nix::unistd::Pid::from_raw(pid as i32);
                let _ = nix::sys::signal::kill(npid, nix::sys::signal::Signal::SIGTERM);
                std::thread::sleep(std::time::Duration::from_secs(2));
                if SessionStore::is_alive(session) {
                    let _ = nix::sys::signal::kill(npid, nix::sys::signal::Signal::SIGKILL);
                }
            }
        }

        let overlay = OverlayFs::from_session(session);
        let _ = overlay.cleanup();

        if let Some(ref veth) = session.veth_host {
            let _ = NetworkSetup::delete_veth(veth);
        }
        let _ = NetworkSetup::cleanup_iptables(&session.iptables_rules);

        if let Some(ref cg) = session.cgroup_path {
            let _ = CgroupSetup::cleanup(std::path::Path::new(cg));
        }

        SessionStore::delete(&session.id)?;

        println!(
            "{} Destroyed session {}",
            "cbox".green().bold(),
            session.display_name().cyan()
        );
    }

    Ok(())
}
