use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::io::AsRawFd;
use std::path::Path;

use cbox_core::capability::Capabilities;
use cbox_core::{CboxConfig, Session, SessionStatus, SessionStore};
use cbox_network::{NetworkSetup, NetworkConfig, NetworkMode};
use cbox_overlay::OverlayFs;
use nix::mount::{mount, MsFlags};
use nix::sched::CloneFlags;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{self, ForkResult};
use tracing::{debug, error, info, warn};

use crate::cgroup::CgroupSetup;
use crate::cleanup::CleanupStack;
use crate::error::SandboxError;
use crate::seccomp;

/// Main sandbox orchestrator.
pub struct Sandbox {
    pub session: Session,
    pub config: CboxConfig,
    pub capabilities: Capabilities,
}

pub struct SandboxResult {
    pub exit_code: i32,
    pub session: Session,
}

impl Sandbox {
    pub fn new(session: Session, config: CboxConfig, capabilities: Capabilities) -> Self {
        Self {
            session,
            config,
            capabilities,
        }
    }

    /// Execute the sandbox lifecycle: setup, run, teardown.
    pub fn run(
        mut self,
        command: &[String],
        env: HashMap<String, String>,
        dry_run: bool,
    ) -> Result<SandboxResult, SandboxError> {
        if dry_run {
            info!("dry-run: would create sandbox for {:?}", command);
            self.print_dry_run(command, &env);
            return Ok(SandboxResult {
                exit_code: 0,
                session: self.session,
            });
        }

        let mut cleanup = CleanupStack::new();

        // === Phase 1: Pre-fork (host) ===

        let overlay = OverlayFs::from_session(&self.session);
        overlay.setup()?;

        // Overlay dirs are preserved for `cbox diff` / `cbox merge` after exit.
        // Full cleanup happens via `cbox destroy`.

        let resolved_hosts = NetworkSetup::resolve_whitelist(&self.config.network.allow)?;
        let net_mode = match self.config.network.mode.as_str() {
            "allow" => NetworkMode::Allow,
            _ => NetworkMode::Deny,
        };

        let existing_sessions = SessionStore::list_all()?;
        let subnet_index = NetworkSetup::allocate_subnet_index(&existing_sessions);
        self.session.subnet_index = Some(subnet_index);

        let cgroup_path = if self.capabilities.cgroups_v2 {
            let mem_bytes = CboxConfig::parse_memory_bytes(&self.config.resources.memory)?;
            let (cpu_quota, cpu_period) = CboxConfig::parse_cpu_quota(&self.config.resources.cpu)?;
            match CgroupSetup::create(
                &self.session.id,
                mem_bytes,
                cpu_quota,
                cpu_period,
                self.config.resources.max_pids,
            ) {
                Ok(path) => {
                    let cpath = path.clone();
                    cleanup.push("remove cgroup", move || {
                        let _ = CgroupSetup::cleanup(&cpath);
                    });
                    self.session.cgroup_path = Some(path.to_string_lossy().to_string());
                    Some(path)
                }
                Err(e) => {
                    warn!("cgroup setup failed (continuing without): {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Two socketpairs for parent↔child sync:
        // unshare_*: child signals parent after unshare(), parent writes mappings then signals back
        // ready_*: parent signals child after all host-side setup is done
        let (unshare_parent_fd, unshare_child_fd) = nix::sys::socket::socketpair(
            nix::sys::socket::AddressFamily::Unix,
            nix::sys::socket::SockType::Stream,
            None,
            nix::sys::socket::SockFlag::empty(),
        )?;
        let (ready_parent_fd, ready_child_fd) = nix::sys::socket::socketpair(
            nix::sys::socket::AddressFamily::Unix,
            nix::sys::socket::SockType::Stream,
            None,
            nix::sys::socket::SockFlag::empty(),
        )?;

        let is_root = unistd::getuid().is_root();

        // === Phase 2: Fork ===
        info!("forking sandbox process...");

        let child_command: Vec<CString> = command
            .iter()
            .map(|s| CString::new(s.as_str()).unwrap())
            .collect();
        let child_env = env.clone();
        let child_overlay = OverlayFs::from_session(&self.session);
        let child_ro_mounts = self.config.sandbox.ro_mounts.clone();
        let child_dns = self.config.network.dns.clone();
        let child_blocked_syscalls = self.config.sandbox.blocked_syscalls.clone();
        let child_subnet = subnet_index;
        let has_net_tools = self.capabilities.ip_command;
        let child_project_dir = self.session.project_dir.clone();

        // Skip CLONE_NEWUSER when running as root — already have full privileges
        let mut clone_flags = CloneFlags::CLONE_NEWPID
            | CloneFlags::CLONE_NEWNS
            | CloneFlags::CLONE_NEWNET
            | CloneFlags::CLONE_NEWUTS
            | CloneFlags::CLONE_NEWIPC;
        if !is_root {
            clone_flags |= CloneFlags::CLONE_NEWUSER;
        }

        match unsafe { unistd::fork() }? {
            ForkResult::Parent { child } => {
                // === Phase 3: Parent post-fork ===
                let child_pid = child.as_raw() as u32;
                self.session.pid = Some(child_pid);
                info!("sandbox child pid: {}", child_pid);

                drop(unshare_child_fd);
                drop(ready_child_fd);

                // Wait for child to complete unshare()
                let mut buf = [0u8; 1];
                nix::unistd::read(unshare_parent_fd.as_raw_fd(), &mut buf)
                    .map_err(|e| SandboxError::Namespace(format!("wait for child unshare: {}", e)))?;
                drop(unshare_parent_fd);

                info!("child has unshared namespaces");

                if !is_root {
                    let uid = unistd::getuid();
                    let gid = unistd::getgid();
                    Self::write_id_mappings(child_pid, uid.as_raw(), gid.as_raw())?;
                }

                if has_net_tools && net_mode != NetworkMode::Allow {
                    let veth_name = NetworkSetup::veth_host_name(&self.session.id);
                    match NetworkSetup::create_veth_pair(&veth_name, child_pid, subnet_index) {
                        Ok(()) => {
                            self.session.veth_host = Some(veth_name.clone());
                            let veth_cleanup = veth_name.clone();
                            cleanup.push("delete veth", move || {
                                let _ = NetworkSetup::delete_veth(&veth_cleanup);
                            });

                            let net_config = NetworkConfig {
                                mode: net_mode.clone(),
                                allowed_hosts: resolved_hosts,
                                dns_servers: self.config.network.dns.clone(),
                                subnet_index,
                            };
                            match NetworkSetup::apply_iptables_rules(&veth_name, &net_config) {
                                Ok(rules) => {
                                    self.session.iptables_rules = rules.clone();
                                    cleanup.push("cleanup iptables", move || {
                                        let _ = NetworkSetup::cleanup_iptables(&rules);
                                    });
                                }
                                Err(e) => warn!("iptables setup failed: {}", e),
                            }
                        }
                        Err(e) => warn!("veth setup failed: {}", e),
                    }
                }

                if let Some(ref cg) = cgroup_path {
                    let _ = CgroupSetup::add_process(cg, child_pid);
                }

                SessionStore::save(&self.session)?;

                // Signal child that host-side setup is done
                nix::unistd::write(&ready_parent_fd, &[1u8])?;
                drop(ready_parent_fd);

                info!("waiting for sandbox process...");
                let exit_code = match waitpid(child, None) {
                    Ok(WaitStatus::Exited(_, code)) => {
                        info!("sandbox exited with code {}", code);
                        code
                    }
                    Ok(WaitStatus::Signaled(_, sig, _)) => {
                        info!("sandbox killed by signal {:?}", sig);
                        128 + sig as i32
                    }
                    Ok(status) => {
                        warn!("sandbox exited with unexpected status: {:?}", status);
                        1
                    }
                    Err(e) => {
                        error!("waitpid failed: {}", e);
                        1
                    }
                };

                self.session.status = SessionStatus::Stopped;
                SessionStore::save(&self.session)?;

                // Clean up ephemeral resources (network, cgroup) but keep
                // overlay dirs for `cbox diff` / `cbox merge`.
                cleanup.run_all();

                Ok(SandboxResult {
                    exit_code,
                    session: self.session,
                })
            }
            ForkResult::Child => {
                // === Phase 4: Child (inside namespaces) ===
                drop(unshare_parent_fd);
                drop(ready_parent_fd);

                nix::sched::unshare(clone_flags)
                    .map_err(|e| {
                        eprintln!("unshare failed: {}", e);
                        std::process::exit(1);
                    })
                    .unwrap();

                // Signal parent so it can write uid/gid mappings
                nix::unistd::write(&unshare_child_fd, &[1u8]).unwrap();
                drop(unshare_child_fd);

                // Wait for parent to finish host-side setup (uid_map, network, cgroup)
                let mut buf = [0u8; 1];
                nix::unistd::read(ready_child_fd.as_raw_fd(), &mut buf).unwrap();
                drop(ready_child_fd);

                // Second fork to be PID 1 in the new PID namespace
                match unsafe { unistd::fork() }.unwrap() {
                    ForkResult::Parent { child } => {
                        match waitpid(child, None) {
                            Ok(WaitStatus::Exited(_, code)) => std::process::exit(code),
                            _ => std::process::exit(1),
                        }
                    }
                    ForkResult::Child => {
                        if has_net_tools {
                            if let Err(e) = NetworkSetup::configure_child_network(
                                child_subnet,
                                &child_dns,
                            ) {
                                eprintln!("warning: child network setup failed: {}", e);
                            }
                        }

                        if let Err(e) = Self::setup_child_mounts(
                            &child_overlay,
                            &child_ro_mounts,
                            &child_project_dir,
                        ) {
                            eprintln!("mount setup failed: {}", e);
                            std::process::exit(1);
                        }

                        let _ = nix::unistd::sethostname("cbox");

                        // Seccomp MUST be applied last — it blocks mount/pivot_root syscalls needed above
                        if let Err(e) = seccomp::apply_seccomp_filter(&child_blocked_syscalls) {
                            eprintln!("seccomp setup failed: {}", e);
                            std::process::exit(1);
                        }

                        let mut final_env: Vec<CString> = Vec::new();
                        for (k, v) in &child_env {
                            final_env.push(
                                CString::new(format!("{}={}", k, v)).unwrap(),
                            );
                        }
                        final_env.push(CString::new("PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin").unwrap());
                        if final_env.iter().all(|e| !e.to_str().unwrap_or("").starts_with("HOME=")) {
                            final_env.push(CString::new("HOME=/root").unwrap());
                        }
                        if final_env.iter().all(|e| !e.to_str().unwrap_or("").starts_with("TERM=")) {
                            final_env.push(CString::new("TERM=xterm-256color").unwrap());
                        }

                        if child_command.is_empty() {
                            eprintln!("no command to execute");
                            std::process::exit(1);
                        }

                        let resolved = Self::resolve_command(&child_command[0]);
                        let exec_path = resolved.as_ref().unwrap_or(&child_command[0]);

                        match nix::unistd::execve(exec_path, &child_command, &final_env) {
                            Ok(_) => unreachable!(),
                            Err(e) => {
                                eprintln!("execve failed: {} (path: {:?})", e, exec_path);
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }
        }
    }

    fn write_id_mappings(child_pid: u32, uid: u32, gid: u32) -> Result<(), SandboxError> {
        // "deny" setgroups is required before writing gid_map as unprivileged user
        let setgroups_path = format!("/proc/{}/setgroups", child_pid);
        std::fs::write(&setgroups_path, "deny").map_err(|e| {
            SandboxError::Namespace(format!("write setgroups: {}", e))
        })?;

        // Map uid/gid 0 inside → real uid/gid outside
        let uid_map = format!("/proc/{}/uid_map", child_pid);
        std::fs::write(&uid_map, format!("0 {} 1", uid)).map_err(|e| {
            SandboxError::Namespace(format!("write uid_map: {}", e))
        })?;

        let gid_map = format!("/proc/{}/gid_map", child_pid);
        std::fs::write(&gid_map, format!("0 {} 1", gid)).map_err(|e| {
            SandboxError::Namespace(format!("write gid_map: {}", e))
        })?;

        debug!("uid/gid mappings written for pid {}", child_pid);
        Ok(())
    }

    fn setup_child_mounts(
        overlay: &OverlayFs,
        ro_mounts: &[String],
        _project_dir: &Path,
    ) -> Result<(), SandboxError> {
        mount::<str, str, str, str>(
            None,
            "/",
            None,
            MsFlags::MS_PRIVATE | MsFlags::MS_REC,
            None,
        )
        .map_err(|e| SandboxError::Mount(format!("privatize /: {}", e)))?;

        overlay
            .mount()
            .map_err(|e| SandboxError::Mount(format!("overlayfs: {}", e)))?;

        for dir in ro_mounts {
            let source = Path::new(dir);
            let target = overlay.merged_dir.join(dir.trim_start_matches('/'));
            if source.exists() {
                std::fs::create_dir_all(&target)?;
                mount(
                    Some(source),
                    &target,
                    None::<&str>,
                    MsFlags::MS_BIND | MsFlags::MS_REC,
                    None::<&str>,
                )
                .map_err(|e| {
                    SandboxError::Mount(format!("bind mount {}: {}", dir, e))
                })?;
                mount(
                    None::<&str>,
                    &target,
                    None::<&str>,
                    MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY | MsFlags::MS_REC,
                    None::<&str>,
                )
                .map_err(|e| {
                    SandboxError::Mount(format!("remount ro {}: {}", dir, e))
                })?;
            }
        }

        let proc_dir = overlay.merged_dir.join("proc");
        std::fs::create_dir_all(&proc_dir)?;
        mount(
            Some("proc"),
            &proc_dir,
            Some("proc"),
            MsFlags::empty(),
            None::<&str>,
        )
        .map_err(|e| SandboxError::Mount(format!("mount /proc: {}", e)))?;

        let tmp_dir = overlay.merged_dir.join("tmp");
        std::fs::create_dir_all(&tmp_dir)?;
        mount(
            Some("tmpfs"),
            &tmp_dir,
            Some("tmpfs"),
            MsFlags::empty(),
            Some("size=1G"),
        )
        .map_err(|e| SandboxError::Mount(format!("mount /tmp: {}", e)))?;

        let devpts_dir = overlay.merged_dir.join("dev/pts");
        std::fs::create_dir_all(&devpts_dir)?;
        let dev_dir = overlay.merged_dir.join("dev");
        mount(
            Some("tmpfs"),
            &dev_dir,
            Some("tmpfs"),
            MsFlags::empty(),
            Some("size=64K,mode=755"),
        )
        .map_err(|e| SandboxError::Mount(format!("mount /dev: {}", e)))?;
        std::fs::create_dir_all(&devpts_dir)?;

        Self::create_dev_nodes(&dev_dir)?;

        let old_root = overlay.merged_dir.join("old_root");
        std::fs::create_dir_all(&old_root)?;

        nix::unistd::pivot_root(&overlay.merged_dir, &old_root).map_err(|e| {
            SandboxError::Mount(format!("pivot_root: {}", e))
        })?;

        std::env::set_current_dir("/").map_err(|e| {
            SandboxError::Mount(format!("chdir /: {}", e))
        })?;

        nix::mount::umount2("/old_root", nix::mount::MntFlags::MNT_DETACH).map_err(|e| {
            SandboxError::Mount(format!("umount old_root: {}", e))
        })?;
        std::fs::remove_dir("/old_root").ok();

        Ok(())
    }

    fn create_dev_nodes(dev_dir: &Path) -> Result<(), SandboxError> {
        use nix::sys::stat;
        use std::os::unix::fs::symlink;

        // mknod may fail without CAP_MKNOD — that's fine, .ok() ignores errors
        for (name, minor) in [("null", 3), ("zero", 5), ("random", 8), ("urandom", 9)] {
            nix::sys::stat::mknod(
                &dev_dir.join(name),
                stat::SFlag::S_IFCHR,
                stat::Mode::from_bits_truncate(0o666),
                nix::sys::stat::makedev(1, minor),
            )
            .ok();
        }

        symlink("/proc/self/fd/0", dev_dir.join("stdin")).ok();
        symlink("/proc/self/fd/1", dev_dir.join("stdout")).ok();
        symlink("/proc/self/fd/2", dev_dir.join("stderr")).ok();
        symlink("/proc/self/fd", dev_dir.join("fd")).ok();

        Ok(())
    }

    /// Resolve a command name to an absolute path by searching PATH directories.
    fn resolve_command(cmd: &CString) -> Option<CString> {
        let cmd_str = cmd.to_str().ok()?;
        if cmd_str.starts_with('/') {
            return None;
        }
        let search_dirs = [
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/usr/local/sbin",
            "/usr/sbin",
            "/sbin",
        ];
        for dir in &search_dirs {
            let full = format!("{}/{}", dir, cmd_str);
            if std::path::Path::new(&full).exists() {
                return CString::new(full).ok();
            }
        }
        None
    }

    fn print_dry_run(&self, command: &[String], env: &HashMap<String, String>) {
        println!("=== Dry Run ===");
        println!("Session: {}", self.session.display_name());
        println!("Project: {}", self.session.project_dir.display());
        println!("Command: {:?}", command);
        println!("Adapter: {}", self.session.adapter);
        println!("Network: {}", self.config.network.mode);
        println!("Memory:  {}", self.config.resources.memory);
        println!("CPU:     {}", self.config.resources.cpu);
        println!("Persist: {}", self.session.persist);
        if !env.is_empty() {
            println!("Env vars: {:?}", env.keys().collect::<Vec<_>>());
        }
        println!("\nCapabilities:");
        println!("  user namespaces: {}", self.capabilities.user_namespaces);
        println!("  overlayfs:       {}", self.capabilities.overlayfs);
        println!("  cgroups v2:      {}", self.capabilities.cgroups_v2);
        println!("  iptables:        {}", self.capabilities.iptables);
        println!("  ip command:      {}", self.capabilities.ip_command);
    }
}
