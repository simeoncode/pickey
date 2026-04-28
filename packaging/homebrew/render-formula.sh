#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <version> <sha256sums> <output>" >&2
  exit 2
fi

VERSION="$1"
SHA256SUMS="$2"
OUTPUT="$3"

sha_for() {
  binary="$1"
  awk -v binary="$binary" '
    {
      path = $2
      sub(/^.*\//, "", path)
      if (path == binary) {
        print $1
      }
    }
  ' "$SHA256SUMS"
}

DARWIN_ARM_SHA="$(sha_for pickey-aarch64-apple-darwin)"
DARWIN_X86_SHA="$(sha_for pickey-x86_64-apple-darwin)"
LINUX_ARM_SHA="$(sha_for pickey-aarch64-unknown-linux-musl)"
LINUX_X86_SHA="$(sha_for pickey-x86_64-unknown-linux-musl)"

if [ -z "$DARWIN_ARM_SHA" ] || [ -z "$DARWIN_X86_SHA" ] ||
   [ -z "$LINUX_ARM_SHA" ] || [ -z "$LINUX_X86_SHA" ]; then
  echo "missing expected binary checksum in $SHA256SUMS" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"

cat > "$OUTPUT" <<EOF
class Pickey < Formula
  desc "Automatic SSH key selection for git"
  homepage "https://github.com/simeoncode/pickey"
  version "$VERSION"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/simeoncode/pickey/releases/download/v#{version}/pickey-aarch64-apple-darwin"
      sha256 "$DARWIN_ARM_SHA"
    else
      url "https://github.com/simeoncode/pickey/releases/download/v#{version}/pickey-x86_64-apple-darwin"
      sha256 "$DARWIN_X86_SHA"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/simeoncode/pickey/releases/download/v#{version}/pickey-aarch64-unknown-linux-musl"
      sha256 "$LINUX_ARM_SHA"
    else
      url "https://github.com/simeoncode/pickey/releases/download/v#{version}/pickey-x86_64-unknown-linux-musl"
      sha256 "$LINUX_X86_SHA"
    end
  end

  def install
    binary = Dir["pickey-*"].first
    chmod 0755, binary
    bin.install binary => "pickey"
  end

  test do
    assert_match "Automatic SSH key selection for git", shell_output("#{bin}/pickey --help")
  end
end
EOF
