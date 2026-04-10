use std::process::{Command, Stdio};

use crate::log;

/// Invoke real ssh with the matched key injected.
/// Returns the exit code from ssh.
pub fn invoke_ssh(
    original_args: &[String],
    key_path: &str,
    has_identities_only: bool,
    port: Option<u16>,
) -> Result<i32, String> {
    let ssh_args = build_ssh_args(original_args, key_path, has_identities_only, port);

    log::debug(&format!("Invoking: ssh {}", ssh_args.join(" ")));

    let status = Command::new("ssh")
        .args(&ssh_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| format!("Failed to invoke ssh: {}", e))?;

    Ok(status.code().unwrap_or(1))
}

fn build_ssh_args(
    original_args: &[String],
    key_path: &str,
    has_identities_only: bool,
    port: Option<u16>,
) -> Vec<String> {
    let mut ssh_args: Vec<String> = Vec::new();

    // Inject -i <key> and force agent-off so only the selected key is offered.
    ssh_args.push("-i".to_string());
    ssh_args.push(key_path.to_string());

    if !has_identity_agent_flag(original_args) {
        ssh_args.push("-o".to_string());
        ssh_args.push("IdentityAgent=none".to_string());
    }

    if !has_identities_only {
        ssh_args.push("-o".to_string());
        ssh_args.push("IdentitiesOnly=yes".to_string());
    }

    // Inject port if configured and not already in args
    if let Some(p) = port {
        if !has_port_flag(original_args) {
            ssh_args.push("-p".to_string());
            ssh_args.push(p.to_string());
        }
    }

    // Append all original args
    ssh_args.extend_from_slice(original_args);
    ssh_args
}

/// Check if the original args already contain IdentitiesOnly.
pub fn has_identities_only(args: &[String]) -> bool {
    for (i, arg) in args.iter().enumerate() {
        if arg == "-o" {
            if let Some(next) = args.get(i + 1) {
                if next.starts_with("IdentitiesOnly") {
                    return true;
                }
            }
        }
        if arg.starts_with("-oIdentitiesOnly") {
            return true;
        }
    }
    false
}

/// Check if the original args already contain an IdentityAgent option.
fn has_identity_agent_flag(args: &[String]) -> bool {
    for (i, arg) in args.iter().enumerate() {
        if arg == "-o" {
            if let Some(next) = args.get(i + 1) {
                if next.starts_with("IdentityAgent=") {
                    return true;
                }
            }
        }
        if arg.starts_with("-oIdentityAgent=") {
            return true;
        }
    }
    false
}

/// Check if the original args already contain a -p port flag.
pub fn has_port_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "-p")
}

/// Invoke ssh in passthrough mode (no key injection).
pub fn passthrough_ssh(original_args: &[String]) -> Result<i32, String> {
    log::debug(&format!("Passthrough: ssh {}", original_args.join(" ")));

    let status = Command::new("ssh")
        .args(original_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| format!("Failed to invoke ssh: {}", e))?;

    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_identity_agent_none_by_default() {
        let original_args = vec![
            "git@github.com".to_string(),
            "git-upload-pack".to_string(),
            "Org/repo.git".to_string(),
        ];

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", false, None);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=none"));
    }

    #[test]
    fn preserves_existing_identity_agent_flag() {
        let original_args = vec![
            "-o".to_string(),
            "IdentityAgent=/tmp/agent.sock".to_string(),
            "git@github.com".to_string(),
            "git-upload-pack".to_string(),
            "Org/repo.git".to_string(),
        ];

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", false, None);

        let none_count = final_args
            .windows(2)
            .filter(|w| w[0] == "-o" && w[1] == "IdentityAgent=none")
            .count();
        assert_eq!(none_count, 0);
    }

    #[test]
    fn preserve_explicit_port_443_from_original_args() {
        let original_args = vec![
            "-p".to_string(),
            "443".to_string(),
            "git@ssh.github.com".to_string(),
            "git-receive-pack".to_string(),
            "Org/repo.git".to_string(),
        ];

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", false, Some(22));

        let p_count = final_args.iter().filter(|a| a.as_str() == "-p").count();
        assert_eq!(p_count, 1);

        let p_pos = final_args.iter().position(|a| a == "-p").unwrap();
        assert_eq!(final_args[p_pos + 1], "443");
        assert!(final_args.iter().any(|a| a == "git@ssh.github.com"));
    }
}
