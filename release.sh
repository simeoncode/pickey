#!/bin/sh
# Validates (v-prefix, semver, branch, clean, up-to-date, no dup tag, newer than latest)
# → cargo test → bump Cargo.toml → commit → tag → push
set -eu

VERSION="${1:?Usage: ./release.sh v0.1.1}"

# Must start with 'v'
case "$VERSION" in
  v*) ;;
  *)  echo "ERROR: version must start with 'v' (got: $VERSION)" >&2; exit 1 ;;
esac

# Must be valid semver: vX.Y.Z
SEMVER="${VERSION#v}"
case "$SEMVER" in
  [0-9]*.[0-9]*.[0-9]*)  ;;
  *)  echo "ERROR: invalid semver — expected vX.Y.Z (got: $VERSION)" >&2; exit 1 ;;
esac

# Ensure we're on main and clean
BRANCH="$(git branch --show-current)"
if [ "$BRANCH" != "main" ]; then
  echo "ERROR: not on main (on $BRANCH)" >&2
  exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
  echo "ERROR: working tree not clean" >&2
  exit 1
fi

# Ensure we're up to date with remote
git fetch origin main --quiet
LOCAL="$(git rev-parse HEAD)"
REMOTE="$(git rev-parse origin/main)"
if [ "$LOCAL" != "$REMOTE" ]; then
  echo "ERROR: local main is not up to date with origin/main" >&2
  echo "  local:  $LOCAL" >&2
  echo "  remote: $REMOTE" >&2
  exit 1
fi

# Check tag doesn't already exist
if git rev-parse "$VERSION" >/dev/null 2>&1; then
  echo "ERROR: tag $VERSION already exists" >&2
  exit 1
fi

# Check version is newer than current Cargo.toml version
CURRENT="$(grep '^version = ' Cargo.toml | head -1 | cut -d'"' -f2)"
if [ "$CURRENT" = "$SEMVER" ]; then
  echo "ERROR: version $SEMVER is already the current version in Cargo.toml" >&2
  exit 1
fi

# Check this version is newer than the latest git tag
LATEST_TAG="$(git tag --sort=-v:refname | head -1 || true)"
if [ -n "$LATEST_TAG" ]; then
  LATEST_SEMVER="${LATEST_TAG#v}"
  # Use sort -V to compare versions
  HIGHER="$(printf '%s\n%s\n' "$LATEST_SEMVER" "$SEMVER" | sort -V | tail -1)"
  if [ "$HIGHER" != "$SEMVER" ]; then
    echo "ERROR: $VERSION is not newer than latest tag $LATEST_TAG" >&2
    exit 1
  fi
fi

# Run tests before releasing
echo "Running tests..."
cargo test --quiet

# Bump version in Cargo.toml
sed -i '' "s/^version = \".*\"/version = \"${SEMVER}\"/" Cargo.toml

# Update Cargo.lock
cargo check --quiet

# Commit, tag, push
git add Cargo.toml Cargo.lock
git commit -m "release: ${VERSION}"
git tag "$VERSION"
git push origin main "$VERSION"

echo ""
echo "Released ${VERSION} — CI will build and attach binaries."
echo "https://github.com/simeoncode/pickey/releases/tag/${VERSION}"
