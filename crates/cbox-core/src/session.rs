use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::backend::BackendKind;
use crate::error::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    Stopped,
    Saved,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Saved => write!(f, "saved"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: Option<String>,
    pub status: SessionStatus,
    pub pid: Option<u32>,
    pub project_dir: PathBuf,
    pub created_at: DateTime<Utc>,
    pub adapter: String,
    pub persist: bool,

    /// Network rules applied (for cleanup)
    #[serde(default)]
    pub iptables_rules: Vec<String>,

    /// Veth interface name on the host side
    pub veth_host: Option<String>,

    /// Cgroup path
    pub cgroup_path: Option<String>,

    /// Subnet index used for veth addressing
    pub subnet_index: Option<u8>,

    /// Which backend was used to create this session.
    #[serde(default)]
    pub backend: BackendKind,

    /// Container runtime used (e.g. "docker", "podman").
    #[serde(default)]
    pub container_runtime: Option<String>,
}

impl Session {
    pub fn new(
        project_dir: PathBuf,
        name: Option<String>,
        adapter: String,
        persist: bool,
        backend: BackendKind,
    ) -> Self {
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        Self {
            id,
            name,
            status: SessionStatus::Running,
            pid: None,
            project_dir,
            created_at: Utc::now(),
            adapter,
            persist,
            iptables_rules: vec![],
            veth_host: None,
            cgroup_path: None,
            subnet_index: None,
            backend,
            container_runtime: None,
        }
    }

    /// Directory for this session's data (overlay dirs, metadata).
    pub fn session_dir(&self) -> PathBuf {
        SessionStore::base_dir().join(&self.id)
    }

    pub fn upper_dir(&self) -> PathBuf {
        self.session_dir().join("upper")
    }

    pub fn work_dir(&self) -> PathBuf {
        self.session_dir().join("work")
    }

    pub fn merged_dir(&self) -> PathBuf {
        self.session_dir().join("merged")
    }

    /// Display name: use name if set, else id.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// Manages session persistence on disk.
pub struct SessionStore;

impl SessionStore {
    pub fn base_dir() -> PathBuf {
        let data_dir = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                PathBuf::from(home).join(".local/share")
            });
        data_dir.join("cbox/sessions")
    }

    pub fn ensure_dir() -> Result<(), CoreError> {
        fs::create_dir_all(Self::base_dir())?;
        Ok(())
    }

    fn session_file(id: &str) -> PathBuf {
        Self::base_dir().join(id).join("session.json")
    }

    pub fn save(session: &Session) -> Result<(), CoreError> {
        let dir = Self::base_dir().join(&session.id);
        fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(session)?;
        fs::write(Self::session_file(&session.id), json)?;
        Ok(())
    }

    pub fn load(id: &str) -> Result<Session, CoreError> {
        let path = Self::session_file(id);
        if !path.exists() {
            return Err(CoreError::SessionNotFound(id.to_string()));
        }
        let json = fs::read_to_string(&path)?;
        let session: Session = serde_json::from_str(&json)?;
        Ok(session)
    }

    /// Find a session by id prefix or name.
    pub fn find(query: &str) -> Result<Session, CoreError> {
        let sessions = Self::list_all()?;
        // Exact name match first
        if let Some(s) = sessions.iter().find(|s| s.name.as_deref() == Some(query)) {
            return Ok(s.clone());
        }
        // ID prefix match
        let matches: Vec<_> = sessions
            .iter()
            .filter(|s| s.id.starts_with(query))
            .collect();
        match matches.len() {
            0 => Err(CoreError::SessionNotFound(query.to_string())),
            1 => Ok(matches[0].clone()),
            _ => Err(CoreError::Config(format!(
                "ambiguous session query '{}': {} matches",
                query,
                matches.len()
            ))),
        }
    }

    pub fn list_all() -> Result<Vec<Session>, CoreError> {
        let base = Self::base_dir();
        if !base.exists() {
            return Ok(vec![]);
        }
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&base)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let session_file = entry.path().join("session.json");
                if session_file.exists() {
                    let json = fs::read_to_string(&session_file)?;
                    if let Ok(session) = serde_json::from_str::<Session>(&json) {
                        sessions.push(session);
                    }
                }
            }
        }
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(sessions)
    }

    pub fn delete(id: &str) -> Result<(), CoreError> {
        let dir = Self::base_dir().join(id);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Check if a session's process is still alive.
    pub fn is_alive(session: &Session) -> bool {
        if let Some(pid) = session.pid {
            // Check /proc/<pid> exists
            Path::new(&format!("/proc/{}", pid)).exists()
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = Session::new(
            PathBuf::from("/tmp/test"),
            Some("test-session".to_string()),
            "generic".to_string(),
            false,
            BackendKind::Native,
        );
        assert_eq!(session.name.as_deref(), Some("test-session"));
        assert_eq!(session.display_name(), "test-session");
        assert_eq!(session.status, SessionStatus::Running);
        assert_eq!(session.id.len(), 8);
    }

    #[test]
    fn test_session_dirs() {
        let session = Session::new(
            PathBuf::from("/tmp/test"),
            None,
            "generic".to_string(),
            false,
            BackendKind::Native,
        );
        let session_dir = session.session_dir();
        assert!(session_dir.ends_with(&session.id));
        assert!(session.upper_dir().ends_with("upper"));
        assert!(session.work_dir().ends_with("work"));
        assert!(session.merged_dir().ends_with("merged"));
    }
}
