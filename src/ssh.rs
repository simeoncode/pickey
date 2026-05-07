use std::process::{Command, Stdio};

use crate::log;

/// Invoke real ssh with the matched key injected.
/// Returns the exit code from ssh.
pub fn invoke_ssh(
    original_args: &[String],
    key_path: &str,
    has_identities_only: bool,
    port: Option<u16>,
    use_macos_keychain: bool,
) -> Result<i32, String> {
    let ssh_args = build_ssh_args(
        original_args,
        key_path,
        has_identities_only,
        port,
        use_macos_keychain,
    );

    let ssh_program = ssh_program(use_macos_keychain);

    log::debug(&format!("Invoking: {} {}", ssh_program, ssh_args.join(" ")));

    let status = Command::new(ssh_program)
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
    use_macos_keychain: bool,
) -> Vec<String> {
    let mut ssh_args: Vec<String> = Vec::new();

    // Inject -i <key> so the selected key is always offered.
    ssh_args.push("-i".to_string());
    ssh_args.push(key_path.to_string());

    if !has_identity_agent_flag(original_args) {
        ssh_args.push("-o".to_string());
        if should_use_keychain(use_macos_keychain) {
            // Keep agent connected so Keychain can supply passphrases.
            let agent_sock = std::env::var("SSH_AUTH_SOCK").unwrap_or_default();
            if agent_sock.is_empty() {
                ssh_args.push("IdentityAgent=none".to_string());
            } else {
                ssh_args.push(format!("IdentityAgent={}", agent_sock));
            }
        } else {
            ssh_args.push("IdentityAgent=none".to_string());
        }
    }

    if !has_identities_only {
        ssh_args.push("-o".to_string());
        ssh_args.push("IdentitiesOnly=yes".to_string());
    }

    if should_use_keychain(use_macos_keychain) {
        if !has_use_keychain_flag(original_args) {
            ssh_args.push("-o".to_string());
            ssh_args.push("UseKeychain=yes".to_string());
        }
        if !has_add_keys_to_agent_flag(original_args) {
            ssh_args.push("-o".to_string());
            ssh_args.push("AddKeysToAgent=yes".to_string());
        }
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

fn should_use_keychain(use_macos_keychain: bool) -> bool {
    use_macos_keychain && cfg!(target_os = "macos")
}

fn ssh_program(use_macos_keychain: bool) -> &'static str {
    if should_use_keychain(use_macos_keychain) {
        "/usr/bin/ssh"
    } else {
        "ssh"
    }
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

/// Check if the original args already contain a UseKeychain option.
fn has_use_keychain_flag(args: &[String]) -> bool {
    has_ssh_option(args, "UseKeychain=")
}

/// Check if the original args already contain an AddKeysToAgent option.
fn has_add_keys_to_agent_flag(args: &[String]) -> bool {
    has_ssh_option(args, "AddKeysToAgent=")
}

/// Check if the original args already contain a given -o option prefix.
fn has_ssh_option(args: &[String], prefix: &str) -> bool {
    let compact = format!("-o{}", prefix);
    for (i, arg) in args.iter().enumerate() {
        if arg == "-o" {
            if let Some(next) = args.get(i + 1) {
                if next.starts_with(prefix) {
                    return true;
                }
            }
        }
        if arg.starts_with(&compact) {
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

    fn sample_args() -> Vec<String> {
        vec![
            "git@github.com".to_string(),
            "git-upload-pack".to_string(),
            "Org/repo.git".to_string(),
        ]
    }

    #[test]
    fn injects_identity_agent_none_when_keychain_disabled() {
        let args = sample_args();
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", false, None, false);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=none"));
    }

    #[test]
    fn always_injects_identities_only() {
        let args = sample_args();
        // With keychain
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", false, None, true);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentitiesOnly=yes"));
        // Without keychain
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", false, None, false);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentitiesOnly=yes"));
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

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", false, None, false);
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

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", false, Some(22), false);

        let p_count = final_args.iter().filter(|a| a.as_str() == "-p").count();
        assert_eq!(p_count, 1);

        let p_pos = final_args.iter().position(|a| a == "-p").unwrap();
        assert_eq!(final_args[p_pos + 1], "443");
        assert!(final_args.iter().any(|a| a == "git@ssh.github.com"));
    }

    #[test]
    fn does_not_duplicate_use_keychain_flag() {
        let original_args = vec![
            "-o".to_string(),
            "UseKeychain=no".to_string(),
            "git@github.com".to_string(),
            "git-upload-pack".to_string(),
            "Org/repo.git".to_string(),
        ];

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", false, None, true);
        let count = final_args
            .windows(2)
            .filter(|w| w[0] == "-o" && w[1].starts_with("UseKeychain="))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn does_not_duplicate_add_keys_to_agent_flag() {
        let original_args = vec![
            "-o".to_string(),
            "AddKeysToAgent=no".to_string(),
            "git@github.com".to_string(),
            "git-upload-pack".to_string(),
            "Org/repo.git".to_string(),
        ];

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", false, None, true);
        let count = final_args
            .windows(2)
            .filter(|w| w[0] == "-o" && w[1].starts_with("AddKeysToAgent="))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn keychain_disabled_no_use_keychain_or_add_keys() {
        let args = sample_args();
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", false, None, false);
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1].starts_with("UseKeychain=")));
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1].starts_with("AddKeysToAgent=")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keychain_injects_all_flags() {
        let args = sample_args();
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", false, None, true);

        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "UseKeychain=yes"));
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "AddKeysToAgent=yes"));
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentitiesOnly=yes"));
        // Should NOT have IdentityAgent=none
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=none"));
        assert_eq!(ssh_program(true), "/usr/bin/ssh");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keychain_uses_ssh_auth_sock() {
        let args = sample_args();
        // Set a known value for the test
        std::env::set_var("SSH_AUTH_SOCK", "/tmp/test-agent.sock");
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", false, None, true);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=/tmp/test-agent.sock"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keychain_falls_back_when_no_auth_sock() {
        let args = sample_args();
        std::env::remove_var("SSH_AUTH_SOCK");
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", false, None, true);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=none"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_opt_out_falls_back_to_agent_off() {
        let args = sample_args();
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", false, None, false);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=none"));
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1].starts_with("UseKeychain=")));
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1].starts_with("AddKeysToAgent=")));
        assert_eq!(ssh_program(false), "ssh");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_ignores_keychain_setting() {
        let args = sample_args();
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", false, None, true);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=none"));
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1].starts_with("UseKeychain=")));
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1].starts_with("AddKeysToAgent=")));
        assert_eq!(ssh_program(true), "ssh");
    }
}
