/// Parse the SSH command line that git produces.
///
/// When git invokes sshCommand, the args look like:
///   pickey [ssh-options...] git@<host> <git-command> '<org/repo.git>'
///
/// We need to extract the host and path from these args.

#[derive(Debug, Clone)]
pub struct SshInvocation {
    /// The SSH hostname (e.g. "github.com")
    pub host: String,
    /// The path after the host (e.g. "VolvoGroup-Internal/repo.git")
    pub path: String,
    /// True if this is a push operation (git-receive-pack)
    pub is_push: bool,
}

/// Try to parse args as an SSH invocation from git.
/// Returns None if the args don't look like an SSH call.
pub fn parse_ssh_args(args: &[String]) -> Option<SshInvocation> {
    // git invokes sshCommand as:
    //   <sshCommand> [ssh-opts] <user@host> <git-upload-pack|git-receive-pack|...> '<path>'
    //
    // We need to find the user@host argument and the path argument.
    // SSH options that take a value: -b, -c, -D, -E, -e, -F, -I, -i, -J, -L, -l, -m, -O, -o, -p, -Q, -R, -S, -W, -w

    let ssh_opts_with_value = "bcDEeFIiJLlmOopQRSWw";

    let mut i = 0;
    let mut host_user: Option<String> = None;
    let mut remaining_after_host: Vec<String> = Vec::new();
    let mut found_host = false;

    while i < args.len() {
        let arg = &args[i];

        if found_host {
            remaining_after_host.push(arg.clone());
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            // Check if this option takes a value
            let opt_char = arg.chars().nth(1);
            if let Some(c) = opt_char {
                if ssh_opts_with_value.contains(c) {
                    if arg.len() == 2 {
                        // Value is next arg: -i keyfile
                        i += 2;
                    } else {
                        // Value is attached: -ikeyfile
                        i += 1;
                    }
                    continue;
                }
            }
            i += 1;
            continue;
        }

        // Not an option — this should be user@host or host
        let host = if let Some(at_pos) = arg.find('@') {
            arg[at_pos + 1..].to_string()
        } else {
            arg.clone()
        };

        host_user = Some(host);
        found_host = true;
        i += 1;
    }

    let host = host_user?;

    // Extract the path from remaining args.
    // Git may pass the command + path as either:
    //   Two args:  "git-upload-pack" "'org/repo.git'"
    //   One arg:   "git-upload-pack 'org/repo.git'"  (when invoked via shell)
    let path = extract_path(&remaining_after_host);

    // Detect push (git-receive-pack) vs fetch (git-upload-pack)
    let is_push = remaining_after_host
        .iter()
        .any(|a| a.contains("git-receive-pack"));

    // Strip trailing .git if present for matching purposes
    let path = path.strip_suffix(".git").unwrap_or(&path).to_string();
    // Strip leading / if present
    let path = path.strip_prefix('/').unwrap_or(&path).to_string();

    Some(SshInvocation {
        host,
        path,
        is_push,
    })
}

/// Extract the repo path from the args after the host.
/// Handles both:
///   ["git-upload-pack", "'org/repo.git'"]           — two separate args
///   ["git-upload-pack 'org/repo.git'"]               — single combined arg (shell invocation)
///   ["-o", "SendEnv=GIT_PROTOCOL", "git-upload-pack 'org/repo.git'"]  — with options before
fn extract_path(args: &[String]) -> String {
    // Find the last arg (or the last part of the last arg) that contains a quoted path
    let combined = args.join(" ");

    // Look for a quoted path: 'path' or "path"
    if let Some(start) = combined.rfind('\'') {
        let before = &combined[..start];
        if let Some(open) = before.rfind('\'') {
            return combined[open + 1..start].to_string();
        }
    }
    if let Some(start) = combined.rfind('"') {
        let before = &combined[..start];
        if let Some(open) = before.rfind('"') {
            return combined[open + 1..start].to_string();
        }
    }

    // Fallback: last arg, stripped of quotes
    args.last()
        .map(|p| p.trim_matches('\'').trim_matches('"').to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parse_basic_github_clone() {
        let a = args("git@github.com git-upload-pack 'VolvoGroup-Internal/repo.git'");
        let inv = parse_ssh_args(&a).unwrap();
        assert_eq!(inv.host, "github.com");
        assert_eq!(inv.path, "VolvoGroup-Internal/repo");
    }

    #[test]
    fn parse_with_ssh_options() {
        let a =
            args("-o StrictHostKeyChecking=no -p 22 git@github.com git-upload-pack 'Org/repo.git'");
        let inv = parse_ssh_args(&a).unwrap();
        assert_eq!(inv.host, "github.com");
        assert_eq!(inv.path, "Org/repo");
    }

    #[test]
    fn parse_with_identity_flag() {
        let a = args("-i /tmp/key git@gitlab.com git-upload-pack 'group/subgroup/repo.git'");
        let inv = parse_ssh_args(&a).unwrap();
        assert_eq!(inv.host, "gitlab.com");
        assert_eq!(inv.path, "group/subgroup/repo");
    }

    #[test]
    fn parse_azure_devops() {
        let a = args("git@ssh.dev.azure.com git-receive-pack 'v3/ClientX/Project/Repo.git'");
        let inv = parse_ssh_args(&a).unwrap();
        assert_eq!(inv.host, "ssh.dev.azure.com");
        assert_eq!(inv.path, "v3/ClientX/Project/Repo");
    }

    #[test]
    fn parse_github_ssh_over_https_port_443_push() {
        let a = args("-p 443 git@ssh.github.com git-receive-pack 'Org/repo.git'");
        let inv = parse_ssh_args(&a).unwrap();
        assert_eq!(inv.host, "ssh.github.com");
        assert_eq!(inv.path, "Org/repo");
        assert!(inv.is_push);
    }

    #[test]
    fn parse_no_user() {
        let a = args("example.com git-upload-pack 'test/repo.git'");
        let inv = parse_ssh_args(&a).unwrap();
        assert_eq!(inv.host, "example.com");
    }

    #[test]
    fn parse_leading_slash() {
        let a = args("git@github.com git-upload-pack '/Org/repo.git'");
        let inv = parse_ssh_args(&a).unwrap();
        assert_eq!(inv.path, "Org/repo");
    }

    #[test]
    fn parse_combined_command_and_path() {
        // When git invokes sshCommand via shell, command+path may be one arg
        let a = vec![
            "-o".to_string(),
            "SendEnv=GIT_PROTOCOL".to_string(),
            "git@github.com".to_string(),
            "git-upload-pack 'VolvoGroup-Internal/repo.git'".to_string(),
        ];
        let inv = parse_ssh_args(&a).unwrap();
        assert_eq!(inv.host, "github.com");
        assert_eq!(inv.path, "VolvoGroup-Internal/repo");
    }

    #[test]
    fn parse_combined_with_leading_slash() {
        let a = vec![
            "git@hem-assistans.duckdns.org".to_string(),
            "git-upload-pack '/simeon/villajakt.git'".to_string(),
        ];
        let inv = parse_ssh_args(&a).unwrap();
        assert_eq!(inv.host, "hem-assistans.duckdns.org");
        assert_eq!(inv.path, "simeon/villajakt");
    }
}
