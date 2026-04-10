<p align="center">
  <br>
  <br>
  <img src="logo.svg" width="160" alt="pickey logo">
  <br>
  <br>
</p>

# 🔑🤏 pickey

_/ˈpɪkiː/_

Automatic, observable SSH key picking, for Git.

## The problem

It's not always easy to make Git/SSH use the right key. Even when using the best workarounds, it's tedious to debug which key is being used, or why a push/pull is failing.

## How pickey works

pickey is a small, fast binary that sits as git's `sshCommand`. When git calls SSH, pickey matches the arguments against your rules and injects the right key.

Works at clone time. Works from your `/tmp` folder. Works when AI agents spawn terminals. Shell-agnostic. Immediate feedback on which key is being used.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/simeoncode/pickey/main/install.sh | sh
# then
pickey init # add --dry-run to only preview changes
```

## Usage

```bash
pickey [status]           # Shows pickey status, current repo rule
pickey init               # Scan SSH keys, write config, enable pickey
pickey init --dry-run     # Preview what init would do
pickey init --revert      # Undo all changes made by init
pickey check <url>        # Dry-run: rule matches for this remote
pickey list               # List all rules
pickey test               # Test SSH connection
```

## Configuration

`~/.config/pickey/config.toml`:

```toml
[[rule]]
host = "github.com"
match = "WORK-Internal/*"
key = "~/.ssh/id_work"
email = "email@work.com"
name = "My Name"

[[rule]]
host = "github.com"
match = "MyPersonalOrg/*"
key = "~/.ssh/id_personal"
```

Rules auto-detected by `pickey init` include `auto = true` — these are safely replaced when you re-run init. Manually added rules (without `auto = true`) are always preserved.

Rules are evaluated top-to-bottom, first match wins. `match` is a glob pattern against the full path after the host. If no rule matches, pickey falls through to plain `ssh` (with a warning).

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `host` | yes | Exact match against the SSH hostname |
| `match` | no | Glob against the path after the host. Omit to match any path on that host |
| `key` | yes | Path to private key (`~` expansion supported) |
| `port` | no | SSH port override (for non-standard ports) |
| `email` | no | If set, written to repo's local `user.email` after each SSH operation |
| `name` | no | If set, written to repo's local `user.name` after each SSH operation |
| `auto` | no | If `true`, this rule was auto-detected by `pickey init` and will be replaced on re-run |

## What it does NOT do

- Manage key lifecycle (create, delete, backup) — use `ssh-keygen`
- Touch `~/.ssh/config` — your global SSH config works as-is, for non-git SSH purposes (DBs, VMs, etc.)
- Handle non-git SSH — that's what `~/.ssh/config` Host entries are for (they work fine there)
- Act as an ssh-agent replacement

pickey solves exactly one problem: multiple identities on the same git host. For everything else (SSH to VMs, databases, etc.), the host is unique and `~/.ssh/config` already works.

