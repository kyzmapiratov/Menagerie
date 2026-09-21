#!/usr/bin/env bash
#
# Packs a built binary into the release tarball that `install.sh --binary` installs:
#
#   packaging/make-tarball.sh <binary> <output-dir> [version]
#
# Produces menagerie-linux-<arch>.tar.gz and a .sha256 next to it. The name has no
# version in it on purpose: ".../releases/latest/download/<name>" then always finds the
# newest release. The version is the top folder inside the archive.
# The release workflow runs this; it is also how to test the binary installer locally.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"

bin="${1:?usage: make-tarball.sh <binary> <output-dir> [version]}"
out="${2:?usage: make-tarball.sh <binary> <output-dir> [version]}"
version="${3:-$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$root/src-tauri/Cargo.toml" | head -1)}"

arch="$(uname -m)"
name="menagerie-linux-$arch"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

top="menagerie-$version-linux-$arch"
"$here/stage.sh" "$bin" "$work/$top"

mkdir -p "$out"
tar -C "$work" -czf "$out/$name.tar.gz" "$top"
(cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")

echo "$out/$name.tar.gz"
