# pickey — TODO

## Done

- [x] Project setup — Rust, Cargo, dependencies (clap, toml, glob-match, serde, dirs)
- [x] Arg parser — SSH command line parsing, handles combined args from shell invocation
- [x] Config loader — TOML, `~` expansion, `port` field, `auto` field for init-managed rules
- [x] Rule matcher — Glob matching against host + path, first-match-wins
- [x] Agent interaction — Check/load keys in ssh-agent, macOS keychain support
- [x] SSH invocation — `-i`, `-o IdentitiesOnly=yes`, `-p` port injection, passthrough
- [x] Git config writer — `user.email`/`user.name` via `git config --local` after successful SSH
- [x] Logging — One-line stderr, `PICKEY_LOG=debug|off`
- [x] CLI — `status`, `check <url>`, `list`, `test`
- [x] Main entry — Detect sshCommand vs interactive, wire everything together
- [x] 25 unit tests — arg parsing, config, matching, URL parsing
- [x] Cross-compile — `cargo-zigbuild` for macOS (aarch64/x86_64) + Linux musl (aarch64/x86_64)
- [x] Local testing — Verified on real repos: GitHub, Azure DevOps, self-hosted Gitea (port 222)
- [x] `init` command — Scan `~/.ssh/` keys, detect `includeIf`/`sshCommand` configs, auto-generate rules
- [x] Init applies by default — No `--apply` flag needed, `--dry-run` for preview
- [x] Smart config merge — `auto = true` rules replaced on re-run, user rules preserved
- [x] `git config --file` conflict resolution — Back up and unset sshCommand entries reversibly
- [x] `init --revert` — Restore backed-up sshCommand values, unset global sshCommand
- [x] Status dashboard — Default no-arg behavior shows active/inactive + current repo match
- [x] Integration tests — eg. `pickey init` in a temp repo, verify config changes and SSH behavior

## Next

- [ ] Directory matching (dir = "~/work/*") support in rules
- [ ] GitHub Actions CI
- [ ] Install script (`install.sh`) — Detect OS/arch, copy binary, print instruction to run `pickey init`
- [ ] Do the SSH keys exist on disk? Warn if not, or if `ssh-add -L` doesn't list them (agent not running or keys not added)
- [ ] Config validation — Warn about rules that can never match (eg. port 22 with non-SSH URL), or duplicate rules

## Later
- [ ] Specificity Scoring — More specific rules (ie. more exact match, longer path or more attributes) override less specific ones, secondary sort by order in config file
- [ ] `rule add` CLI command — Interactive prompt to add a new rule, with validation (are there rules the can never match)
- [ ] `rule add/remove` CLI commands (`pickey rule add` adds interactively, `pickey config edit` etc.)
- [ ] E2E test harness — Scripted scenarios with real git commands
