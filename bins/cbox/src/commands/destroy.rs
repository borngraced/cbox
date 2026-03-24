use anyhow::Result;
use colored::Colorize;

use cbox_core::{BackendKind, SessionStore};
use cbox_overlay::OverlayFs;

use crate::util;

pub fn execute(all: bool, force: bool, session_query: Option<String>) -> Result<()> {
    let sessions = if all {
        SessionStore::list_all()?
    } else {
        vec![util::resolve_session(session_query)?]
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

        let prompt = format!("\nDestroy {} session(s)?", sessions.len());
        if !util::confirm(&prompt)? {
            println!("Aborted.");
            return Ok(());
        }
    }

    for session in &sessions {
        match session.backend {
            #[cfg(target_os = "linux")]
            BackendKind::Native => {
                // Kill process, clean up veth/iptables/cgroup
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

                if let Some(ref veth) = session.veth_host {
                    let _ = cbox_network::NetworkSetup::delete_veth(veth);
                }
                let _ = cbox_network::NetworkSetup::cleanup_iptables(&session.iptables_rules);

                if let Some(ref cg) = session.cgroup_path {
                    let _ = cbox_sandbox::cgroup::CgroupSetup::cleanup(std::path::Path::new(cg));
                }
            }
            #[cfg(not(target_os = "linux"))]
            BackendKind::Native => {
                // Native backend cleanup is Linux-only; shouldn't reach here on other platforms
            }
            BackendKind::Container => {
                // Stop/remove the container
                if let Some(ref runtime) = session.container_runtime {
                    let name = format!("cbox_{}", session.id);
                    let _ = std::process::Command::new(runtime)
                        .args(["stop", &name])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    let _ = std::process::Command::new(runtime)
                        .args(["rm", "-f", &name])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
            }
        }

        // Overlay cleanup is the same for both backends
        let overlay = OverlayFs::from_session(session);
        let _ = overlay.cleanup();

        SessionStore::delete(&session.id)?;

        println!(
            "{} Destroyed session {}",
            "cbox".green().bold(),
            session.display_name().cyan()
        );
    }

    Ok(())
}
