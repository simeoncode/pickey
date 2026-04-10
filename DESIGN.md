# pickey — Design Decisions

## The problem

SSH config is host-centric, but git forges are org-centric. You authenticate to `github.com` — but you might need three different SSH keys for three different GitHub orgs. SSH has no concept of "this key for this org."

The common workarounds all have a fatal flaw:

- **Host aliases** (`github.com-work` in `~/.ssh/config`) — contaminates your remote URLs with fake hostnames. Every clone URL must be rewritten. Breaks copy-paste, breaks scripts, breaks AI agents.
- **`includeIf "gitdir:"`** — ties identity to filesystem location. Breaks for `/tmp` clones, worktrees in unexpected paths, or any repo not under the expected directory.
- **`includeIf "hasconfig:remote.*.url:"`** — the remote doesn't exist yet during `git clone`, so the rule can't fire when it matters most.
- **SSH key managers** — indirection, complexity, or not built to work in tandem with git.

pickey solves this by sitting as git's `sshCommand`. At invocation time, the full remote URL is already in the SSH arguments — host, org, and repo. pickey matches that against rules and injects the right key. Works at clone time, works from `/tmp`, works when AI agents spawn terminals.

## Boundaries

pickey is a transparent SSH proxy for git. It does one thing: pick the right key.

**Does:**
- Select SSH key based on remote URL pattern
- Ensure the key is loaded in ssh-agent
- Set repo-local `user.email`/`user.name` after SSH operations
- Onboard from existing git config (`pickey init`)

**Does not:**
- Manage key lifecycle (create, rotate, delete) — use `ssh-keygen`
- Parse or modify `~/.ssh/config` — pickey's command-line flags (`-i`, `-o IdentitiesOnly=yes`) take precedence at runtime without touching SSH config
- Manage ssh-agent beyond loading matched keys — agent lifecycle is the OS/shell's job
- Handle HTTPS auth — SSH only

## FAQ / Decisions

### Why `sshCommand`, not SSH config rewriting

Git's `core.sshCommand` receives the full remote URL in its arguments. SSH config only sees the hostname. By operating at the sshCommand level, pickey has access to the org and repo path — the exact information needed to pick the right key. No host aliases, no URL rewriting, no fake hostnames.

### Why spawn+wait, not exec

pickey needs to perform post-SSH actions: setting `user.email` and `user.name` in the repo's local git config after a successful operation. If pickey exec'd into ssh, it would lose control. Instead, it spawns ssh as a child, passes through stdin/stdout (git needs them for transport), waits for exit, performs post-actions, then exits with ssh's code.

### Why `IdentitiesOnly=yes` is always injected

Without it, ssh offers every key in the agent to the server, regardless of what `-i` specifies. On forges like GitHub, the server accepts the first key that matches *any* account — which may not be the one pickey selected. `IdentitiesOnly=yes` tells ssh to only offer the key pickey chose. This is the entire point.

### Why `auto = true` exists

`pickey init` auto-detects rules from the user's existing SSH setup and writes them to config. But users also add their own rules manually. On re-run, init needs to update stale auto-detected rules without destroying manual ones. The `auto` field marks which rules init owns. Rules without `auto = true` are never touched by init.

### Why `git config --file` for conflict backup

When init finds conflicting `sshCommand` entries in includeIf configs or repo-local `.git/config`, it needs to disable them. Rather than text-manipulating config files, init uses `git config --file <path>` to read the current value, store it in `pickey.previousSshCommand` within the same file, and `--unset core.sshCommand`. Revert reads the backup and restores it. This uses git's own config system end-to-end — no parsing, no escaping bugs, clean round-trip.

### Why email/name in config?

Without pickey (or `includeIf`), it's easy to commit to a work repo with your personal email, or vice versa. If you set `email` and `name` on a rule, pickey writes them to the repo's local git config after each SSH operation — so the right identity is always used for commits, not just for SSH auth.

These fields are optional. If you only need SSH key routing, leave them out.

### Does pickey work with SSH over HTTPS (port 443)?

Yes. Because pickey sits on top of your system's SSH binary, any Hostname or Port overrides in your ~/.ssh/config are automatically respected. If you use explicit ssh://git@ssh.github.com:443 remotes, just use host = "ssh.github.com" in your pickey rules.
