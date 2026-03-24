use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use cbox_adapter::AdapterRegistry;
use cbox_container::{ContainerBackend, ContainerRuntime};
#[cfg(target_os = "linux")]
use cbox_core::capability::Capabilities;
use cbox_core::{BackendKind, CboxConfig, NetworkMode, SandboxBackend, Session, SessionStore};
#[cfg(target_os = "linux")]
use cbox_sandbox::Sandbox;

/// Options for the `cbox run` command.
pub struct RunOptions {
    pub adapter_name: String,
    pub persist: bool,
    pub session_name: Option<String>,
    pub network: String,
    pub memory: Option<String>,
    pub cpu: Option<String>,
    pub dry_run: bool,
    pub backend: String,
    pub image: Option<String>,
    pub cmd: Vec<String>,
}

pub fn execute(opts: RunOptions) -> Result<()> {
    let backend_kind = select_backend(&opts.backend)?;

    let cmd = if opts.cmd.is_empty() {
        let shell = match backend_kind {
            // For containers, use a sentinel that the entrypoint resolves to the
            // image's $SHELL (set via ENV in Dockerfile), falling back to /bin/bash.
            BackendKind::Container => ContainerBackend::default_shell_sentinel().to_string(),
            _ => std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
        };
        vec![shell]
    } else {
        opts.cmd
    };

    let cwd = std::env::current_dir()?;
    let mut config = CboxConfig::find_and_load(&cwd)?;

    config.network.mode = opts
        .network
        .parse::<NetworkMode>()
        .map_err(|e| anyhow::anyhow!(e))?;
    if let Some(mem) = opts.memory {
        config.resources.memory = mem;
    }
    if let Some(c) = opts.cpu {
        config.resources.cpu = c;
    }
    if let Some(img) = opts.image {
        config.sandbox.image = img;
    }

    let registry = AdapterRegistry::new();
    let adapter = registry.resolve(&opts.adapter_name, &cmd)?;

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

    // Skip adapter validation for container backend — tools like claude are
    // inside the container image, not on the host. The adapter's validate()
    // checks for host binaries which won't exist on macOS.
    if backend_kind != BackendKind::Container {
        adapter.validate(&config)?;
    }

    let mut env = HashMap::new();
    adapter.prepare_env(&mut env, &config)?;

    // For container backend, tools are in the image on PATH — use the command
    // as-is without adapter resolution (which looks for host binaries).
    let full_cmd = if backend_kind == BackendKind::Container {
        cmd.clone()
    } else {
        let sandbox_cmd = adapter.build_command(&cmd, &config)?;

        for (k, v) in sandbox_cmd.env {
            env.insert(k, v);
        }

        let mut c = vec![sandbox_cmd.program];
        c.extend(sandbox_cmd.args);
        c
    };

    let project_dir = CboxConfig::project_root(&cwd);
    SessionStore::ensure_dir()?;
    let session = Session::new(
        project_dir,
        opts.session_name,
        adapter.name().to_string(),
        opts.persist,
        backend_kind,
    );

    println!(
        "{} session {} (adapter: {}, backend: {}, persist: {})",
        "cbox".green().bold(),
        session.display_name().cyan(),
        adapter.name(),
        backend_kind,
        opts.persist
    );

    let result = match backend_kind {
        #[cfg(target_os = "linux")]
        BackendKind::Native => {
            let caps = Capabilities::detect();
            if !opts.dry_run {
                caps.check_minimum()?;
            }
            let sandbox = Sandbox::new(session, config, caps);
            sandbox.run(&full_cmd, env, opts.dry_run)?
        }
        #[cfg(not(target_os = "linux"))]
        BackendKind::Native => {
            anyhow::bail!("native backend is only available on Linux");
        }
        BackendKind::Container => {
            let runtime = ContainerRuntime::detect()?;
            let backend = ContainerBackend::new(session, config, runtime);
            backend.run(&full_cmd, env, opts.dry_run)?
        }
    };

    if !opts.dry_run {
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

fn select_backend(requested: &str) -> Result<BackendKind> {
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
        anyhow::bail!("no container runtime found. Install Docker Desktop or Podman.");
    }
}
