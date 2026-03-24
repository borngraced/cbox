use std::collections::HashMap;
use anyhow::{Context, Result};
use colored::Colorize;

use cbox_adapter::AdapterRegistry;
use cbox_core::capability::Capabilities;
use cbox_core::{CboxConfig, Session, SessionStore};
use cbox_sandbox::Sandbox;

pub fn execute(
    adapter_name: String,
    persist: bool,
    session_name: Option<String>,
    network: String,
    memory: Option<String>,
    cpu: Option<String>,
    dry_run: bool,
    cmd: Vec<String>,
) -> Result<()> {
    let cmd = if cmd.is_empty() {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        vec![shell]
    } else {
        cmd
    };

    let caps = Capabilities::detect();
    if !dry_run {
        caps.check_minimum()
            .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    let cwd = std::env::current_dir()?;
    let mut config = CboxConfig::find_and_load(&cwd)?;

    config.network.mode = network;
    if let Some(mem) = memory {
        config.resources.memory = mem;
    }
    if let Some(c) = cpu {
        config.resources.cpu = c;
    }

    let registry = AdapterRegistry::new();
    let adapter = registry
        .resolve(&adapter_name, &cmd)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Collect adapter mounts before validation so validate() sees the full config
    for mount in adapter.extra_ro_mounts() {
        if !config.sandbox.ro_mounts.contains(&mount) {
            config.sandbox.ro_mounts.push(mount);
        }
    }
    for mount in adapter.extra_rw_mounts() {
        if !config.sandbox.rw_mounts.contains(&mount) {
            config.sandbox.rw_mounts.push(mount);
        }
    }

    adapter
        .validate(&config)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut env = HashMap::new();
    adapter
        .prepare_env(&mut env, &config)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let sandbox_cmd = adapter
        .build_command(&cmd, &config)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    for (k, v) in sandbox_cmd.env {
        env.insert(k, v);
    }

    let full_cmd = {
        let mut c = vec![sandbox_cmd.program];
        c.extend(sandbox_cmd.args);
        c
    };

    let project_dir = CboxConfig::project_root(&cwd);
    SessionStore::ensure_dir()?;
    let session = Session::new(project_dir, session_name, adapter.name().to_string(), persist);

    println!(
        "{} session {} (adapter: {}, persist: {})",
        "cbox".green().bold(),
        session.display_name().cyan(),
        adapter.name(),
        persist
    );

    let sandbox = Sandbox::new(session, config, caps);
    let result = sandbox
        .run(&full_cmd, env, dry_run)
        .context("sandbox execution failed")?;

    if !dry_run {
        println!(
            "\n{} session {} exited with code {}",
            "cbox".green().bold(),
            result.session.display_name().cyan(),
            result.exit_code
        );

        if result.exit_code == 0 {
            println!(
                "  Use {} to see changes, {} to apply them.",
                "cbox diff".yellow(),
                "cbox merge".yellow()
            );
        }
    }

    std::process::exit(result.exit_code);
}
