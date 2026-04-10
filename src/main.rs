mod agent;
mod args;
mod cli;
mod config;
mod gitconfig;
mod init;
mod log;
mod matcher;
mod ssh;

use clap::{Parser, Subcommand};
use std::process;

#[derive(Parser)]
#[command(name = "pickey", about = "Automatic SSH key selection for git")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Remaining args (when invoked as sshCommand by git)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    ssh_args: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show which key + email will be used for the current repo
    Status,
    /// Dry-run: what would match for this URL
    Check {
        /// Git remote URL or host:org/repo shorthand
        url: String,
    },
    /// List all rules and agent status
    List,
    /// SSH to the forge, show which identity it returns
    Test,
    /// Scan SSH keys and git config, set up pickey
    Init {
        /// Preview what init would do without making changes
        #[arg(long)]
        dry_run: bool,
        /// Undo all changes made by init
        #[arg(long)]
        revert: bool,
    },
}

fn main() {
    // Detect if we're being invoked as sshCommand (SSH-style args present)
    // vs interactively (subcommand or no args).
    let raw_args: Vec<String> = std::env::args().collect();

    // If we have args that look like SSH invocation (not a known subcommand),
    // go straight to sshCommand mode.
    if raw_args.len() > 1 && is_ssh_invocation(&raw_args[1..]) {
        run_ssh_command(&raw_args[1..]);
        return;
    }

    // Otherwise, parse as CLI
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Status) => {
            let config = load_config_or_exit();
            cli::status(&config);
        }
        Some(Commands::Check { url }) => {
            let config = load_config_or_exit();
            cli::check(&config, &url);
        }
        Some(Commands::List) => {
            let config = load_config_or_exit();
            cli::list(&config);
        }
        Some(Commands::Test) => {
            let config = load_config_or_exit();
            cli::test(&config);
        }
        Some(Commands::Init { dry_run, revert }) => {
            if revert {
                init::revert();
            } else {
                init::init(dry_run);
            }
        }
        None => {
            if !cli.ssh_args.is_empty() {
                run_ssh_command(&cli.ssh_args);
            } else {
                // No subcommand — show status if config exists, otherwise help
                match config::load_config(None) {
                    Ok(config) => cli::status(&config),
                    Err(_) => {
                        use clap::CommandFactory;
                        Cli::command().print_help().ok();
                        println!();
                    }
                }
            }
        }
    }
}

/// Detect if args look like an SSH invocation from git vs a CLI subcommand.
fn is_ssh_invocation(args: &[String]) -> bool {
    let known_subcommands = [
        "status",
        "check",
        "list",
        "test",
        "init",
        "help",
        "--help",
        "-h",
        "--version",
        "-V",
    ];
    if let Some(first) = args.first() {
        if known_subcommands.contains(&first.as_str()) {
            return false;
        }
        if first.starts_with('-') {
            return true;
        }
        if first.contains('@') {
            return true;
        }
        if args.len() >= 2 && args[1].starts_with("git-") {
            return true;
        }
    }
    false
}

fn run_ssh_command(args: &[String]) {
    let config = match config::load_config(None) {
        Ok(c) => c,
        Err(e) => {
            log::warn(&format!("{}; falling through to plain ssh", e));
            let code = ssh::passthrough_ssh(args).unwrap_or(1);
            process::exit(code);
        }
    };

    let invocation = match args::parse_ssh_args(args) {
        Some(inv) => inv,
        None => {
            log::warn("Could not parse SSH args; falling through to plain ssh");
            let code = ssh::passthrough_ssh(args).unwrap_or(1);
            process::exit(code);
        }
    };

    log::debug(&format!(
        "Parsed: host={} path={}",
        invocation.host, invocation.path
    ));

    match matcher::find_match(&config.rules, &invocation.host, &invocation.path) {
        Some(m) => {
            let key_path = m.rule.expanded_key();

            // Verify key exists on disk
            if !key_path.exists() {
                log::error(&format!("Key not found: {}", key_path.display()));
                process::exit(1);
            }

            // Ensure key is in agent
            let agent_status = match agent::ensure_key_loaded(&key_path, config.apple_keychain) {
                Ok(true) => "loaded",
                Ok(false) => "just loaded",
                Err(e) => {
                    log::error(&format!("Agent error: {}", e));
                    process::exit(1);
                }
            };

            log::info(&format!(
                "{}/{} → {} (agent: {})",
                invocation.host, invocation.path, m.rule.key, agent_status
            ));

            // Pre-flight: abort push if commits have wrong email
            if invocation.is_push {
                if let Some(email) = m.rule.email.as_deref() {
                    if gitconfig::check_email_before_push(email, &invocation.path) {
                        process::exit(1);
                    }
                }
            }

            // Invoke ssh with the matched key
            let id_only = ssh::has_identities_only(args);
            let code = ssh::invoke_ssh(args, &key_path.to_string_lossy(), id_only, m.rule.port)
                .unwrap_or(1);

            // Post-SSH: set git local config if rule specifies email/name
            if code == 0 {
                gitconfig::set_local_config(
                    m.rule.email.as_deref(),
                    m.rule.name.as_deref(),
                    &invocation.path,
                );
            }

            process::exit(code);
        }
        None => {
            let hint =
                agent::default_key_hint().unwrap_or_else(|| "unknown (agent empty?)".to_string());
            log::warn(&format!(
                "{}/{} → no matching rule, falling through to ssh default: {}",
                invocation.host, invocation.path, hint
            ));
            let code = ssh::passthrough_ssh(args).unwrap_or(1);
            process::exit(code);
        }
    }
}

fn load_config_or_exit() -> config::Config {
    match config::load_config(None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}
