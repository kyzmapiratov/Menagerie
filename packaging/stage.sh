#!/usr/bin/env bash
#
# Lays out an installed copy of Menagerie under a directory, the way
# `make install` would: binary, launcher entry, icons, AppStream metadata, licenses.
# Everything that installs the app (install.sh, the release tarball, the Arch
# package) goes through here, so they cannot drift apart.
#
#   packaging/stage.sh <binary> <destination> [exec]
#
# <destination> is a prefix such as /usr or ~/.local; the tree is created inside it.
# [exec] is what the launcher entry runs (default: menagerie, found on PATH).

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"

bin="${1:?usage: stage.sh <binary> <destination> [exec]}"
dest="${2:?usage: stage.sh <binary> <destination> [exec]}"
exec_path="${3:-menagerie}"

[[ -x "$bin" ]] || { echo "stage.sh: $bin is not an executable file" >&2; exit 1; }

icons="$dest/share/icons/hicolor"

install -Dm755 "$bin" "$dest/bin/menagerie"

# The launcher entry is written with the path it should run.
sed "s|^Exec=.*|Exec=$exec_path|" "$here/menagerie.desktop" |
  install -Dm644 /dev/stdin "$dest/share/applications/menagerie.desktop"

install -Dm644 "$root/src-tauri/icons/32x32.png"       "$icons/32x32/apps/menagerie.png"
install -Dm644 "$root/src-tauri/icons/128x128.png"     "$icons/128x128/apps/menagerie.png"
install -Dm644 "$root/src-tauri/icons/256x256.png"     "$icons/256x256/apps/menagerie.png"
install -Dm644 "$root/src-tauri/icons/icon.png"        "$icons/512x512/apps/menagerie.png"
install -Dm644 "$here/menagerie.svg"             "$icons/scalable/apps/menagerie.svg"

install -Dm644 "$here/io.github.kyzmapiratov.Menagerie.metainfo.xml" \
  "$dest/share/metainfo/io.github.kyzmapiratov.Menagerie.metainfo.xml"

install -Dm644 "$root/LICENSE"        "$dest/share/licenses/menagerie/LICENSE"
install -Dm644 "$root/docs/third-party.md" "$dest/share/licenses/menagerie/THIRD-PARTY.md"
