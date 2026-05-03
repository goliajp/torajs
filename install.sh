#!/usr/bin/env bash
# torajs installer — fetch the latest release tarball for the
# detected platform, extract to ~/.torajs, optionally PATH-hint.
#
# Usage:
#   curl -fsSL https://install.torajs.com | bash                # vanity domain
#   curl -fsSL https://raw.githubusercontent.com/goliajp/torajs/main/install.sh | bash
#
# Override:
#   TORAJS_REPO=goliajp/torajs   — repo to fetch from (default below)
#   TORAJS_VERSION=v0.1.0-beta   — pin a specific version
#   TORAJS_PREFIX=$HOME/.torajs  — install root

set -euo pipefail

repo="${TORAJS_REPO:-goliajp/torajs}"
prefix="${TORAJS_PREFIX:-$HOME/.torajs}"
version="${TORAJS_VERSION:-}"

red()   { printf '\033[31m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
blue()  { printf '\033[34m%s\033[0m\n' "$*"; }

need() {
  command -v "$1" >/dev/null 2>&1 || { red "missing: $1"; exit 1; }
}
need curl
need tar
need shasum

# Detect platform.
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin)  os_label="darwin" ;;
  Linux)   os_label="linux" ;;
  *)       red "unsupported OS: $os"; exit 1 ;;
esac
case "$arch" in
  arm64|aarch64)   arch_label="arm64" ;;
  x86_64|amd64)    arch_label="x64" ;;
  *)               red "unsupported arch: $arch"; exit 1 ;;
esac

if [ "$os_label" = "darwin" ] && [ "$arch_label" = "x64" ]; then
  red "torajs darwin-x64 binaries aren't published yet — build from source via cargo, or run on Apple Silicon."
  exit 1
fi
if [ "$os_label" = "linux" ] && [ "$arch_label" = "arm64" ]; then
  red "torajs linux-arm64 binaries aren't published yet — build from source via cargo."
  exit 1
fi

platform="${os_label}-${arch_label}"

# Resolve version (latest release if unset).
if [ -z "$version" ]; then
  blue "looking up latest release..."
  # /releases/latest excludes prereleases (404s when only a -beta /
  # -rc release exists), so fall back to /releases (full list) and
  # take the topmost tag — GitHub returns it newest-first regardless
  # of prerelease flag.
  version="$(curl -fsSL "https://api.github.com/repos/${repo}/releases?per_page=20" \
    | grep '"tag_name"' | head -n1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
fi
if [ -z "$version" ]; then
  red "could not determine release version (set TORAJS_VERSION=v0.1.0-beta)"
  exit 1
fi

archive="tr-${version}-${platform}.tar.gz"
url="https://github.com/${repo}/releases/download/${version}/${archive}"
sha_url="${url}.sha256"

blue "downloading ${archive}..."
mkdir -p "$prefix"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL -o "$tmp/$archive" "$url"
curl -fsSL -o "$tmp/$archive.sha256" "$sha_url"

# Verify checksum.
expected="$(awk '{print $1}' "$tmp/$archive.sha256")"
actual="$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')"
if [ "$expected" != "$actual" ]; then
  red "checksum mismatch:"
  red "  expected: $expected"
  red "  actual:   $actual"
  exit 1
fi
green "checksum verified"

blue "installing to ${prefix}..."
tar -xzf "$tmp/$archive" -C "$tmp"
mkdir -p "$prefix/bin" "$prefix/share"
cp "$tmp/tr/tr" "$prefix/bin/tr"
chmod +x "$prefix/bin/tr"
cp -r "$tmp/tr/docs" "$prefix/share/docs"
cp -r "$tmp/tr/examples" "$prefix/share/examples"
cp "$tmp/tr/README.md" "$prefix/share/README.md" 2>/dev/null || true

green "installed tr ${version} to ${prefix}/bin/tr"

# PATH hint.
case ":$PATH:" in
  *":$prefix/bin:"*) ;;
  *)
    echo
    blue "add tr to your PATH by appending one of these to your shell rc:"
    echo "  export PATH=\"$prefix/bin:\$PATH\""
    ;;
esac

echo
echo "verify:"
echo "  $prefix/bin/tr --version"
echo
echo "next steps:"
echo "  cd $prefix/share/examples/sha256 && tr run sha256.ts"
echo "  see $prefix/share/docs/getting-started.md"
