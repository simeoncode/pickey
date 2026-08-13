use std::process::{Command, Stdio};

use crate::log;

/// Invoke real ssh with the matched key injected.
/// Returns the exit code from ssh.
pub fn invoke_ssh(
    original_args: &[String],
    key_path: &str,
    port: Option<u16>,
    use_macos_keychain: bool,
) -> Result<i32, String> {
    let ssh_args = build_ssh_args(original_args, key_path, port, use_macos_keychain);

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

pub(crate) fn build_ssh_args(
    original_args: &[String],
    key_path: &str,
    port: Option<u16>,
    use_macos_keychain: bool,
) -> Vec<String> {
    // Inject -i <key> so the selected key is always offered.
    let mut ssh_args = vec!["-i".to_string(), key_path.to_string()];

    // Keychain passphrase lookup does not require the agent. Keep it disabled so
    // an agent-provided identity can never override the selected key.
    ssh_args.extend([
        "-o".to_string(),
        "IdentitiesOnly=yes".to_string(),
        "-o".to_string(),
        "IdentityAgent=none".to_string(),
    ]);

    if should_use_keychain(use_macos_keychain) {
        ssh_args.push("-o".to_string());
        ssh_args.push("UseKeychain=yes".to_string());
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

pub(crate) fn ssh_program(use_macos_keychain: bool) -> &'static str {
    if should_use_keychain(use_macos_keychain) {
        "/usr/bin/ssh"
    } else {
        "ssh"
    }
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
    fn always_disables_identity_agent() {
        let args = sample_args();
        for use_keychain in [false, true] {
            let final_args = build_ssh_args(&args, "~/.ssh/id_work", None, use_keychain);
            assert!(final_args
                .windows(2)
                .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=none"));
        }
    }

    #[test]
    fn always_injects_identities_only() {
        let args = sample_args();
        // With keychain
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", None, true);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentitiesOnly=yes"));
        // Without keychain
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", None, false);
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentitiesOnly=yes"));
    }

    #[test]
    fn selected_identity_overrides_original_identities_only_no() {
        let original_args = vec![
            "-o".to_string(),
            "IdentitiesOnly=no".to_string(),
            "git@github.com".to_string(),
        ];

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", None, false);
        let injected_yes = final_args
            .windows(2)
            .position(|w| w[0] == "-o" && w[1] == "IdentitiesOnly=yes")
            .unwrap();
        let original_no = final_args
            .windows(2)
            .position(|w| w[0] == "-o" && w[1] == "IdentitiesOnly=no")
            .unwrap();
        assert!(injected_yes < original_no);
    }

    #[test]
    fn selected_identity_overrides_an_agent_from_original_args() {
        let original_args = vec![
            "-o".to_string(),
            "IdentityAgent=/tmp/agent.sock".to_string(),
            "git@github.com".to_string(),
            "git-upload-pack".to_string(),
            "Org/repo.git".to_string(),
        ];

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", None, false);
        let injected_none = final_args
            .windows(2)
            .position(|w| w[0] == "-o" && w[1] == "IdentityAgent=none")
            .unwrap();
        let original_agent = final_args
            .windows(2)
            .position(|w| w[0] == "-o" && w[1] == "IdentityAgent=/tmp/agent.sock")
            .unwrap();
        assert!(injected_none < original_agent);
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

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", Some(22), false);

        let p_count = final_args.iter().filter(|a| a.as_str() == "-p").count();
        assert_eq!(p_count, 1);

        let p_pos = final_args.iter().position(|a| a == "-p").unwrap();
        assert_eq!(final_args[p_pos + 1], "443");
        assert!(final_args.iter().any(|a| a == "git@ssh.github.com"));
    }

    #[test]
    fn keychain_setting_overrides_original_use_keychain_no() {
        let original_args = vec![
            "-o".to_string(),
            "UseKeychain=no".to_string(),
            "git@github.com".to_string(),
            "git-upload-pack".to_string(),
            "Org/repo.git".to_string(),
        ];

        let final_args = build_ssh_args(&original_args, "~/.ssh/id_work", None, true);
        let injected_yes = final_args
            .windows(2)
            .position(|w| w[0] == "-o" && w[1] == "UseKeychain=yes")
            .unwrap();
        let original_no = final_args
            .windows(2)
            .position(|w| w[0] == "-o" && w[1] == "UseKeychain=no")
            .unwrap();
        assert!(injected_yes < original_no);
    }

    #[test]
    fn keychain_disabled_does_not_inject_keychain_options() {
        let args = sample_args();
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", None, false);
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1].starts_with("UseKeychain=")));
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1].starts_with("AddKeysToAgent=")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keychain_uses_selected_key_without_exposing_agent() {
        let args = sample_args();
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", None, true);

        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-i" && w[1] == "~/.ssh/id_work"));
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "UseKeychain=yes"));
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentitiesOnly=yes"));
        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=none"));
        assert!(!final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1].starts_with("AddKeysToAgent=")));
        assert_eq!(ssh_program(true), "/usr/bin/ssh");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keychain_ignores_agent_with_wrong_valid_key() {
        let args = sample_args();
        let previous_agent = std::env::var_os("SSH_AUTH_SOCK");
        std::env::set_var("SSH_AUTH_SOCK", "/tmp/agent-with-wrong-valid-key.sock");
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", None, true);
        match previous_agent {
            Some(value) => std::env::set_var("SSH_AUTH_SOCK", value),
            None => std::env::remove_var("SSH_AUTH_SOCK"),
        }

        assert!(final_args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "IdentityAgent=none"));
        assert!(!final_args
            .iter()
            .any(|arg| arg.contains("agent-with-wrong-valid-key.sock")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_opt_out_falls_back_to_agent_off() {
        let args = sample_args();
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", None, false);
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
        let final_args = build_ssh_args(&args, "~/.ssh/id_work", None, true);
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
