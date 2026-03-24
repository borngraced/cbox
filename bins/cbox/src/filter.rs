use cbox_overlay::OverlayChange;

/// Filter out changes matching any of the exclude glob patterns.
pub fn filter_excluded(changes: Vec<OverlayChange>, exclude: &[String]) -> Vec<OverlayChange> {
    if exclude.is_empty() {
        return changes;
    }
    changes
        .into_iter()
        .filter(|change| {
            let path_str = change.path.to_string_lossy();
            !exclude.iter().any(|pattern| glob_match(pattern, &path_str))
        })
        .collect()
}

/// Simple glob matching supporting `*` (single segment) and `**` (any depth).
fn glob_match(pattern: &str, path: &str) -> bool {
    if pattern == path {
        return true;
    }

    if let Some(prefix) = pattern.strip_suffix("/**") {
        if path.starts_with(prefix) && path.len() > prefix.len() && path.as_bytes()[prefix.len()] == b'/' {
            return true;
        }
        if path == prefix {
            return true;
        }
    }

    if pattern.contains('*') && !pattern.contains("**") {
        return glob_simple(pattern, path);
    }

    false
}

/// Match a simple glob with `*` wildcards (no directory traversal).
fn glob_simple(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }

    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match text[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 {
                    return false; // First part must match at start
                }
                pos += idx + part.len();
            }
            None => return false,
        }
    }

    if !pattern.ends_with('*') {
        return pos == text.len();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_exact() {
        assert!(glob_match("root/.bash_history", "root/.bash_history"));
        assert!(!glob_match("root/.bash_history", "root/.zsh_history"));
    }

    #[test]
    fn test_glob_double_star() {
        assert!(glob_match("root/.cache/**", "root/.cache/helix/helix.log"));
        assert!(glob_match("root/.cache/**", "root/.cache/foo"));
        assert!(!glob_match("root/.cache/**", "root/.config/foo"));
    }

    #[test]
    fn test_glob_single_star() {
        assert!(glob_match("*.log", "debug.log"));
        assert!(glob_match(".viminfo", ".viminfo"));
        assert!(!glob_match(".viminfo", "root/.viminfo"));
    }

    #[test]
    fn test_glob_home_exclude() {
        // home/** should match all user home artifacts from overlay diff
        assert!(glob_match("home/**", "home/borngraced/.cache/claude-cli-nodejs/-/mcp-logs/test.jsonl"));
        assert!(glob_match("home/**", "home/borngraced/.claude.json"));
        assert!(glob_match("home/**", "home/borngraced/.npm/_logs/debug.log"));
        assert!(glob_match("home/**", "home/user/.cache/keyring/control"));
        // Should NOT match project files
        assert!(!glob_match("home/**", "src/main.rs"));
        assert!(!glob_match("home/**", "Cargo.toml"));
    }
}
