use std::path::{Path, PathBuf};
use std::process::Command;

use crate::log;

/// Determines which directory to write git config into.
/// Returns the absolute path to the target repo directory.
///
/// Strategy:
/// 1. Check for clone target (./repo_name/.git relative to cwd) — during clone,
///    CWD may be a different repo, so this must be checked first.
/// 2. Fall back to CWD if it's inside a git repo (covers fetch/push).
fn resolve_target(cwd: &Path, repo_path: &str) -> Option<PathBuf> {
    // Derive clone target from repo path (last segment, e.g. "WorkOrg/repo" → "repo")
    let repo_name = repo_path.rsplit('/').next().unwrap_or(repo_path);
    let clone_candidate = cwd.join(repo_name);

    // Check clone target first — during clone, CWD might be an unrelated repo
    if clone_candidate.join(".git").is_dir() {
        return Some(clone_candidate);
    }

    // Fall back to CWD (covers fetch/push inside an existing repo)
    let in_repo = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if in_repo {
        Some(cwd.to_path_buf())
    } else {
        None
    }
}

/// Set git local config for user.email and/or user.name.
/// Checks for a clone target directory first (e.g. "WorkOrg/repo" → "./repo"),
/// since during clone CWD may be an unrelated repo. Falls back to CWD for
/// fetch/push inside an existing repo.
pub fn set_local_config(email: Option<&str>, name: Option<&str>, repo_path: &str) {
    if email.is_none() && name.is_none() {
        return;
    }

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => {
            log::debug("Cannot determine CWD, skipping config write");
            return;
        }
    };

    let target = match resolve_target(&cwd, repo_path) {
        Some(t) => {
            if t != cwd {
                log::debug(&format!("Clone detected, using {}", t.display()));
            }
            t
        }
        None => {
            log::debug("Not in a git repo and clone target not found, skipping config write");
            return;
        }
    };

    fn git_config_set(
        target: &Path,
        key: &str,
        value: &str,
    ) -> std::io::Result<std::process::Output> {
        Command::new("git")
            .arg("-C")
            .arg(target)
            .args(["config", "--local", key, value])
            .output()
    }

    if let Some(email) = email {
        match git_config_set(&target, "user.email", email) {
            Ok(o) if o.status.success() => {
                log::debug(&format!("Set user.email = {}", email));
            }
            Ok(o) => {
                log::warn(&format!(
                    "Failed to set user.email: {}",
                    String::from_utf8_lossy(&o.stderr)
                ));
            }
            Err(e) => {
                log::warn(&format!("Failed to run git config: {}", e));
            }
        }
    }

    if let Some(name) = name {
        match git_config_set(&target, "user.name", name) {
            Ok(o) if o.status.success() => {
                log::debug(&format!("Set user.name = {}", name));
            }
            Ok(o) => {
                log::warn(&format!(
                    "Failed to set user.name: {}",
                    String::from_utf8_lossy(&o.stderr)
                ));
            }
            Err(e) => {
                log::warn(&format!("Failed to run git config: {}", e));
            }
        }
    }
}

/// Pre-flight check before push: are there tracked unpushed commits with the wrong email?
/// Returns true if the push should be aborted.
///
/// Skipped if PICKEY_ALLOW_EMAIL=1 is set in the environment.
pub fn check_email_before_push(expected_email: &str, repo_path: &str) -> bool {
    // Allow bypass via env var
    if std::env::var("PICKEY_ALLOW_EMAIL").as_deref() == Ok("1") {
        return false;
    }

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => return false,
    };

    let target = match resolve_target(&cwd, repo_path) {
        Some(t) => t,
        None => return false,
    };

    let mismatched = match tracked_mismatched_emails(&target, expected_email) {
        Some(mismatched) => mismatched,
        None => return false,
    };

    if mismatched.is_empty() {
        return false;
    }

    let list = mismatched.join(", ");

    log::error(&format!(
        "Push blocked: this repo has commits authored with {}, \
         but the rule expects {}.",
        list, expected_email,
    ));
    eprintln!();
    eprintln!("  Commits checked by pickey:");
    eprintln!("    git log --format='%h %ae %s' @{{u}}..HEAD");
    eprintln!();
    eprintln!("  To fix the mismatched commits:");
    eprintln!("    git rebase -i @{{u}} --exec 'git commit --amend --reset-author --no-edit'");
    eprintln!();
    eprintln!("  To bypass this check just for this push:");
    eprintln!("    PICKEY_ALLOW_EMAIL=1 git push");
    true
}

/// Find unique author emails in tracked unpushed commits that don't match the expected email.
/// Only checks @{u}..HEAD, since that is the exact range git will compare for a tracked branch.
#[cfg(test)]
fn find_mismatched_emails(target: &Path, expected_email: &str) -> Vec<String> {
    tracked_mismatched_emails(target, expected_email).unwrap_or_default()
}

fn tracked_mismatched_emails(target: &Path, expected_email: &str) -> Option<Vec<String>> {
    let stdout = match git_log_emails(target, &["@{u}..HEAD"]) {
        Some(stdout) => stdout,
        None => {
            log::debug("Skipping push email check: current branch has no upstream");
            return None;
        }
    };

    let expected_lower = expected_email.to_lowercase();

    let mut seen = std::collections::HashSet::new();
    let mismatched = stdout
        .lines()
        .filter(|e| !e.is_empty() && e.to_lowercase() != expected_lower)
        .filter(|e| seen.insert(e.to_lowercase()))
        .map(|e| e.to_string())
        .collect();

    Some(mismatched)
}

fn git_log_emails(target: &Path, revisions: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(target)
        .arg("log")
        .arg("--format=%ae")
        .args(revisions)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    /// Helper: read a local git config value
    fn git_config_get(dir: &Path, key: &str) -> Option<String> {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["config", "--local", key])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    /// Helper: init a git repo
    fn git_init(dir: &Path) {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q"])
            .output()
            .expect("git init failed");
    }

    /// Helper: set a local git config value
    fn git_config_set(dir: &Path, key: &str, value: &str) {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["config", "--local", key, value])
            .output()
            .expect("git config set failed");
    }

    fn git_update_ref(dir: &Path, reference: &str, revision: &str) {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["update-ref", reference, revision])
            .output()
            .expect("git update-ref failed");
        assert!(output.status.success(), "git update-ref should succeed");
    }

    fn git_current_branch(dir: &Path) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["branch", "--show-current"])
            .output()
            .expect("git branch --show-current failed");
        assert!(
            output.status.success(),
            "git branch --show-current should succeed"
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    // ---- resolve_target tests (pure path logic, no CWD mutation) ----

    /// Clone from a non-repo dir: CWD=/tmp/xxx, clone target=./myrepo
    #[test]
    fn resolve_clone_from_non_repo_dir() {
        let tmp = TempDir::new().unwrap();
        let clone_target = tmp.path().join("myrepo");
        std::fs::create_dir(&clone_target).unwrap();
        git_init(&clone_target);

        let result = resolve_target(tmp.path(), "WorkOrg/myrepo");
        assert_eq!(result.as_deref(), Some(clone_target.as_path()));
    }

    /// Clone from inside another repo: CWD=repo_a, clone target=repo_a/myrepo
    /// Must return the clone target, not repo_a
    #[test]
    fn resolve_clone_from_inside_another_repo() {
        let tmp = TempDir::new().unwrap();
        let repo_a = tmp.path().join("repo_a");
        std::fs::create_dir(&repo_a).unwrap();
        git_init(&repo_a);

        let clone_target = repo_a.join("myrepo");
        std::fs::create_dir(&clone_target).unwrap();
        git_init(&clone_target);

        let result = resolve_target(&repo_a, "WorkOrg/myrepo");
        assert_eq!(result.as_deref(), Some(clone_target.as_path()));
    }

    /// Fetch/push inside an existing repo: CWD=the repo, no clone subdir
    #[test]
    fn resolve_fetch_push_inside_repo() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("myrepo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        let result = resolve_target(&repo, "WorkOrg/myrepo");
        // No ./myrepo subdir inside repo, so falls back to CWD
        assert_eq!(result.as_deref(), Some(repo.as_path()));
    }

    /// Not in a repo and no clone target → None
    #[test]
    fn resolve_no_repo_no_clone_target() {
        let tmp = TempDir::new().unwrap();
        let result = resolve_target(tmp.path(), "WorkOrg/nonexistent");
        assert_eq!(result, None);
    }

    // ---- integration tests (actually write git config) ----

    /// Clone: writes email+name to clone target, not to CWD
    #[test]
    fn integration_clone_writes_to_clone_target() {
        let tmp = TempDir::new().unwrap();
        let parent_repo = tmp.path().join("parent");
        std::fs::create_dir(&parent_repo).unwrap();
        git_init(&parent_repo);
        git_config_set(&parent_repo, "user.email", "personal@home.com");

        let clone_target = parent_repo.join("myrepo");
        std::fs::create_dir(&clone_target).unwrap();
        git_init(&clone_target);

        // Simulate: CWD=parent_repo, cloning WorkOrg/myrepo
        let _cwd = CwdGuard::new(&parent_repo);
        super::set_local_config(Some("work@corp.com"), Some("Work Name"), "WorkOrg/myrepo");

        // Clone target gets work email
        assert_eq!(
            git_config_get(&clone_target, "user.email").as_deref(),
            Some("work@corp.com")
        );
        assert_eq!(
            git_config_get(&clone_target, "user.name").as_deref(),
            Some("Work Name")
        );
        // Parent repo is untouched
        assert_eq!(
            git_config_get(&parent_repo, "user.email").as_deref(),
            Some("personal@home.com")
        );
    }

    /// Fetch/push: writes to CWD repo
    #[test]
    fn integration_fetch_writes_to_cwd_repo() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("therepo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        let _cwd = CwdGuard::new(&repo);
        super::set_local_config(Some("work@corp.com"), None, "WorkOrg/therepo");

        assert_eq!(
            git_config_get(&repo, "user.email").as_deref(),
            Some("work@corp.com")
        );
        assert_eq!(git_config_get(&repo, "user.name"), None);
    }

    /// Noop when email=None and name=None
    #[test]
    fn integration_noop_when_nothing_set() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("myrepo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        let _cwd = CwdGuard::new(&repo);
        super::set_local_config(None, None, "WorkOrg/myrepo");

        assert_eq!(git_config_get(&repo, "user.email"), None);
    }

    /// Tracked branches should block when commits since @{u} use the wrong email.
    #[test]
    fn preflight_tracked_branch_blocks_push() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("my-repo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:WorkOrg/my-repo.git",
            ])
            .output()
            .unwrap();

        git_config_set(&repo, "user.email", "work@corp.com");
        git_config_set(&repo, "user.name", "Work Name");
        std::fs::write(repo.join("base.txt"), "base").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "base.txt"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "base commit"])
            .output()
            .unwrap();

        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["checkout", "-q", "-b", "feature"])
            .output()
            .unwrap();

        git_update_ref(&repo, "refs/remotes/origin/feature", "HEAD");
        git_config_set(&repo, "branch.feature.remote", "origin");
        git_config_set(&repo, "branch.feature.merge", "refs/heads/feature");

        // User commits with personal email
        git_config_set(&repo, "user.email", "personal@gmail.com");
        git_config_set(&repo, "user.name", "Personal Me");
        std::fs::write(repo.join("README.md"), "# my-repo").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "first commit"])
            .output()
            .unwrap();

        // Pre-flight check should block because the branch tracks origin/feature
        let blocked = find_mismatched_emails(&repo, "work@corp.com");
        assert!(!blocked.is_empty(), "Should detect mismatched email");
        assert!(blocked.iter().any(|e| e == "personal@gmail.com"));
    }

    /// PICKEY_ALLOW_EMAIL=1 should bypass the pre-flight check
    #[test]
    fn preflight_env_bypass_allows_push() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("my-repo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        git_config_set(&repo, "user.email", "personal@gmail.com");
        git_config_set(&repo, "user.name", "Me");
        std::fs::write(repo.join("README.md"), "# test").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "first"])
            .output()
            .unwrap();

        // With PICKEY_ALLOW_EMAIL=1, check should be skipped
        std::env::set_var("PICKEY_ALLOW_EMAIL", "1");
        let _cwd = CwdGuard::new(&repo);
        let should_abort = super::check_email_before_push("work@corp.com", "WorkOrg/my-repo");
        std::env::remove_var("PICKEY_ALLOW_EMAIL");
        assert!(!should_abort, "PICKEY_ALLOW_EMAIL=1 should bypass check");
    }

    /// No mismatched commits on a tracked branch → push should proceed
    #[test]
    fn preflight_no_mismatch_allows_push() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("my-repo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:WorkOrg/my-repo.git",
            ])
            .output()
            .unwrap();

        git_config_set(&repo, "user.email", "work@corp.com");
        git_config_set(&repo, "user.name", "Work Name");
        std::fs::write(repo.join("README.md"), "# test").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "correct email"])
            .output()
            .unwrap();

        let branch = git_current_branch(&repo);
        git_update_ref(&repo, &format!("refs/remotes/origin/{}", branch), "HEAD");
        git_config_set(&repo, &format!("branch.{}.remote", branch), "origin");
        git_config_set(
            &repo,
            &format!("branch.{}.merge", branch),
            &format!("refs/heads/{}", branch),
        );

        let mismatched = find_mismatched_emails(&repo, "work@corp.com");
        assert!(mismatched.is_empty(), "No mismatch should be found");
    }

    /// New branches without an upstream should not block, even if history contains
    /// other author emails, because pickey cannot prove what will be pushed yet.
    #[test]
    fn preflight_new_branch_without_upstream_skips_check() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("my-repo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        git_config_set(&repo, "user.email", "personal@gmail.com");
        git_config_set(&repo, "user.name", "Personal Me");
        std::fs::write(repo.join("feature.txt"), "feature").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "feature.txt"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "local personal"])
            .output()
            .unwrap();

        let mismatched = find_mismatched_emails(&repo, "work@corp.com");
        assert!(
            mismatched.is_empty(),
            "No upstream should mean no blocking preflight result"
        );
    }

    /// Empty repo (no commits) → no mismatch
    #[test]
    fn preflight_empty_repo_no_mismatch() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("my-repo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        let mismatched = find_mismatched_emails(&repo, "work@corp.com");
        assert!(mismatched.is_empty());
    }

    /// After pickey sets email, new commits are correct (full workflow)
    #[test]
    fn integration_github_workflow_future_commits_correct() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("my-repo");
        std::fs::create_dir(&repo).unwrap();
        git_init(&repo);

        // First commit with wrong email
        git_config_set(&repo, "user.email", "personal@gmail.com");
        git_config_set(&repo, "user.name", "Personal Me");
        std::fs::write(repo.join("README.md"), "# my-repo").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "first commit"])
            .output()
            .unwrap();

        // pickey sets local config (post-SSH, after the push goes through)
        let _cwd = CwdGuard::new(&repo);
        super::set_local_config(Some("work@corp.com"), Some("Work Name"), "WorkOrg/my-repo");

        // Future commits use correct email
        assert_eq!(
            git_config_get(&repo, "user.email").as_deref(),
            Some("work@corp.com")
        );

        std::fs::write(repo.join("file2.txt"), "hello").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "file2.txt"])
            .output()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "second commit"])
            .output()
            .unwrap();
        let new_email = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["log", "-1", "--format=%ae"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&new_email.stdout).trim(),
            "work@corp.com"
        );
    }

    /// RAII guard for CWD (integration tests that exercise set_local_config).
    /// Uses a mutex to prevent parallel CWD changes.
    struct CwdGuard {
        prev: std::path::PathBuf,
    }

    static CWD_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl CwdGuard {
        fn new(dir: &Path) -> (std::sync::MutexGuard<'static, ()>, Self) {
            let lock = CWD_MUTEX.lock().unwrap();
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir).unwrap();
            (lock, CwdGuard { prev })
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev);
        }
    }
}
