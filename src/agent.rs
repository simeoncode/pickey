use std::path::Path;
use std::process::Command;

use crate::log;

/// Check if a key is loaded in the ssh-agent by comparing fingerprints.
pub fn is_key_loaded(key_path: &Path) -> Result<bool, String> {
    // Get fingerprint of our key
    let key_fp = key_fingerprint(key_path)?;

    // Get list of loaded keys
    let output = Command::new("ssh-add")
        .arg("-l")
        .output()
        .map_err(|e| format!("Failed to run ssh-add -l: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // "The agent has no identities." is exit code 1 but not an error
        if stderr.contains("no identities") || stderr.contains("Could not open") {
            return Ok(false);
        }
        // Agent might not be running
        if stderr.contains("Could not open a connection") {
            return Err("ssh-agent is not running".to_string());
        }
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.contains(&key_fp))
}

/// Get the SHA256 fingerprint of a key file.
fn key_fingerprint(key_path: &Path) -> Result<String, String> {
    let output = Command::new("ssh-keygen")
        .args(["-lf", &key_path.to_string_lossy()])
        .output()
        .map_err(|e| format!("Failed to run ssh-keygen: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "ssh-keygen failed for {}: {}",
            key_path.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "256 SHA256:xxxxx comment (type)"
    // Extract the fingerprint hash
    stdout
        .split_whitespace()
        .nth(1)
        .map(String::from)
        .ok_or_else(|| {
            format!(
                "Could not parse fingerprint from ssh-keygen output: {}",
                stdout
            )
        })
}

/// Load a key into the ssh-agent.
pub fn load_key(key_path: &Path, apple_keychain: bool) -> Result<(), String> {
    let mut cmd = Command::new("ssh-add");

    if cfg!(target_os = "macos") && apple_keychain {
        cmd.arg("--apple-use-keychain");
    }

    cmd.arg(key_path);

    log::debug(&format!("Running: ssh-add {}", key_path.display()));

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run ssh-add: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "ssh-add failed for {}: {}",
            key_path.display(),
            stderr.trim()
        ));
    }

    Ok(())
}

/// Ensure a key is loaded in the agent. Returns whether it was already loaded.
pub fn ensure_key_loaded(key_path: &Path, apple_keychain: bool) -> Result<bool, String> {
    match is_key_loaded(key_path) {
        Ok(true) => Ok(true),
        Ok(false) => {
            load_key(key_path, apple_keychain)?;
            Ok(false)
        }
        Err(e) if e.contains("not running") => {
            log::warn("ssh-agent is not running, attempting to load key anyway");
            load_key(key_path, apple_keychain)?;
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

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
