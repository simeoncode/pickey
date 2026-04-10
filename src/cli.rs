use std::process::Command;

use crate::agent;
use crate::config::{self, Config};
use crate::matcher;

/// `pickey` (no args) or `pickey status` — health check dashboard.
pub fn status(config: &Config) {
    // 1. Is pickey active?
    let global_cmd = Command::new("git")
        .args(["config", "--global", "core.sshCommand"])
        .output();
    let is_active = global_cmd.as_ref().is_ok_and(|o| {
        o.status.success() && String::from_utf8_lossy(&o.stdout).trim().contains("pickey")
    });

    if is_active {
        println!("pickey: active ({} rules)", config.rules.len());
    } else {
        println!("pickey: not active");
        println!("  Run `pickey init --apply` to set up");
        return;
    }

    // 2. If in a repo, show what key would be used
    let in_repo = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .is_ok_and(|o| o.status.success());

    if !in_repo {
        return;
    }

    if let Some(remote_url) = get_first_remote_url() {
        if let Some((host, path)) = parse_remote_url(&remote_url) {
            println!("\nThis repo: {}:{}", host, path);
            match matcher::find_match(&config.rules, &host, &path) {
                Some(m) => {
                    let key_name = m.rule.key.rsplit('/').next().unwrap_or(&m.rule.key);
                    print!("SSH key: {} (rule #{})", key_name, m.rule_index + 1);
                    if let Some(port) = m.rule.port {
                        print!(", port {}", port);
                    }
                    println!();
                    if let Some(email) = &m.rule.email {
                        println!(
                            "Commits: {} {}",
                            m.rule.name.as_deref().unwrap_or(""),
                            format!("<{}>", email)
                        );
                    }
                }
                None => {
                    println!("SSH key: no matching rule (will use default)");
                }
            }
        }
    } else {
        println!("\nThis repo: no remotes configured");
    }
}

fn get_first_remote_url() -> Option<String> {
    // Try origin first
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output();
    if let Ok(o) = output {
        if o.status.success() {
            let url = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !url.is_empty() {
                return Some(url);
            }
        }
    }
    // Fall back to first remote
    let output = Command::new("git").args(["remote"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let first = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .to_string();
    if first.is_empty() {
        return None;
    }
    let output = Command::new("git")
        .args(["remote", "get-url", &first])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// `pickey check <url>` — dry-run, show what would match.
pub fn check(config: &Config, url: &str) {
    let (host, path) = match parse_remote_url(url) {
        Some(v) => v,
        None => {
            // Try as host:path shorthand
            match url.split_once(':') {
                Some((h, p)) => {
                    let p = p.strip_suffix(".git").unwrap_or(p);
                    (h.to_string(), p.to_string())
                }
                None => {
                    eprintln!("Could not parse URL: {}", url);
                    return;
                }
            }
        }
    };

    println!("Host: {}", host);
    println!("Path: {}", path);

    match matcher::find_match(&config.rules, &host, &path) {
        Some(m) => {
            let key_path = m.rule.expanded_key();
            println!("Rule:  #{}", m.rule_index + 1);
            if let Some(pat) = &m.rule.match_pattern {
                println!("Match: {}", pat);
            }
            println!("Key:   {}", m.rule.key);
            println!(
                "File:  {} {}",
                key_path.display(),
                if key_path.exists() { "✓" } else { "✗" }
            );
            if let Some(port) = m.rule.port {
                println!("Port:  {}", port);
            }
            if let Some(email) = &m.rule.email {
                println!("Email: {}", email);
            }
            if let Some(name) = &m.rule.name {
                println!("Name:  {}", name);
            }
        }
        None => {
            println!("No matching rule. SSH will use default key selection.");
        }
    }
}

/// `pickey list` — show all rules and agent status.
pub fn list(config: &Config) {
    if config.rules.is_empty() {
        println!("No rules configured.");
        println!("Config: {}", config::default_config_path().display());
        return;
    }

    for (i, rule) in config.rules.iter().enumerate() {
        let key_path = rule.expanded_key();
        let exists = key_path.exists();
        let in_agent = if exists {
            agent::is_key_loaded(&key_path).unwrap_or(false)
        } else {
            false
        };

        println!(
            "#{} {}{}",
            i + 1,
            rule.host,
            rule.match_pattern
                .as_ref()
                .map(|p| format!("/{}", p))
                .unwrap_or_default()
        );
        println!(
            "   Key:   {} {} {}",
            rule.key,
            if exists { "✓" } else { "✗" },
            if in_agent {
                "(agent: loaded)"
            } else {
                "(agent: not loaded)"
            }
        );
        if let Some(email) = &rule.email {
            println!("   Email: {}", email);
        }
        if let Some(name) = &rule.name {
            println!("   Name:  {}", name);
        }
        println!();
    }
}

/// `pickey test` — SSH to the forge with the matched key, show identity.
pub fn test(config: &Config) {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output();

    let remote_url = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            eprintln!("Not in a git repo or no 'origin' remote found.");
            return;
        }
    };

    let (host, path) = match parse_remote_url(&remote_url) {
        Some(v) => v,
        None => {
            eprintln!("Could not parse remote URL: {}", remote_url);
            return;
        }
    };

    let m = match matcher::find_match(&config.rules, &host, &path) {
        Some(m) => m,
        None => {
            eprintln!("No matching rule for {}", remote_url);
            eprintln!("Testing with default SSH key...");
            let result = Command::new("ssh")
                .args(["-T", &format!("git@{}", host)])
                .output();
            if let Ok(o) = result {
                let out = String::from_utf8_lossy(&o.stderr);
                println!("{}", out.trim());
            }
            return;
        }
    };

    let key_path = m.rule.expanded_key();
    println!("Testing SSH to {} with key {}", host, m.rule.key);

    let result = Command::new("ssh")
        .args([
            "-T",
            "-i",
            &key_path.to_string_lossy(),
            "-o",
            "IdentitiesOnly=yes",
            &format!("git@{}", host),
        ])
        .output();

    match result {
        Ok(o) => {
            // Most forges return the identity message on stderr
            let stderr = String::from_utf8_lossy(&o.stderr);
            let stdout = String::from_utf8_lossy(&o.stdout);
            if !stderr.is_empty() {
                println!("{}", stderr.trim());
            }
            if !stdout.is_empty() {
                println!("{}", stdout.trim());
            }
        }
        Err(e) => {
            eprintln!("SSH failed: {}", e);
        }
    }
}

/// Parse a git remote URL into (host, path).
/// Supports:
///   git@github.com:Org/repo.git
///   ssh://git@github.com/Org/repo.git
///   git@github.com:Org/repo  (no .git)
pub fn parse_remote_url(url: &str) -> Option<(String, String)> {
    // ssh:// format
    if let Some(rest) = url.strip_prefix("ssh://") {
        // ssh://git@host/path or ssh://git@host:port/path
        let rest = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
        // Handle optional port: host:port/path
        let (host, path) = if let Some((before_slash, after_slash)) = rest.split_once('/') {
            let host = before_slash.split(':').next().unwrap_or(before_slash);
            (host.to_string(), after_slash.to_string())
        } else {
            return None;
        };
        let path = path.strip_suffix(".git").unwrap_or(&path).to_string();
        return Some((host, path));
    }

    // SCP-style: git@host:path
    if let Some(at_pos) = url.find('@') {
        let after_at = &url[at_pos + 1..];
        if let Some(colon_pos) = after_at.find(':') {
            let host = &after_at[..colon_pos];
            let path = &after_at[colon_pos + 1..];
            let path = path.strip_suffix(".git").unwrap_or(path);
            let path = path.strip_prefix('/').unwrap_or(path);
            return Some((host.to_string(), path.to_string()));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scp_style() {
        let (host, path) = parse_remote_url("git@github.com:VolvoGroup-Internal/repo.git").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "VolvoGroup-Internal/repo");
    }

    #[test]
    fn parse_ssh_url() {
        let (host, path) = parse_remote_url("ssh://git@github.com/Org/repo.git").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "Org/repo");
    }

    #[test]
    fn parse_ssh_url_with_port() {
        let (host, path) = parse_remote_url("ssh://git@github.com:22/Org/repo.git").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "Org/repo");
    }

    #[test]
    fn parse_no_dot_git() {
        let (host, path) = parse_remote_url("git@github.com:Org/repo").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "Org/repo");
    }

    #[test]
    fn parse_azure_devops() {
        let (host, path) =
            parse_remote_url("git@ssh.dev.azure.com:v3/ClientX/Project/Repo").unwrap();
        assert_eq!(host, "ssh.dev.azure.com");
        assert_eq!(path, "v3/ClientX/Project/Repo");
    }
}
