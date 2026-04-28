# Homebrew Packaging

This directory contains the formula generator used by the release workflow.

The public Homebrew formula lives in a separate tap repository:

```text
simeoncode/homebrew-tap
└── Formula
    └── pickey.rb
```

Users install from that tap with:

```bash
brew install simeoncode/tap/pickey
```

## Automation

The `Release` workflow updates the tap automatically after Release Please creates
a release. It:

1. Builds the release binaries.
2. Computes `SHA256SUMS`.
3. Renders `Formula/pickey.rb` with `render-formula.sh`.
4. Commits the formula into `simeoncode/homebrew-tap`.

One-time setup:

- Create the `simeoncode/homebrew-tap` repository.
- Add a repo secret named `HOMEBREW_TAP_TOKEN` with write access to that tap.

After that, the recurring release path is just accepting the Release Please PR.
