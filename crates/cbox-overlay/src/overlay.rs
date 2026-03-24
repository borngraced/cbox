use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use cbox_core::Session;
use tracing::{debug, info};
use walkdir::WalkDir;

use crate::error::OverlayError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

impl std::fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Added => write!(f, "A"),
            Self::Modified => write!(f, "M"),
            Self::Deleted => write!(f, "D"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OverlayChange {
    pub kind: ChangeKind,
    /// Path relative to the project root.
    pub path: PathBuf,
    /// Absolute path in the upper dir (for reading new content).
    pub upper_path: PathBuf,
}

pub struct OverlayFs {
    pub lower_dir: PathBuf,
    pub upper_dir: PathBuf,
    pub work_dir: PathBuf,
    pub merged_dir: PathBuf,
}

impl OverlayFs {
    pub fn from_session(session: &Session) -> Self {
        Self {
            lower_dir: session.project_dir.clone(),
            upper_dir: session.upper_dir(),
            work_dir: session.work_dir(),
            merged_dir: session.merged_dir(),
        }
    }

    /// Create the overlay directory structure.
    pub fn setup(&self) -> Result<(), OverlayError> {
        for dir in [&self.upper_dir, &self.work_dir, &self.merged_dir] {
            fs::create_dir_all(dir).map_err(|e| {
                OverlayError::Setup(format!("failed to create {}: {}", dir.display(), e))
            })?;
        }
        info!(
            "overlay dirs created: upper={}, work={}, merged={}",
            self.upper_dir.display(),
            self.work_dir.display(),
            self.merged_dir.display()
        );
        Ok(())
    }

    /// Mount the overlayfs. Must be called inside a mount namespace.
    #[cfg(target_os = "linux")]
    pub fn mount(&self) -> Result<(), OverlayError> {
        let opts = format!(
            "lowerdir={},upperdir={},workdir={}",
            self.lower_dir.display(),
            self.upper_dir.display(),
            self.work_dir.display()
        );

        nix::mount::mount(
            Some("overlay"),
            &self.merged_dir,
            Some("overlay"),
            nix::mount::MsFlags::empty(),
            Some(opts.as_str()),
        )
        .map_err(|e| OverlayError::Mount(format!("mount overlay: {}", e)))?;

        info!("overlayfs mounted at {}", self.merged_dir.display());
        Ok(())
    }

    /// Unmount the overlayfs.
    #[cfg(target_os = "linux")]
    pub fn unmount(&self) -> Result<(), OverlayError> {
        if self.merged_dir.exists() {
            nix::mount::umount(&self.merged_dir)
                .map_err(|e| OverlayError::Unmount(format!("umount: {}", e)))?;
            info!("overlayfs unmounted from {}", self.merged_dir.display());
        }
        Ok(())
    }

    /// Walk the upper directory and detect changes relative to the lower (project) dir.
    pub fn diff(&self) -> Result<Vec<OverlayChange>, OverlayError> {
        let mut changes = Vec::new();

        if !self.upper_dir.exists() {
            return Ok(changes);
        }

        for entry in WalkDir::new(&self.upper_dir)
            .min_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let upper_path = entry.path().to_path_buf();
            let rel_path = upper_path
                .strip_prefix(&self.upper_dir)
                .map_err(|e| OverlayError::Diff(e.to_string()))?
                .to_path_buf();

            // Skip .wh.* directories, but NOT .wh..wh..opq (opaque dir sentinel)
            // which is checked later by is_opaque_dir()
            if entry.file_type().is_dir() {
                if let Some(name) = rel_path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(".wh.") && name != ".wh..wh..opq" {
                        continue;
                    }
                }
            }

            let lower_path = self.lower_dir.join(&rel_path);

            let kind = if Self::is_whiteout(&upper_path) {
                // Whiteout file = deletion
                let real_name = Self::whiteout_to_real_name(&rel_path);
                changes.push(OverlayChange {
                    kind: ChangeKind::Deleted,
                    path: real_name,
                    upper_path: upper_path.clone(),
                });
                continue;
            } else if entry.file_type().is_dir() {
                // Directories in upper are not changes themselves unless opaque
                if Self::is_opaque_dir(&upper_path) {
                    ChangeKind::Modified
                } else {
                    continue;
                }
            } else if lower_path.exists() {
                ChangeKind::Modified
            } else {
                ChangeKind::Added
            };

            debug!("{} {}", kind, rel_path.display());
            changes.push(OverlayChange {
                kind,
                path: rel_path,
                upper_path,
            });
        }

        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    /// Merge changes from upper dir into the lower (project) dir.
    pub fn merge(&self, changes: &[OverlayChange]) -> Result<(), OverlayError> {
        for change in changes {
            let target = self.lower_dir.join(&change.path);
            match change.kind {
                ChangeKind::Added | ChangeKind::Modified => {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    if change.upper_path.is_file() {
                        fs::copy(&change.upper_path, &target).map_err(|e| {
                            OverlayError::Merge(format!(
                                "copy {} -> {}: {}",
                                change.upper_path.display(),
                                target.display(),
                                e
                            ))
                        })?;
                        // Preserve permissions
                        let meta = fs::metadata(&change.upper_path)?;
                        let perms = meta.permissions();
                        fs::set_permissions(&target, perms)?;
                    }
                    info!("{} {}", change.kind, change.path.display());
                }
                ChangeKind::Deleted => {
                    if target.is_dir() {
                        fs::remove_dir_all(&target).map_err(|e| {
                            OverlayError::Merge(format!("rm dir {}: {}", target.display(), e))
                        })?;
                    } else if target.exists() {
                        fs::remove_file(&target).map_err(|e| {
                            OverlayError::Merge(format!("rm {}: {}", target.display(), e))
                        })?;
                    }
                    info!("D {}", change.path.display());
                }
            }
        }
        Ok(())
    }

    /// Clean up all overlay directories.
    pub fn cleanup(&self) -> Result<(), OverlayError> {
        // Try to unmount first (may already be unmounted)
        #[cfg(target_os = "linux")]
        {
            let _ = self.unmount();
        }

        for dir in [&self.merged_dir, &self.work_dir, &self.upper_dir] {
            if dir.exists() {
                fs::remove_dir_all(dir).map_err(|e| {
                    OverlayError::Setup(format!("cleanup {}: {}", dir.display(), e))
                })?;
            }
        }
        Ok(())
    }

    /// Check if a file is an overlayfs whiteout (char device 0,0).
    fn is_whiteout(path: &Path) -> bool {
        if let Ok(meta) = fs::symlink_metadata(path) {
            // Whiteout files are character devices with major=0, minor=0
            use std::os::unix::fs::FileTypeExt;
            if meta.file_type().is_char_device() {
                return meta.rdev() == 0;
            }
        }
        // Also check for .wh. prefix naming convention
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with(".wh."))
            .unwrap_or(false)
    }

    /// Check if a directory has the opaque xattr (entire directory replaced).
    fn is_opaque_dir(path: &Path) -> bool {
        // Check for .wh..wh..opq file
        path.join(".wh..wh..opq").exists()
    }

    /// Convert a whiteout filename back to the real filename.
    fn whiteout_to_real_name(rel_path: &Path) -> PathBuf {
        if let Some(name) = rel_path.file_name().and_then(|n| n.to_str()) {
            if let Some(real_name) = name.strip_prefix(".wh.") {
                if let Some(parent) = rel_path.parent() {
                    return parent.join(real_name);
                }
                return PathBuf::from(real_name);
            }
        }
        rel_path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dirs() -> (tempfile::TempDir, OverlayFs) {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();

        let lower = base.join("lower");
        let upper = base.join("upper");
        let work = base.join("work");
        let merged = base.join("merged");

        fs::create_dir_all(&lower).unwrap();
        fs::create_dir_all(&upper).unwrap();
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(&merged).unwrap();

        // Create some files in lower
        fs::write(lower.join("existing.txt"), "original content").unwrap();
        fs::create_dir_all(lower.join("subdir")).unwrap();
        fs::write(lower.join("subdir/nested.txt"), "nested content").unwrap();

        let overlay = OverlayFs {
            lower_dir: lower,
            upper_dir: upper,
            work_dir: work,
            merged_dir: merged,
        };

        (tmp, overlay)
    }

    #[test]
    fn test_diff_added_file() {
        let (_tmp, overlay) = setup_test_dirs();

        // Simulate adding a new file in upper
        fs::write(overlay.upper_dir.join("new_file.txt"), "new content").unwrap();

        let changes = overlay.diff().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Added);
        assert_eq!(changes[0].path, PathBuf::from("new_file.txt"));
    }

    #[test]
    fn test_diff_modified_file() {
        let (_tmp, overlay) = setup_test_dirs();

        // Simulate modifying an existing file
        fs::write(overlay.upper_dir.join("existing.txt"), "modified content").unwrap();

        let changes = overlay.diff().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Modified);
    }

    #[test]
    fn test_diff_deleted_file_wh_prefix() {
        let (_tmp, overlay) = setup_test_dirs();

        // Simulate deletion via .wh. prefix
        fs::write(overlay.upper_dir.join(".wh.existing.txt"), "").unwrap();

        let changes = overlay.diff().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Deleted);
        assert_eq!(changes[0].path, PathBuf::from("existing.txt"));
    }

    #[test]
    fn test_merge_added_file() {
        let (_tmp, overlay) = setup_test_dirs();

        let new_content = "brand new file";
        fs::write(overlay.upper_dir.join("added.txt"), new_content).unwrap();

        let changes = overlay.diff().unwrap();
        overlay.merge(&changes).unwrap();

        let merged = fs::read_to_string(overlay.lower_dir.join("added.txt")).unwrap();
        assert_eq!(merged, new_content);
    }

    #[test]
    fn test_merge_modified_file() {
        let (_tmp, overlay) = setup_test_dirs();

        fs::write(overlay.upper_dir.join("existing.txt"), "updated").unwrap();

        let changes = overlay.diff().unwrap();
        overlay.merge(&changes).unwrap();

        let content = fs::read_to_string(overlay.lower_dir.join("existing.txt")).unwrap();
        assert_eq!(content, "updated");
    }

    #[test]
    fn test_whiteout_name_conversion() {
        assert_eq!(
            OverlayFs::whiteout_to_real_name(&PathBuf::from(".wh.foo.txt")),
            PathBuf::from("foo.txt")
        );
        assert_eq!(
            OverlayFs::whiteout_to_real_name(&PathBuf::from("subdir/.wh.bar.txt")),
            PathBuf::from("subdir/bar.txt")
        );
    }

    #[test]
    fn test_empty_diff() {
        let (_tmp, overlay) = setup_test_dirs();
        let changes = overlay.diff().unwrap();
        assert!(changes.is_empty());
    }
}
