use std::process::Command;

/// Return a short description of the first key loaded in the agent,
/// to inform the user what SSH will default to in passthrough mode.
/// Format: "~/.ssh/id_rsa (RSA)" or the comment field from ssh-add -l.
pub fn default_key_hint() -> Option<String> {
    let output = Command::new("ssh-add").arg("-l").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // ssh-add -l format: "2048 SHA256:xxx comment (TYPE)"
    // We want the comment (typically a path) and the type.
    let first_line = stdout.lines().next()?;
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() >= 4 {
        let comment = parts[2];
        let key_type = parts[parts.len() - 1]; // "(RSA)" etc
        Some(format!("{} {}", comment, key_type))
    } else {
        None
    }
}
