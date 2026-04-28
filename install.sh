#!/bin/sh
set -eu

REPO="simeoncode/pickey"
VERSION="${PICKEY_VERSION:-latest}"
INSTALL_DIR="${PICKEY_INSTALL_DIR:-/usr/local/bin}"

# Detect OS
OS="$(uname -s)"
case "$OS" in
  Darwin) os="apple-darwin" ;;
  Linux)  os="unknown-linux-musl" ;;
  *)      echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

# Detect arch
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)  arch="x86_64" ;;
  aarch64|arm64)  arch="aarch64" ;;
  *)              echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

TARGET="${arch}-${os}"
BINARY="pickey-${TARGET}"

# Resolve version
if [ "$VERSION" = "latest" ]; then
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)"
fi

URL="https://github.com/${REPO}/releases/download/${VERSION}/${BINARY}"

echo "Installing pickey ${VERSION} (${TARGET})..."

# Download
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT
curl -fsSL -o "${TMPDIR}/pickey" "$URL"
chmod +x "${TMPDIR}/pickey"

# Install
if [ -w "$INSTALL_DIR" ]; then
  mv "${TMPDIR}/pickey" "${INSTALL_DIR}/pickey"
elif [ ! -e "$INSTALL_DIR" ] && [ -w "$(dirname "$INSTALL_DIR")" ]; then
  mkdir -p "$INSTALL_DIR"
  mv "${TMPDIR}/pickey" "${INSTALL_DIR}/pickey"
else
  echo "Installing to ${INSTALL_DIR} (requires sudo)..."
  sudo mkdir -p "$INSTALL_DIR"
  sudo mv "${TMPDIR}/pickey" "${INSTALL_DIR}/pickey"
fi

echo "Installed pickey to ${INSTALL_DIR}/pickey"
echo ""
echo "Next: run 'pickey init' to scan your SSH keys and enable pickey."
