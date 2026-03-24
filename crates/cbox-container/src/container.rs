use std::collections::HashMap;
use std::process::Command;

use cbox_core::config::CboxConfig;
use cbox_core::session::{Session, SessionStatus, SessionStore};
use cbox_core::{BackendError, BackendKind, BackendResult, SandboxBackend};
use tracing::info;

use crate::runtime::ContainerRuntime;

const BASE_IMAGE: &str = "ubuntu:24.04";

/// Container-based sandbox backend (Docker/Podman).
pub struct ContainerBackend {
    pub session: Session,
    pub config: CboxConfig,
    pub runtime: ContainerRuntime,
}

impl ContainerBackend {
    pub fn new(session: Session, config: CboxConfig, runtime: ContainerRuntime) -> Self {
        Self {
            session,
            config,
            runtime,
        }
    }

    fn build_args(
        &self,
        command: &[String],
        env: &HashMap<String, String>,
    ) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--name".to_string(),
            format!("cbox_{}", self.session.id),
            "--hostname".to_string(),
            "cbox".to_string(),
        ];

        // Volumes: project as lower, session upper/work dirs
        args.extend([
            "-v".to_string(),
            format!("{}:/lower:ro", self.session.project_dir.display()),
            "-v".to_string(),
            format!("{}:/upper:rw", self.session.upper_dir().display()),
            "-v".to_string(),
            format!("{}:/work:rw", self.session.work_dir().display()),
        ]);

        // RW bind mounts (e.g. ~/.claude) — pass through directly
        for dir in &self.config.sandbox.rw_mounts {
            if std::path::Path::new(dir).exists() {
                args.extend([
                    "-v".to_string(),
                    format!("{}:{}:rw", dir, dir),
                ]);
            }
        }

        // RO bind mounts
        for dir in &self.config.sandbox.ro_mounts {
            if std::path::Path::new(dir).exists() {
                args.extend([
                    "-v".to_string(),
                    format!("{}:{}:ro", dir, dir),
                ]);
            }
        }

        // Network
        match self.config.network.mode.as_str() {
            "allow" => {} // default bridge
            _ => {
                args.extend(["--network".to_string(), "none".to_string()]);
            }
        }

        // Resource limits
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

        // Security: need SYS_ADMIN for overlayfs inside container
        args.extend([
            "--cap-add".to_string(),
            "SYS_ADMIN".to_string(),
            "--security-opt".to_string(),
            "no-new-privileges".to_string(),
        ]);

        // Environment
        for (k, v) in env {
            args.extend(["-e".to_string(), format!("{}={}", k, v)]);
        }

        // Interactive TTY if stdin is a terminal
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            args.push("-it".to_string());
        }

        args.extend(["-w".to_string(), "/workspace".to_string()]);
        args.push(BASE_IMAGE.to_string());

        // Entrypoint: mount overlayfs then exec user command
        let user_cmd = command
            .iter()
            .map(|s| shell_escape(s))
            .collect::<Vec<_>>()
            .join(" ");

        args.extend([
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "mkdir -p /workspace && \
                 mount -t overlay overlay -o lowerdir=/lower,upperdir=/upper,workdir=/work /workspace && \
                 cd /workspace && \
                 exec {}",
                user_cmd
            ),
        ]);

        args
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

        // Create overlay directories on host
        std::fs::create_dir_all(self.session.upper_dir())?;
        std::fs::create_dir_all(self.session.work_dir())?;

        let docker_args = self.build_args(command, &env);

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

fn shell_escape(s: &str) -> String {
    if s.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == '\\' || c == '$') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}
