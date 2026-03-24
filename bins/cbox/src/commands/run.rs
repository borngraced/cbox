use std::collections::HashMap;
use anyhow::Result;
use colored::Colorize;

use cbox_adapter::AdapterRegistry;
use cbox_core::capability::Capabilities;
use cbox_core::{BackendKind, CboxConfig, SandboxBackend, Session, SessionStore};
use cbox_container::{ContainerBackend, ContainerRuntime};
use cbox_sandbox::Sandbox;

#[allow(clippy::too_many_arguments)]
pub fn execute(
    adapter_name: String,
    persist: bool,
    session_name: Option<String>,
    network: String,
    memory: Option<String>,
    cpu: Option<String>,
    dry_run: bool,
    backend_str: String,
    cmd: Vec<String>,
) -> Result<()> {
    let cmd = if cmd.is_empty() {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        vec![shell]
    } else {
        cmd
    };

    let backend_kind = select_backend(&backend_str, dry_run)?;

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
    let session = Session::new(
        project_dir,
        session_name,
        adapter.name().to_string(),
        persist,
        backend_kind,
    );

    println!(
        "{} session {} (adapter: {}, backend: {}, persist: {})",
        "cbox".green().bold(),
        session.display_name().cyan(),
        adapter.name(),
        backend_kind,
        persist
    );

    let result = match backend_kind {
        BackendKind::Native => {
            let caps = Capabilities::detect();
            if !dry_run {
                caps.check_minimum()
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            let sandbox = Sandbox::new(session, config, caps);
            sandbox
                .run(&full_cmd, env, dry_run)
                .map_err(|e| anyhow::anyhow!("{}", e))?
        }
        BackendKind::Container => {
            let runtime = ContainerRuntime::detect()
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let backend = ContainerBackend::new(session, config, runtime);
            backend
                .run(&full_cmd, env, dry_run)
                .map_err(|e| anyhow::anyhow!("{}", e))?
        }
    };

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

fn select_backend(requested: &str, _dry_run: bool) -> Result<BackendKind> {
    match requested {
        "native" => Ok(BackendKind::Native),
        "container" | "docker" | "podman" => Ok(BackendKind::Container),
        "auto" => auto_detect_backend(),
        other => anyhow::bail!("unknown backend: {}", other),
    }
}

fn auto_detect_backend() -> Result<BackendKind> {
    #[cfg(target_os = "linux")]
    {
        let caps = Capabilities::detect();
        if caps.user_namespaces {
            return Ok(BackendKind::Native);
        }
        if ContainerRuntime::detect().is_ok() {
            tracing::warn!("user namespaces unavailable, falling back to container backend");
            return Ok(BackendKind::Container);
        }
        anyhow::bail!("neither native namespaces nor container runtime available");
    }

    #[cfg(not(target_os = "linux"))]
    {
        if ContainerRuntime::detect().is_ok() {
            return Ok(BackendKind::Container);
        }
        anyhow::bail!(
            "no container runtime found. Install Docker Desktop or Podman."
        );
    }
}
