use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::error::SandboxError;

/// Set up cgroup v2 resource limits for a sandbox.
pub struct CgroupSetup;

impl CgroupSetup {
    /// Create a cgroup for the session and apply resource limits.
    /// Returns the cgroup path.
    pub fn create(
        session_id: &str,
        memory_bytes: u64,
        cpu_quota: u64,
        cpu_period: u64,
        max_pids: u64,
    ) -> Result<PathBuf, SandboxError> {
        let cgroup_base = PathBuf::from("/sys/fs/cgroup");
        let cgroup_path = cgroup_base.join(format!("cbox_{}", session_id));

        fs::create_dir_all(&cgroup_path).map_err(|e| {
            SandboxError::Cgroup(format!(
                "failed to create cgroup {}: {}",
                cgroup_path.display(),
                e
            ))
        })?;

        // Memory limit
        let mem_max = cgroup_path.join("memory.max");
        if let Err(e) = fs::write(&mem_max, memory_bytes.to_string()) {
            warn!("failed to set memory.max: {}", e);
        } else {
            debug!("cgroup memory.max = {} bytes", memory_bytes);
        }

        // CPU limit
        let cpu_max = cgroup_path.join("cpu.max");
        let cpu_value = format!("{} {}", cpu_quota, cpu_period);
        if let Err(e) = fs::write(&cpu_max, &cpu_value) {
            warn!("failed to set cpu.max: {}", e);
        } else {
            debug!("cgroup cpu.max = {}", cpu_value);
        }

        // PID limit
        let pids_max = cgroup_path.join("pids.max");
        if let Err(e) = fs::write(&pids_max, max_pids.to_string()) {
            warn!("failed to set pids.max: {}", e);
        } else {
            debug!("cgroup pids.max = {}", max_pids);
        }

        info!("cgroup created at {}", cgroup_path.display());
        Ok(cgroup_path)
    }

    /// Move a process into the cgroup.
    pub fn add_process(cgroup_path: &Path, pid: u32) -> Result<(), SandboxError> {
        let procs_file = cgroup_path.join("cgroup.procs");
        fs::write(&procs_file, pid.to_string()).map_err(|e| {
            SandboxError::Cgroup(format!("failed to add pid {} to cgroup: {}", pid, e))
        })?;
        debug!("added pid {} to cgroup {}", pid, cgroup_path.display());
        Ok(())
    }

    /// Remove the cgroup directory.
    pub fn cleanup(cgroup_path: &Path) -> Result<(), SandboxError> {
        if cgroup_path.exists() {
            // Move remaining processes to parent first
            let procs_file = cgroup_path.join("cgroup.procs");
            if let Ok(contents) = fs::read_to_string(&procs_file) {
                let parent_procs = cgroup_path
                    .parent()
                    .unwrap_or(Path::new("/sys/fs/cgroup"))
                    .join("cgroup.procs");
                for line in contents.lines() {
                    let _ = fs::write(&parent_procs, line);
                }
            }

            if let Err(e) = fs::remove_dir(cgroup_path) {
                warn!("failed to remove cgroup {}: {}", cgroup_path.display(), e);
            } else {
                info!("cgroup removed: {}", cgroup_path.display());
            }
        }
        Ok(())
    }
}
