/// Resolve the real user's home directory.
///
/// When running under `sudo`, `HOME` points to `/root`, but we need the
/// invoking user's home. Falls back to `HOME` or `/root` if `SUDO_USER`
/// is not set.
pub fn real_user_home() -> String {
    std::env::var("SUDO_USER")
        .ok()
        .and_then(|user| {
            std::fs::read_to_string("/etc/passwd")
                .ok()?
                .lines()
                .find(|l| l.starts_with(&format!("{}:", user)))
                .and_then(|l| l.split(':').nth(5))
                .map(String::from)
        })
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/root".to_string()))
}
