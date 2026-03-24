use std::collections::HashMap;
use std::process::Command;

use cbox_core::config::CboxConfig;
use cbox_core::session::{Session, SessionStatus, SessionStore};
use cbox_core::{BackendError, BackendKind, BackendResult, SandboxBackend};
use tracing::info;

use crate::runtime::ContainerRuntime;

const DEFAULT_IMAGE: &str = "ubuntu:24.04";
/// Bump this version whenever CBOX_BASE_DOCKERFILE changes to force a rebuild.
const CBOX_BASE_IMAGE: &str = "cbox-base:v2";

/// Sentinel value used to signal that the container should use its image's
/// default $SHELL. Must not collide with any real command name.
const DEFAULT_SHELL_SENTINEL: &str = "__cbox_internal_default_shell_v1__";

/// Dockerfile for the auto-built cbox base image.
const CBOX_BASE_DOCKERFILE: &str = r#"
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    bash zsh fish \
    curl wget git vim less jq tree htop \
    build-essential ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV SHELL=/bin/bash
ENV PATH="/root/.local/bin:${PATH}"
"#;

/// Container-based sandbox backend (Docker/Podman).
pub struct ContainerBackend {
    session: Session,
    config: CboxConfig,
    runtime: ContainerRuntime,
}

impl ContainerBackend {
    pub fn new(session: Session, config: CboxConfig, runtime: ContainerRuntime) -> Self {
        Self {
            session,
            config,
            runtime,
        }
    }

    /// Ensure the cbox base image exists, building it if necessary.
    fn ensure_base_image(&self) -> Result<(), BackendError> {
        let output = Command::new(self.runtime.command_name())
            .args(["image", "inspect", CBOX_BASE_IMAGE])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        if let Ok(status) = output {
            if status.success() {
                return Ok(());
            }
        }

        println!("Building cbox base image (first run only)...");

        let mut child = Command::new(self.runtime.command_name())
            .args(["build", "-t", CBOX_BASE_IMAGE, "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| BackendError::Backend(format!("failed to start build: {}", e)))?;

        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            stdin
                .write_all(CBOX_BASE_DOCKERFILE.as_bytes())
                .map_err(|e| BackendError::Backend(format!("failed to write Dockerfile: {}", e)))?;
        }
        // Close stdin so docker reads EOF
        drop(child.stdin.take());

        let status = child
            .wait()
            .map_err(|e| BackendError::Backend(format!("build failed: {}", e)))?;

        if !status.success() {
            return Err(BackendError::Backend(
                "failed to build cbox base image".to_string(),
            ));
        }

        println!("cbox base image built successfully.");
        Ok(())
    }

    fn resolve_image(&self) -> Result<String, BackendError> {
        let configured = &self.config.sandbox.image;
        let image = if configured.is_empty() || configured == DEFAULT_IMAGE {
            CBOX_BASE_IMAGE.to_string()
        } else {
            configured.clone()
        };
        if image.starts_with('-') {
            return Err(BackendError::Backend(format!(
                "invalid image name {:?}: must not start with '-'",
                image
            )));
        }
        Ok(image)
    }

    /// Return the sentinel used for default shell resolution inside containers.
    pub fn default_shell_sentinel() -> &'static str {
        DEFAULT_SHELL_SENTINEL
    }

    fn build_args(
        &self,
        command: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Vec<String>, BackendError> {
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--name".to_string(),
            format!("cbox_{}", self.session.id),
            "--hostname".to_string(),
            "cbox".to_string(),
        ];

        // Volumes: project as read-only reference, host upper dir for capturing changes
        args.extend([
            "-v".to_string(),
            format!("{}:/lower:ro", self.session.project_dir.display()),
            "-v".to_string(),
            format!("{}:/host_upper:rw", self.session.upper_dir().display()),
        ]);

        // RW bind mounts — remap host home paths to /root/ inside the container
        // so tools find their config at the expected location.
        let host_home = std::env::var("HOME").unwrap_or_default();
        for dir in &self.config.sandbox.rw_mounts {
            if std::path::Path::new(dir).exists() {
                let container_path = if !host_home.is_empty() && dir.starts_with(&host_home) {
                    format!("/root{}", &dir[host_home.len()..])
                } else {
                    dir.clone()
                };
                args.extend(["-v".to_string(), format!("{}:{}:rw", dir, container_path)]);
            }
        }

        // Always mount Claude Code config so sessions stay authenticated.
        // This runs regardless of the adapter since users may launch claude
        // interactively inside any session.
        if !host_home.is_empty() {
            for name in [".claude", ".claude.json"] {
                let host_path = format!("{}/{}", host_home, name);
                if std::path::Path::new(&host_path).exists()
                    && !self.config.sandbox.rw_mounts.contains(&host_path)
                {
                    args.extend(["-v".to_string(), format!("{}:/root/{}:rw", host_path, name)]);
                }
            }
        }

        // RO bind mounts — the default ro_mounts (/usr, /lib, /bin, /sbin, /etc) are
        // for the native Linux backend where host binaries run inside the sandbox.
        // On macOS, host binaries are mach-o and can't run in the Linux container,
        // so we skip system dirs entirely. Only mount user-specified non-system paths.
        let system_dirs = ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc"];
        for dir in &self.config.sandbox.ro_mounts {
            if system_dirs.contains(&dir.as_str()) {
                continue;
            }
            if std::path::Path::new(dir).exists() {
                args.extend(["-v".to_string(), format!("{}:{}:ro", dir, dir)]);
            }
        }

        match self.config.network.mode {
            cbox_core::NetworkMode::Allow => {} // default bridge
            cbox_core::NetworkMode::Deny => {
                args.extend(["--network".to_string(), "none".to_string()]);
            }
        }

        if let Ok(mem_bytes) = CboxConfig::parse_memory_bytes(&self.config.resources.memory) {
            args.extend(["--memory".to_string(), mem_bytes.to_string()]);
        }
        if let Ok((quota, period)) = CboxConfig::parse_cpu_quota(&self.config.resources.cpu) {
            args.extend([
                "--cpu-quota".to_string(),
                quota.to_string(),
                "--cpu-period".to_string(),
                period.to_string(),
            ]);
        }
        args.extend([
            "--pids-limit".to_string(),
            self.config.resources.max_pids.to_string(),
        ]);

        // Security: SYS_ADMIN needed for overlayfs mount inside the container.
        // Note: we intentionally do NOT add no-new-privileges here because it
        // can prevent SYS_ADMIN from being effective for mount operations.
        args.extend(["--cap-add".to_string(), "SYS_ADMIN".to_string()]);

        for (k, v) in env {
            args.extend(["-e".to_string(), format!("{}={}", k, v)]);
        }

        // Interactive TTY if stdin is a terminal
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            args.push("-it".to_string());
        }

        args.extend(["-w".to_string(), "/workspace".to_string()]);
        // "--" terminates flag parsing, preventing a malicious image name
        // (e.g. "--privileged") from being interpreted as a Docker flag.
        args.push("--".to_string());
        args.push(self.resolve_image()?);

        // Resolve the default shell sentinel to the image's $SHELL or /bin/bash
        let resolved_command: Vec<String> = command
            .iter()
            .map(|s| {
                if s == DEFAULT_SHELL_SENTINEL {
                    "${SHELL:-/bin/bash}".to_string()
                } else {
                    s.clone()
                }
            })
            .collect();

        let user_cmd = resolved_command
            .iter()
            .map(|s| {
                // Don't shell-escape the $SHELL variable expansion
                if s.starts_with("${") {
                    s.clone()
                } else {
                    shell_escape(s)
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        // Entrypoint strategy:
        //   Use a size-capped tmpfs for all overlay dirs — tmpfs is always compatible
        //   with overlayfs regardless of host filesystem (avoids virtiofs and nested-
        //   overlay issues on macOS Docker Desktop). Capped at 2G to avoid unbounded
        //   RAM usage on large projects.
        //   1. Mount tmpfs at /ovl
        //   2. Try overlayfs with /lower (host volume) as lowerdir directly
        //   3. If that fails (virtiofs), copy project to tmpfs and use that
        //   4. On exit, sync the overlay upper (only changes) to host volume
        args.extend([
            "sh".to_string(),
            "-c".to_string(),
            format!(
                r#"mkdir -p /workspace /ovl && mount -t tmpfs -o size=2G tmpfs /ovl && mkdir -p /ovl/upper /ovl/work;
if mount -t overlay overlay -o lowerdir=/lower,upperdir=/ovl/upper,workdir=/ovl/work /workspace 2>/dev/null; then
  :;
else
  mkdir -p /ovl/lower && cp -a /lower/. /ovl/lower/ && \
  mount -t overlay overlay -o lowerdir=/ovl/lower,upperdir=/ovl/upper,workdir=/ovl/work /workspace;
fi;
cd /workspace && {user_cmd}; EXIT=$?;
cd / && umount /workspace 2>/dev/null;
cp -a /ovl/upper/. /host_upper/ 2>/dev/null;
exit $EXIT"#,
                user_cmd = user_cmd
            ),
        ]);

        Ok(args)
    }

    fn print_dry_run(&self, command: &[String], env: &HashMap<String, String>) {
        println!("=== Dry Run (container: {}) ===", self.runtime);
        println!("Session: {}", self.session.display_name());
        println!("Project: {}", self.session.project_dir.display());
        println!("Command: {:?}", command);
        println!("Adapter: {}", self.session.adapter);
        println!("Network: {}", self.config.network.mode);
        println!("Memory:  {}", self.config.resources.memory);
        println!("CPU:     {}", self.config.resources.cpu);
        println!("Persist: {}", self.session.persist);
        println!("Runtime: {}", self.runtime);
        println!("Image:   {}", self.resolve_image().unwrap_or_default());
        if !env.is_empty() {
            println!("Env vars: {:?}", env.keys().collect::<Vec<_>>());
        }
    }
}

impl SandboxBackend for ContainerBackend {
    fn run(
        mut self,
        command: &[String],
        env: HashMap<String, String>,
        dry_run: bool,
    ) -> Result<BackendResult, BackendError> {
        if dry_run {
            self.print_dry_run(command, &env);
            return Ok(BackendResult {
                exit_code: 0,
                session: self.session,
            });
        }

        // Auto-build the cbox base image if using the default image
        let configured = &self.config.sandbox.image;
        if configured.is_empty() || configured == DEFAULT_IMAGE {
            self.ensure_base_image()?;
        }

        // Create upper directory on host for capturing changes
        std::fs::create_dir_all(self.session.upper_dir())?;

        let docker_args = self.build_args(command, &env)?;

        self.session.container_runtime = Some(self.runtime.to_string());
        SessionStore::save(&self.session)?;

        info!(
            "launching container: {} {}",
            self.runtime,
            docker_args.join(" ")
        );

        let status = Command::new(self.runtime.command_name())
            .args(&docker_args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .map_err(|e| BackendError::Backend(format!("{} failed: {}", self.runtime, e)))?;

        let exit_code = status.code().unwrap_or(1);

        self.session.status = SessionStatus::Stopped;
        SessionStore::save(&self.session)?;

        Ok(BackendResult {
            exit_code,
            session: self.session,
        })
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Container
    }
}

/// Shell-escape a string for safe inclusion in `sh -c` commands.
fn shell_escape(s: &str) -> String {
    if s.contains(|c: char| {
        c.is_whitespace()
            || c == '\''
            || c == '"'
            || c == '\\'
            || c == '$'
            || c == '`'
            || c == '!'
            || c == '('
            || c == ')'
            || c == '&'
            || c == '|'
            || c == ';'
            || c == '<'
            || c == '>'
    }) {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}
