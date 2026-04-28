use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config;

/// `pickey init` — set up pickey: write config, fix conflicts, enable globally.
/// With `--dry-run`: preview what would happen without making changes.
pub fn init(dry_run: bool) {
    let home = dirs::home_dir().unwrap_or_default();

    // Scan environment
    let ssh_dir = home.join(".ssh");
    let extra_key_paths = collect_identity_files_from_ssh_config(&ssh_dir);
    let keys = find_ssh_keys(&ssh_dir, &extra_key_paths);
    let git_info = scan_git_config();
    let scanned_dirs: Vec<String> = git_info
        .include_ifs
        .iter()
        .map(|i| {
            i.pattern
                .trim_end_matches("**")
                .trim_end_matches('/')
                .to_string()
        })
        .collect();
    let local_overrides = find_repos_with_local_ssh_command(&git_info.include_ifs);
    let suggestion_build = build_suggestions(&git_info, &keys, &local_overrides);
    let suggestions = &suggestion_build.rules;

    let already_enabled = git_info
        .global_ssh_command
        .as_deref()
        .is_some_and(|cmd| cmd.contains("pickey"));
    let has_foreign_global = git_info
        .global_ssh_command
        .as_deref()
        .is_some_and(|cmd| !cmd.contains("pickey"));
    let has_include_conflicts = git_info.include_ifs.iter().any(|i| i.ssh_command.is_some());
    let has_local_conflicts = !local_overrides.is_empty();

    if dry_run {
        // Verbose diagnostic output
        print_dry_run(DryRunInput {
            keys: &keys,
            git_info: &git_info,
            local_overrides: &local_overrides,
            suggestions,
            manual_rules: &suggestion_build.manual_rules,
            scanned_dirs: &scanned_dirs,
            already_enabled,
            has_foreign_global,
            has_include_conflicts,
            has_local_conflicts,
        });
        return;
    }

    // --- Apply mode (default) ---
    println!("pickey init\n");

    let mut changed = false;

    // 1. Config
    let config_path = config::default_config_path();
    let config_display = make_display_path(&config_path, &home);

    if suggestions.is_empty() && !config_path.exists() {
        println!("✗ No rules auto-detected and no config exists.");
        println!(
            "  Create {} manually, or add SSH keys and includeIf entries first.",
            config_display
        );
        print_manual_rule_actions(&suggestion_build.manual_rules);
        return;
    }

    if config_path.exists() {
        let merged = merge_config(&config_path, suggestions);
        match merged {
            ConfigMergeResult::Unchanged(count) => {
                println!("✓ Config: {} ({} rules, up to date)", config_display, count);
            }
            ConfigMergeResult::Updated {
                toml,
                total,
                added,
                removed,
            } => {
                if let Err(e) = write_config(&config_path, &toml) {
                    println!("✗ Failed to write {}: {}", config_display, e);
                } else {
                    let mut parts = Vec::new();
                    if added > 0 {
                        parts.push(format!("+{} new", added));
                    }
                    if removed > 0 {
                        parts.push(format!("-{} stale", removed));
                    }
                    println!(
                        "✓ Updated {} ({} rules, {})",
                        config_display,
                        total,
                        parts.join(", ")
                    );
                    changed = true;
                }
            }
        }
    } else if !suggestions.is_empty() {
        let toml = format_auto_config(suggestions);
        if let Err(e) = write_config(&config_path, &toml) {
            println!("✗ Failed to write {}: {}", config_display, e);
        } else {
            println!("✓ Wrote {} ({} rules)", config_display, suggestions.len());
            changed = true;
        }
    }

    print_manual_rule_actions(&suggestion_build.manual_rules);

    // 2. Fix conflicts
    if has_include_conflicts || has_local_conflicts {
        let mut fixed = 0;
        let mut failed = Vec::new();

        if has_include_conflicts {
            for inc in git_info
                .include_ifs
                .iter()
                .filter(|i| i.ssh_command.is_some())
            {
                let config_file = if let Some(tail) = inc.config_path.strip_prefix("~/") {
                    home.join(tail)
                } else {
                    PathBuf::from(&inc.config_path)
                };
                match disable_ssh_command(&config_file) {
                    Ok(()) => fixed += 1,
                    Err(e) => failed.push(format!("{}: {}", inc.config_path, e)),
                }
            }
        }
        if has_local_conflicts {
            for ov in &local_overrides {
                let config_file = ov.repo_dir.join(".git/config");
                match disable_ssh_command(&config_file) {
                    Ok(()) => fixed += 1,
                    Err(e) => {
                        let display = make_display_path(&ov.repo_dir, &home);
                        failed.push(format!("{}: {}", display, e));
                    }
                }
            }
        }

        if failed.is_empty() {
            println!(
                "✓ Fixed {} sshCommand conflict{}",
                fixed,
                if fixed == 1 { "" } else { "s" }
            );
            changed = true;
        } else {
            println!(
                "✓ Fixed {} sshCommand conflict{}",
                fixed,
                if fixed == 1 { "" } else { "s" }
            );
            changed = true;
            for f in &failed {
                println!("  ✗ {}", f);
            }
        }
    }

    // 3. Enable global sshCommand
    if already_enabled {
        println!("✓ Global sshCommand: pickey");
    } else {
        if has_foreign_global {
            let prev = git_info.global_ssh_command.as_deref().unwrap();
            let _ = Command::new("git")
                .args(["config", "--global", "pickey.previousSshCommand", prev])
                .status();
        }
        let status = Command::new("git")
            .args(["config", "--global", "core.sshCommand", "pickey"])
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("✓ Enabled as global sshCommand");
                changed = true;
            }
            _ => println!("✗ Failed to set global core.sshCommand"),
        }
    }

    if changed {
        println!("\nUndo with `pickey init --revert`.");
    }
}

struct DryRunInput<'a> {
    keys: &'a [SshKey],
    git_info: &'a GitInfo,
    local_overrides: &'a [LocalSshOverride],
    suggestions: &'a [SuggestedRule],
    manual_rules: &'a [ManualRule],
    scanned_dirs: &'a [String],
    already_enabled: bool,
    has_foreign_global: bool,
    has_include_conflicts: bool,
    has_local_conflicts: bool,
}

fn print_dry_run(input: DryRunInput<'_>) {
    let home = dirs::home_dir().unwrap_or_default();
    println!("pickey init --dry-run\n");

    // Keys
    if input.keys.is_empty() {
        println!("Keys: (none found)");
    } else {
        let mut by_dir: Vec<(String, Vec<String>)> = Vec::new();
        for key in input.keys {
            let dir_display = key
                .path
                .parent()
                .map(|p| make_display_path(p, &home))
                .unwrap_or_default();
            let name = key
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if let Some(entry) = by_dir.iter_mut().find(|(d, _)| d == &dir_display) {
                entry.1.push(name);
            } else {
                by_dir.push((dir_display, vec![name]));
            }
        }
        for (dir, names) in &by_dir {
            println!("Keys: {} ({} found)", dir, names.len());
            let mut sorted = names.clone();
            sorted.sort();
            println!("  {}", sorted.join(", "));
        }
    }

    // Global sshCommand status
    if input.already_enabled {
        println!("\nGlobal sshCommand: pickey ✓");
    } else if input.has_foreign_global {
        println!(
            "\nGlobal sshCommand: {} (will be replaced)",
            input.git_info.global_ssh_command.as_deref().unwrap()
        );
    } else {
        println!("\nGlobal sshCommand: (not set, will enable)");
    }

    // Conflicts
    if input.has_include_conflicts || input.has_local_conflicts {
        println!("\nConflicts to fix:");
        if input.has_include_conflicts {
            for inc in input
                .git_info
                .include_ifs
                .iter()
                .filter(|i| i.ssh_command.is_some())
            {
                println!("  sshCommand in {} will be disabled", inc.config_path);
            }
        }
        if input.has_local_conflicts {
            for ov in input.local_overrides {
                let display = make_display_path(&ov.repo_dir, &home);
                println!("  sshCommand in {} will be disabled", display);
            }
        }
    }

    // Suggested rules
    let config_path = config::default_config_path();
    let config_display = make_display_path(&config_path, &home);

    if input.suggestions.is_empty() {
        println!("\nNo rules auto-detected.");
    } else {
        println!("\nAuto-detected rules ({}):", input.suggestions.len());
        for s in input.suggestions {
            print!("  {} ", s.host);
            if let Some(pat) = &s.match_pattern {
                print!("{} ", pat);
            }
            print!("→ {}", s.key_display);
            if let Some(port) = s.port {
                print!(" :{}", port);
            }
            println!();
        }
    }

    print_manual_rule_actions(input.manual_rules);

    if config_path.exists() {
        let merged = merge_config(&config_path, input.suggestions);
        match merged {
            ConfigMergeResult::Unchanged(count) => {
                println!("\nConfig: {} ({} rules, up to date)", config_display, count);
            }
            ConfigMergeResult::Updated {
                total,
                added,
                removed,
                ..
            } => {
                let mut parts = Vec::new();
                if added > 0 {
                    parts.push(format!("+{} new", added));
                }
                if removed > 0 {
                    parts.push(format!("-{} stale", removed));
                }
                println!(
                    "\nConfig: {} (would update: {} rules, {})",
                    config_display,
                    total,
                    parts.join(", ")
                );
            }
        }
    } else {
        println!("\nConfig: {} (will be created)", config_display);
    }

    if !input.scanned_dirs.is_empty() {
        println!("\nScope: repos under {}.", input.scanned_dirs.join(", "));
    }

    println!("\nRun `pickey init` to apply.");
}

// --- Config merging ---

enum ConfigMergeResult {
    Unchanged(usize),
    Updated {
        toml: String,
        total: usize,
        added: usize,
        removed: usize,
    },
}

/// Merge auto-detected rules into existing config, preserving user (non-auto) rules.
fn merge_config(config_path: &Path, suggestions: &[SuggestedRule]) -> ConfigMergeResult {
    let existing = config::load_config(Some(config_path));
    let existing_rules = match &existing {
        Ok(config) => &config.rules,
        Err(_) => {
            return ConfigMergeResult::Updated {
                toml: format_auto_config(suggestions),
                total: suggestions.len(),
                added: suggestions.len(),
                removed: 0,
            }
        }
    };

    let user_rules: Vec<&config::Rule> = existing_rules.iter().filter(|r| !r.auto).collect();
    let old_auto: Vec<&config::Rule> = existing_rules.iter().filter(|r| r.auto).collect();

    // Check if auto rules match suggestions
    let auto_match = old_auto.len() == suggestions.len()
        && suggestions.iter().enumerate().all(|(i, s)| {
            let r = old_auto[i];
            r.host == s.host
                && r.match_pattern == s.match_pattern
                && r.key == s.key_display
                && r.port == s.port
                && r.email == s.email
                && r.name == s.name
        });

    if auto_match || (suggestions.is_empty() && !old_auto.is_empty()) {
        // No new suggestions but auto rules exist — keep them (source data may be gone after apply)
        return ConfigMergeResult::Unchanged(existing_rules.len());
    }

    // Build new TOML: user rules first, then new auto rules
    let mut toml = String::new();
    for (i, rule) in user_rules.iter().enumerate() {
        if i > 0 {
            toml.push('\n');
        }
        toml.push_str(&format_rule(rule));
    }

    let added = suggestions.len();
    let removed = old_auto.len();

    if !user_rules.is_empty() && !suggestions.is_empty() {
        toml.push('\n');
    }
    for (i, s) in suggestions.iter().enumerate() {
        if i > 0 {
            toml.push('\n');
        }
        toml.push_str(&format_suggested_rule_with_auto(s));
    }

    let total = user_rules.len() + suggestions.len();

    ConfigMergeResult::Updated {
        toml,
        total,
        added,
        removed,
    }
}

fn format_rule(rule: &config::Rule) -> String {
    let mut out = String::new();
    out.push_str("[[rule]]\n");
    if rule.auto {
        out.push_str("auto = true\n");
    }
    out.push_str(&format!("host = \"{}\"\n", rule.host));
    if let Some(pat) = &rule.match_pattern {
        out.push_str(&format!("match = \"{}\"\n", pat));
    }
    out.push_str(&format!("key = \"{}\"\n", rule.key));
    if let Some(port) = rule.port {
        out.push_str(&format!("port = {}\n", port));
    }
    if let Some(email) = &rule.email {
        out.push_str(&format!("email = \"{}\"\n", email));
    }
    if let Some(name) = &rule.name {
        out.push_str(&format!("name = \"{}\"\n", name));
    }
    out
}

fn format_suggested_rule_with_auto(rule: &SuggestedRule) -> String {
    let mut out = String::new();
    out.push_str("[[rule]]\n");
    out.push_str("auto = true\n");
    out.push_str(&format!("host = \"{}\"\n", rule.host));
    if let Some(pat) = &rule.match_pattern {
        out.push_str(&format!("match = \"{}\"\n", pat));
    }
    out.push_str(&format!("key = \"{}\"\n", rule.key_display));
    if let Some(port) = rule.port {
        out.push_str(&format!("port = {}\n", port));
    }
    if let Some(email) = &rule.email {
        out.push_str(&format!("email = \"{}\"\n", email));
    }
    if let Some(name) = &rule.name {
        out.push_str(&format!("name = \"{}\"\n", name));
    }
    out
}

fn format_auto_config(suggestions: &[SuggestedRule]) -> String {
    let mut out = String::new();
    for (i, rule) in suggestions.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format_suggested_rule_with_auto(rule));
    }
    out
}

fn write_config(path: &Path, toml: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml)
}

// --- Conflict resolution via git config --file ---

/// Back up and unset core.sshCommand in a git config file.
fn disable_ssh_command(path: &Path) -> Result<(), String> {
    let path_str = path.to_string_lossy();

    // Read current sshCommand
    let output = Command::new("git")
        .args(["config", "--file", &path_str, "core.sshCommand"])
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err("no sshCommand found".to_string());
    }

    let current = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if current.is_empty() {
        return Err("empty sshCommand".to_string());
    }

    // Back up to pickey.previousSshCommand
    let status = Command::new("git")
        .args([
            "config",
            "--file",
            &path_str,
            "pickey.previousSshCommand",
            &current,
        ])
        .status()
        .map_err(|e| e.to_string())?;

    if !status.success() {
        return Err("failed to write backup".to_string());
    }

    // Unset core.sshCommand
    let status = Command::new("git")
        .args(["config", "--file", &path_str, "--unset", "core.sshCommand"])
        .status()
        .map_err(|e| e.to_string())?;

    if !status.success() {
        return Err("failed to unset sshCommand".to_string());
    }

    Ok(())
}

/// Restore core.sshCommand from pickey.previousSshCommand backup.
fn restore_ssh_command(path: &Path) -> Result<bool, String> {
    let path_str = path.to_string_lossy();

    // Check for backup
    let output = Command::new("git")
        .args(["config", "--file", &path_str, "pickey.previousSshCommand"])
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Ok(false);
    }

    let prev = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if prev.is_empty() {
        return Ok(false);
    }

    // Restore
    let status = Command::new("git")
        .args(["config", "--file", &path_str, "core.sshCommand", &prev])
        .status()
        .map_err(|e| e.to_string())?;

    if !status.success() {
        return Err("failed to restore sshCommand".to_string());
    }

    // Clean up backup
    let _ = Command::new("git")
        .args(["config", "--file", &path_str, "--remove-section", "pickey"])
        .stderr(std::process::Stdio::null())
        .status();

    Ok(true)
}

/// Check if a git config file has a pickey.previousSshCommand backup.
fn has_pickey_backup(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    Command::new("git")
        .args(["config", "--file", &path_str, "pickey.previousSshCommand"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Collect all git config files that have a pickey.previousSshCommand backup.
fn find_pickey_managed_files() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut files = Vec::new();

    // Check includeIf config files referenced from global gitconfig
    let git_info = scan_git_config();
    for inc in &git_info.include_ifs {
        let config_file = if let Some(tail) = inc.config_path.strip_prefix("~/") {
            home.join(tail)
        } else {
            PathBuf::from(&inc.config_path)
        };
        if has_pickey_backup(&config_file) {
            files.push(config_file);
        }
    }

    // Check repos under includeIf dirs for .git/config with pickey backup
    for inc in &git_info.include_ifs {
        let dir = if let Some(tail) = inc.pattern.strip_prefix("~/") {
            let tail = tail.trim_end_matches("**").trim_end_matches('/');
            home.join(tail)
        } else {
            let cleaned = inc.pattern.trim_end_matches("**").trim_end_matches('/');
            PathBuf::from(cleaned)
        };
        if dir.is_dir() {
            collect_pickey_repo_configs(&dir, 0, 4, &mut files);
        }
    }

    files
}

fn collect_pickey_repo_configs(dir: &Path, depth: u32, max_depth: u32, files: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".git") {
                let config = path.join("config");
                if has_pickey_backup(&config) {
                    files.push(config);
                }
            } else if !path.file_name().is_some_and(|n| {
                n.to_string_lossy().starts_with('.') || n == "node_modules" || n == "target"
            }) {
                collect_pickey_repo_configs(&path, depth + 1, max_depth, files);
            }
        }
    }
}

/// `pickey init --revert` — undo all changes and unset global sshCommand.
pub fn revert() {
    let home = dirs::home_dir().unwrap_or_default();
    let files = find_pickey_managed_files();

    if files.is_empty() {
        // Also check if global sshCommand is pickey
        let global_cmd = git_config_get("core.sshCommand", &["--global"]);
        if global_cmd.as_deref() != Some("pickey") {
            println!("Nothing to revert.");
            return;
        }
    }

    println!("Reverting pickey changes:\n");

    // Restore sshCommand in all managed files
    for file in &files {
        let display = make_display_path(file, &home);
        match restore_ssh_command(file) {
            Ok(true) => println!("  ✓ Restored sshCommand in {}", display),
            Ok(false) => {}
            Err(e) => println!("  ✗ Failed to restore {}: {}", display, e),
        }
    }

    // Restore or unset global sshCommand
    let global_cmd = git_config_get("core.sshCommand", &["--global"]);
    if global_cmd.as_deref() == Some("pickey") {
        let backup = git_config_get("pickey.previousSshCommand", &["--global"]);
        if let Some(prev) = backup {
            let status = Command::new("git")
                .args(["config", "--global", "core.sshCommand", &prev])
                .status();
            match status {
                Ok(s) if s.success() => {
                    println!("  ✓ Restored global core.sshCommand to: {}", prev)
                }
                _ => println!("  ✗ Failed to restore global core.sshCommand"),
            }
        } else {
            let status = Command::new("git")
                .args(["config", "--global", "--unset", "core.sshCommand"])
                .status();
            match status {
                Ok(s) if s.success() => println!("  ✓ Unset global core.sshCommand"),
                _ => println!("  ✗ Failed to unset global core.sshCommand"),
            }
        }
        // Clean up backup key (suppress stderr if section doesn't exist)
        let _ = Command::new("git")
            .args(["config", "--global", "--remove-section", "pickey"])
            .stderr(std::process::Stdio::null())
            .status();
    }

    println!("\nDone. pickey is no longer active.");
}

// --- SSH key discovery ---

struct SshKey {
    path: PathBuf,
}

fn find_ssh_keys(ssh_dir: &Path, extra_paths: &[PathBuf]) -> Vec<SshKey> {
    let mut keys = Vec::new();
    let mut seen = BTreeSet::new();

    // Scan ~/.ssh/ for .pub files
    if ssh_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(ssh_dir) {
            let mut pub_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "pub"))
                .collect();
            pub_files.sort_by_key(|e| e.path());

            for entry in pub_files {
                let pub_path = entry.path();
                let priv_path = pub_path.with_extension("");
                if priv_path.exists() && seen.insert(priv_path.clone()) {
                    keys.push(SshKey { path: priv_path });
                }
            }
        }
    }

    // Add any extra keys from ssh_config IdentityFile directives
    for path in extra_paths {
        if path.exists() && seen.insert(path.clone()) {
            keys.push(SshKey { path: path.clone() });
        }
    }

    keys
}

/// Convert a path to a display-friendly string, using `~` for the home directory.
fn make_display_path(path: &Path, home: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(home) {
        format!("~/{}", rel.display())
    } else {
        path.display().to_string()
    }
}

/// Parse IdentityFile directives from ~/.ssh/config and /etc/ssh/ssh_config
/// to find keys that may not be in ~/.ssh/.
fn collect_identity_files_from_ssh_config(ssh_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();

    let config_files = [ssh_dir.join("config"), PathBuf::from("/etc/ssh/ssh_config")];

    for config_file in &config_files {
        if let Ok(contents) = std::fs::read_to_string(config_file) {
            for line in contents.lines() {
                let trimmed = line.trim();
                // Skip comments
                if trimmed.starts_with('#') {
                    continue;
                }
                // Look for IdentityFile directives (case-insensitive)
                if let Some(rest) = trimmed.strip_prefix("IdentityFile") {
                    let rest = rest.trim();
                    if !rest.is_empty() {
                        let expanded = if let Some(tail) = rest.strip_prefix("~/") {
                            home.join(tail)
                        } else {
                            PathBuf::from(rest)
                        };
                        paths.push(expanded);
                    }
                }
            }
        }
    }

    paths
}

// --- Repo-local sshCommand detection ---

struct LocalSshOverride {
    repo_dir: PathBuf,
    ssh_command: String,
    remote_url: Option<String>,
}

/// Scan directories from includeIf patterns for repos with local core.sshCommand set.
fn find_repos_with_local_ssh_command(include_ifs: &[IncludeIfRule]) -> Vec<LocalSshOverride> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut overrides = Vec::new();

    // Only scan directories we know about from includeIf patterns
    for inc in include_ifs {
        let dir = if let Some(tail) = inc.pattern.strip_prefix("~/") {
            let tail = tail.trim_end_matches("**").trim_end_matches('/');
            home.join(tail)
        } else {
            let cleaned = inc.pattern.trim_end_matches("**").trim_end_matches('/');
            PathBuf::from(cleaned)
        };

        if dir.is_dir() {
            collect_repos_with_local_ssh(&dir, 0, 4, &mut overrides);
        }
    }

    overrides
}

fn collect_repos_with_local_ssh(
    dir: &Path,
    depth: u32,
    max_depth: u32,
    overrides: &mut Vec<LocalSshOverride>,
) {
    if depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".git") {
                // Found a repo — check for local sshCommand
                if let Some(ssh_cmd) = get_local_ssh_command(dir) {
                    let remote_url = get_remote_url(dir);
                    overrides.push(LocalSshOverride {
                        repo_dir: dir.to_path_buf(),
                        ssh_command: ssh_cmd,
                        remote_url,
                    });
                }
            } else if !path.file_name().is_some_and(|n| {
                n.to_string_lossy().starts_with('.') || n == "node_modules" || n == "target"
            }) {
                collect_repos_with_local_ssh(&path, depth + 1, max_depth, overrides);
            }
        }
    }
}

fn get_local_ssh_command(repo_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "config",
            "--local",
            "core.sshCommand",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !val.is_empty() {
            return Some(val);
        }
    }
    None
}

// --- Git config scanning ---

struct GitInfo {
    global_ssh_command: Option<String>,
    include_ifs: Vec<IncludeIfRule>,
    #[allow(dead_code)]
    global_email: Option<String>,
    #[allow(dead_code)]
    global_name: Option<String>,
}

struct IncludeIfRule {
    pattern: String,
    config_path: String,
    ssh_command: Option<String>,
    email: Option<String>,
    name: Option<String>,
}

fn scan_git_config() -> GitInfo {
    let global_ssh_command = git_config_get("core.sshCommand", &["--global"]);
    let global_email = git_config_get("user.email", &["--global"]);
    let global_name = git_config_get("user.name", &["--global"]);

    // Parse includeIf rules from global gitconfig
    let include_ifs = parse_include_ifs();

    GitInfo {
        global_ssh_command,
        include_ifs,
        global_email,
        global_name,
    }
}

fn git_config_get(key: &str, extra_args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("config");
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.arg(key);

    let output = cmd.output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn parse_include_ifs() -> Vec<IncludeIfRule> {
    let output = Command::new("git")
        .args([
            "config",
            "--global",
            "--get-regexp",
            r"^includeif\..*\.path$",
        ])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    let home = dirs::home_dir().unwrap_or_default();
    let mut rules = Vec::new();

    for line in output.lines() {
        let Some((key, config_path)) = parse_git_config_key_value(line) else {
            continue;
        };
        let Some(pattern) = include_pattern_from_config_key(key) else {
            continue;
        };

        let expanded_config = if let Some(tail) = config_path.strip_prefix("~/") {
            home.join(tail)
        } else {
            PathBuf::from(config_path)
        };

        let (ssh_command, email, name) = read_include_config(&expanded_config);

        rules.push(IncludeIfRule {
            pattern,
            config_path: config_path.to_string(),
            ssh_command,
            email,
            name,
        });
    }

    rules
}

fn parse_git_config_key_value(line: &str) -> Option<(&str, &str)> {
    let split_at = line.find(char::is_whitespace)?;
    let key = &line[..split_at];
    let value = line[split_at..].trim_start();
    if key.is_empty() || value.is_empty() {
        None
    } else {
        Some((key, value))
    }
}

fn include_pattern_from_config_key(key: &str) -> Option<String> {
    let lower = key.to_ascii_lowercase();
    let rest = lower.strip_prefix("includeif.")?;
    let condition_len = rest.strip_suffix(".path")?.len();
    let condition = &key["includeif.".len().."includeif.".len() + condition_len];
    let condition_lower = condition.to_ascii_lowercase();

    if condition_lower.starts_with("gitdir/i:") {
        Some(condition["gitdir/i:".len()..].to_string())
    } else if condition_lower.starts_with("gitdir:") {
        Some(condition["gitdir:".len()..].to_string())
    } else {
        None
    }
}

fn read_include_config(path: &Path) -> (Option<String>, Option<String>, Option<String>) {
    (
        git_config_get_from_file(path, "core.sshCommand"),
        git_config_get_from_file(path, "user.email"),
        git_config_get_from_file(path, "user.name"),
    )
}

fn git_config_get_from_file(path: &Path, key: &str) -> Option<String> {
    let mut command = Command::new("git");
    if path.is_absolute() {
        command.current_dir("/");
    }
    let output = command
        .arg("config")
        .arg("--file")
        .arg(path)
        .arg(key)
        .output()
        .ok()?;
    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    } else {
        None
    }
}

// --- Config suggestion ---

struct SuggestedRule {
    host: String,
    match_pattern: Option<String>,
    key_display: String,
    email: Option<String>,
    name: Option<String>,
    port: Option<u16>,
}

struct ManualRule {
    pattern: String,
    key_display: String,
    email: Option<String>,
    name: Option<String>,
    port: Option<u16>,
}

struct SuggestionBuild {
    rules: Vec<SuggestedRule>,
    manual_rules: Vec<ManualRule>,
}

fn build_suggestions(
    git_info: &GitInfo,
    _keys: &[SshKey],
    local_overrides: &[LocalSshOverride],
) -> SuggestionBuild {
    let mut suggestions: Vec<SuggestedRule> = Vec::new();
    let mut manual_rules: Vec<ManualRule> = Vec::new();

    // Build suggestions from includeIf rules that have sshCommand
    for inc in &git_info.include_ifs {
        if let Some(ssh_cmd) = &inc.ssh_command {
            // Try to extract key path and port from the sshCommand
            let (key_path, port) = parse_ssh_command_for_key_and_port(ssh_cmd);

            if let Some(key) = key_path {
                // Try to find matching repos under this gitdir pattern to determine hosts/orgs
                let repos = find_repos_under_pattern(&inc.pattern);
                let grouped = group_repos_by_host_and_org(&repos);

                if grouped.is_empty() {
                    manual_rules.push(ManualRule {
                        pattern: inc.pattern.clone(),
                        key_display: key,
                        email: inc.email.clone(),
                        name: inc.name.clone(),
                        port,
                    });
                } else {
                    for (host, org) in grouped.keys() {
                        let match_pattern = if org.is_empty() {
                            None
                        } else {
                            Some(format!("{}/**", org))
                        };
                        // Avoid duplicate suggestions
                        let already = suggestions
                            .iter()
                            .any(|s| s.host == *host && s.match_pattern == match_pattern);
                        if !already {
                            suggestions.push(SuggestedRule {
                                host: host.clone(),
                                match_pattern,
                                key_display: key.clone(),
                                email: inc.email.clone(),
                                name: inc.name.clone(),
                                port,
                            });
                        }
                    }
                }
            }
        }
    }

    // Also build suggestions from repos with local core.sshCommand
    for ov in local_overrides {
        if let Some(url) = &ov.remote_url {
            if let Some((host, repo_path)) = crate::cli::parse_remote_url(url) {
                let (key_path, port) = parse_ssh_command_for_key_and_port(&ov.ssh_command);
                if let Some(key) = key_path {
                    let org = extract_org(&host, &repo_path);
                    let match_pattern = if org.is_empty() {
                        None
                    } else {
                        Some(format!("{}/**", org))
                    };
                    // Get email/name from the repo's local config
                    let email = Command::new("git")
                        .args([
                            "-C",
                            &ov.repo_dir.to_string_lossy(),
                            "config",
                            "--local",
                            "user.email",
                        ])
                        .output()
                        .ok()
                        .and_then(|o| {
                            if o.status.success() {
                                let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
                                if v.is_empty() {
                                    None
                                } else {
                                    Some(v)
                                }
                            } else {
                                None
                            }
                        });
                    let name = Command::new("git")
                        .args([
                            "-C",
                            &ov.repo_dir.to_string_lossy(),
                            "config",
                            "--local",
                            "user.name",
                        ])
                        .output()
                        .ok()
                        .and_then(|o| {
                            if o.status.success() {
                                let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
                                if v.is_empty() {
                                    None
                                } else {
                                    Some(v)
                                }
                            } else {
                                None
                            }
                        });

                    let already = suggestions
                        .iter()
                        .any(|s| s.host == host && s.match_pattern == match_pattern);
                    if !already {
                        suggestions.push(SuggestedRule {
                            host,
                            match_pattern,
                            key_display: key,
                            email,
                            name,
                            port,
                        });
                    }
                }
            }
        }
    }

    SuggestionBuild {
        rules: suggestions,
        manual_rules,
    }
}

fn print_manual_rule_actions(manual_rules: &[ManualRule]) {
    if manual_rules.is_empty() {
        return;
    }

    println!("\nManual rule needed:");
    for rule in manual_rules {
        println!(
            "  Could not infer host/path for includeIf pattern {}.",
            rule.pattern
        );
        println!("  Key: {}", rule.key_display);
        if let Some(port) = rule.port {
            println!("  Port: {}", port);
        }
        if let Some(email) = &rule.email {
            println!("  Email: {}", email);
        }
        if let Some(name) = &rule.name {
            println!("  Name: {}", name);
        }
    }
}

/// Parse an sshCommand like "/usr/bin/ssh -o IdentitiesOnly=yes -i ~/.ssh/vce_github -p 222"
/// to extract the key path and optional port.
fn parse_ssh_command_for_key_and_port(cmd: &str) -> (Option<String>, Option<u16>) {
    let parts = tokenize_ssh_command(cmd);
    let mut key = None;
    let mut port = None;

    let mut i = 0;
    while i < parts.len() {
        if parts[i] == "-i" && i + 1 < parts.len() {
            let (value, next_i) = collect_ssh_option_value(&parts, i + 1);
            key = Some(value);
            i = next_i;
            continue;
        }
        if let Some(k) = parts[i].strip_prefix("-i").filter(|k| !k.is_empty()) {
            key = Some(k.to_string());
            i += 1;
            continue;
        }
        if parts[i] == "-p" {
            if let Some(p) = parts.get(i + 1) {
                port = p.parse().ok();
                i += 2;
                continue;
            }
        }
        if let Some(p) = parts[i].strip_prefix("-p").filter(|p| !p.is_empty()) {
            port = p.parse().ok();
            i += 1;
            continue;
        }
        if parts[i] == "-o" {
            if let Some(option) = parts.get(i + 1) {
                parse_ssh_option(option, &mut key, &mut port);
                i += 2;
                continue;
            }
        }
        if let Some(option) = parts[i].strip_prefix("-o").filter(|o| !o.is_empty()) {
            parse_ssh_option(option, &mut key, &mut port);
            i += 1;
            continue;
        }
        i += 1;
    }

    (key, port)
}

fn collect_ssh_option_value(parts: &[String], start: usize) -> (String, usize) {
    let mut end = start + 1;
    while end < parts.len() && !parts[end].starts_with('-') {
        end += 1;
    }
    (parts[start..end].join(" "), end)
}

fn parse_ssh_option(option: &str, key: &mut Option<String>, port: &mut Option<u16>) {
    if let Some(value) = option.strip_prefix("IdentityFile=") {
        *key = Some(value.to_string());
    }
    if let Some(value) = option.strip_prefix("Port=") {
        *port = value.parse().ok();
    }
}

fn tokenize_ssh_command(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut in_word = false;
    let mut escaped = false;

    for c in input.chars() {
        if escaped {
            current.push(c);
            in_word = true;
            escaped = false;
            continue;
        }

        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    current.push(c);
                }
            }
            Some('"') => {
                if c == '"' {
                    quote = None;
                } else if c == '\\' {
                    escaped = true;
                } else {
                    current.push(c);
                }
            }
            Some(_) => unreachable!(),
            None => {
                if c.is_whitespace() {
                    if in_word {
                        words.push(std::mem::take(&mut current));
                        in_word = false;
                    }
                } else if c == '\'' || c == '"' {
                    quote = Some(c);
                    in_word = true;
                } else if c == '\\' {
                    escaped = true;
                } else {
                    current.push(c);
                    in_word = true;
                }
            }
        }
    }

    if escaped {
        current.push('\\');
        in_word = true;
    }

    if in_word {
        words.push(current);
    }

    words
}

/// Find git repos under a gitdir pattern like "~/dev/vce/**"
fn find_repos_under_pattern(pattern: &str) -> Vec<(String, String)> {
    let home = dirs::home_dir().unwrap_or_default();
    let dir = if let Some(tail) = pattern.strip_prefix("~/") {
        let tail = tail.trim_end_matches("**").trim_end_matches('/');
        home.join(tail)
    } else {
        let cleaned = pattern.trim_end_matches("**").trim_end_matches('/');
        PathBuf::from(cleaned)
    };

    let mut repos = Vec::new();

    if !dir.is_dir() {
        return repos;
    }

    // Find .git directories up to 4 levels deep
    collect_repos(&dir, 0, 4, &mut repos);
    repos
}

fn collect_repos(dir: &Path, depth: u32, max_depth: u32, repos: &mut Vec<(String, String)>) {
    if depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".git") {
                // Found a repo — get its remote URL
                if let Some(url) = get_remote_url(dir) {
                    if let Some((host, repo_path)) = crate::cli::parse_remote_url(&url) {
                        repos.push((host, repo_path));
                    }
                }
            } else if !path.file_name().is_some_and(|n| {
                n.to_string_lossy().starts_with('.') || n == "node_modules" || n == "target"
            }) {
                collect_repos(&path, depth + 1, max_depth, repos);
            }
        }
    }
}

fn get_remote_url(repo_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "remote",
            "get-url",
            "origin",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Group repos by (host, top-level org/path prefix)
fn group_repos_by_host_and_org(repos: &[(String, String)]) -> BTreeMap<(String, String), usize> {
    let mut map = BTreeMap::new();

    for (host, path) in repos {
        // Extract the top-level org: first path component for GitHub/GitLab,
        // first two components for Azure DevOps (v3/OrgName)
        let org = extract_org(host, path);
        *map.entry((host.clone(), org)).or_insert(0) += 1;
    }

    map
}

fn extract_org(host: &str, path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();

    if host.contains("dev.azure.com") {
        // Azure DevOps: v3/OrgName/Project/Repo → v3/OrgName
        if parts.len() >= 2 && parts[0] == "v3" {
            return format!("{}/{}", parts[0], parts[1]);
        }
    }

    // GitHub/GitLab/Gitea: Org/Repo → Org
    parts.first().unwrap_or(&"").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_include_key_extracts_gitdir_pattern() {
        assert_eq!(
            include_pattern_from_config_key("includeif.gitdir:~/work/**.path").as_deref(),
            Some("~/work/**")
        );
        assert_eq!(
            include_pattern_from_config_key("includeif.gitdir/i:/Users/me/Work/.path").as_deref(),
            Some("/Users/me/Work/")
        );
        assert!(include_pattern_from_config_key("includeif.onbranch:main.path").is_none());
    }

    #[test]
    fn parse_git_config_output_preserves_values_with_spaces() {
        let (key, value) =
            parse_git_config_key_value("includeif.gitdir:~/work/**.path ~/Work Configs/git")
                .unwrap();
        assert_eq!(key, "includeif.gitdir:~/work/**.path");
        assert_eq!(value, "~/Work Configs/git");
    }

    #[test]
    fn read_include_config_uses_git_config_parser() {
        let tmp = TempDir::new().unwrap();
        let config = tmp.path().join("included.gitconfig");
        std::fs::write(
            &config,
            r#"
[core]
    sshCommand = ssh -i "/tmp/key with space" -p 2222
[user]
    email = work@example.com
    name = "Work Name"
"#,
        )
        .unwrap();

        let (ssh_command, email, name) = read_include_config(&config);
        assert_eq!(
            ssh_command.as_deref(),
            Some("ssh -i /tmp/key with space -p 2222")
        );
        assert_eq!(email.as_deref(), Some("work@example.com"));
        assert_eq!(name.as_deref(), Some("Work Name"));
    }

    #[test]
    fn parse_ssh_command_handles_quoted_paths_and_port_options() {
        let (key, port) =
            parse_ssh_command_for_key_and_port(r#"ssh -o Port=443 -i "/Users/me/Keys/work key""#);
        assert_eq!(key.as_deref(), Some("/Users/me/Keys/work key"));
        assert_eq!(port, Some(443));

        let (key, port) =
            parse_ssh_command_for_key_and_port(r#"ssh -i /Users/me/Keys/work key -p 2222"#);
        assert_eq!(key.as_deref(), Some("/Users/me/Keys/work key"));
        assert_eq!(port, Some(2222));

        let (key, port) = parse_ssh_command_for_key_and_port(
            r#"ssh -oIdentityFile="/Users/me/Keys/another key" -p2222"#,
        );
        assert_eq!(key.as_deref(), Some("/Users/me/Keys/another key"));
        assert_eq!(port, Some(2222));
    }

    #[test]
    fn unresolved_include_becomes_manual_action_not_rule() {
        let tmp = TempDir::new().unwrap();
        let pattern = format!("{}/missing/**", tmp.path().display());
        let git_info = GitInfo {
            global_ssh_command: None,
            include_ifs: vec![IncludeIfRule {
                pattern: pattern.clone(),
                config_path: "~/.gitconfig-work".to_string(),
                ssh_command: Some(r#"ssh -i "/tmp/key with space""#.to_string()),
                email: Some("work@example.com".to_string()),
                name: Some("Work Name".to_string()),
            }],
            global_email: None,
            global_name: None,
        };

        let build = build_suggestions(&git_info, &[], &[]);
        assert!(build.rules.is_empty());
        assert_eq!(build.manual_rules.len(), 1);
        assert_eq!(build.manual_rules[0].pattern, pattern);
        assert_eq!(
            build.manual_rules[0].key_display,
            "/tmp/key with space".to_string()
        );
    }
}

// (end of file)
