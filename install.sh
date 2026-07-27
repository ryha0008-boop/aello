#!/bin/sh
# aello installer — download the latest prebuilt binary into a user-writable
# PATH dir. Mirrors the asset mapping in src/update.rs (Linux/Windows x86_64 and
# macOS arm64/x86_64 are published; everything else builds from source).
#
#   curl -fsSL https://raw.githubusercontent.com/ryha0008-boop/aello/main/install.sh | sh
#
# Override the install dir with AELLO_BIN_DIR (default ~/.local/bin).
set -eu

REPO="ryha0008-boop/aello"
BASE="https://github.com/$REPO/releases/download/latest"
BIN_DIR="${AELLO_BIN_DIR:-$HOME/.local/bin}"
SRC_HINT="build from source: cargo install --git https://github.com/$REPO"

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

os="$(uname -s)"
arch="$(uname -m)"
case "$arch" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
esac

case "$os" in
  Linux)
    [ "$arch" = "x86_64" ] || die "no prebuilt binary for $arch Linux — $SRC_HINT"
    asset="aello-x86_64-linux"; out="aello" ;;
  MINGW* | MSYS* | CYGWIN*)
    [ "$arch" = "x86_64" ] || die "no prebuilt binary for $arch Windows — $SRC_HINT"
    asset="aello-x86_64-windows.exe"; out="aello.exe" ;;
  Darwin)
    case "$arch" in
      aarch64) asset="aello-aarch64-macos" ;;
      x86_64)  asset="aello-x86_64-macos" ;;
      *) die "no prebuilt binary for $arch macOS — $SRC_HINT" ;;
    esac
    out="aello" ;;
  *)
    die "unsupported OS '$os' — $SRC_HINT" ;;
esac

url="$BASE/$asset"
dest="$BIN_DIR/$out"
mkdir -p "$BIN_DIR"

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

printf 'Downloading %s...\n' "$asset"
if command -v curl >/dev/null 2>&1; then
  curl -fSL "$url" -o "$tmp" || die "download failed from $url"
elif command -v wget >/dev/null 2>&1; then
  wget -qO "$tmp" "$url" || die "download failed from $url"
else
  die "need curl or wget to download"
fi

# Guard against a truncated download or an HTML error page silently becoming the
# installed binary — real aello builds are multi-MB (see src/update.rs).
size="$(wc -c < "$tmp")"
[ "$size" -ge 1048576 ] || die "download was only $size bytes — not a valid binary (network error or missing release asset)"

chmod +x "$tmp"
mv "$tmp" "$dest"
trap - EXIT

# The binaries are unsigned. On macOS a downloaded file carries a quarantine
# xattr and Gatekeeper refuses to run it ("cannot be opened because the
# developer cannot be verified"); strip it so the install just works.
if [ "$os" = "Darwin" ]; then
  xattr -d com.apple.quarantine "$dest" 2>/dev/null || true
fi

printf 'Installed aello to %s\n' "$dest"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) printf '\nNote: %s is not on your PATH. Add it, e.g.:\n  export PATH="%s:$PATH"\n' "$BIN_DIR" "$BIN_DIR" ;;
esac
