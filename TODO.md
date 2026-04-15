# pickey — TODO

## Done

v0.1 — Arg parsing, config (TOML), rule matching, SSH key injection (`-i`, `IdentitiesOnly`, `IdentityAgent=none`), git config writer, logging, CLI (`status`/`check`/`list`/`test`), `init`/`--revert`, cross-compile (macOS + Linux musl), 40 unit+integration tests, install script, GitHub Actions CI+release.

## Next

- [ ] `pickey check` interactive rule creation — When no rule matches, offer to create one (pick key, set email/name, write to config)
- [ ] Directory matching (dir = "~/work/*") support in rules
- [ ] GitHub Actions CI
- [ ] Install script (`install.sh`) — Detect OS/arch, copy binary, print instruction to run `pickey init`
- [ ] Config validation — Warn about rules that can never match (eg. port 22 with non-SSH URL), or duplicate rules

## Later
- [ ] Specificity Scoring — More specific rules (ie. more exact match, longer path or more attributes) override less specific ones, secondary sort by order in config file
- [ ] `rule add` CLI command — Interactive prompt to add a new rule, with validation (are there rules the can never match)
- [ ] `rule add/remove` CLI commands (`pickey rule add` adds interactively, `pickey config edit` etc.)
- [ ] E2E test harness — Scripted scenarios with real git commands
