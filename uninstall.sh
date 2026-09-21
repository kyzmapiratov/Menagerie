#!/usr/bin/env bash
#
# Menagerie — uninstaller. Removes what install.sh installed.
#
#   ./uninstall.sh                 remove the app installed under ~/.local
#   ./uninstall.sh --prefix DIR    it was installed somewhere else (--system means /usr/local)
#   ./uninstall.sh --engine        also remove wl_shimeji, if install.sh is what built it
#   ./uninstall.sh --purge         also delete the app's own data (settings, favorites, presets)
#   -y, --yes                      do not ask;   -n, --dry-run   only show what would go
#
# install.sh recorded every file it copied; only those are removed, nothing else.
# Your characters (in ~/.local/share/wl_shimeji) belong to the engine and are never touched.
# If you installed from a package (.deb, .rpm, the AUR), remove it with your package manager.

set -euo pipefail

APP="menagerie"
PREFIX="${PREFIX:-$HOME/.local}"
ENGINE_PREFIX="/usr/local"
DO_ENGINE=0
PURGE=0
ASSUME_YES=0
DRY=0

if [[ -t 1 ]]; then B=$'\033[1m'; D=$'\033[2m'; Y=$'\033[33m'; G=$'\033[32m'; C=$'\033[1;34m'; Z=$'\033[0m'
else B=""; D=""; Y=""; G=""; C=""; Z=""; fi

step() { printf '\n%s==>%s %s%s%s\n' "$C" "$Z" "$B" "$*" "$Z"; }
info() { printf '    %s\n' "$*"; }
good() { printf '    %s✓%s %s\n' "$G" "$Z" "$*"; }
warn() { printf '%s  ! %s%s\n' "$Y" "$*" "$Z" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() { awk 'NR > 2 && /^#/ { sub(/^# ?/, ""); print; next } NR > 2 { exit }' "${BASH_SOURCE[0]}"; exit 0; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) PREFIX="${2:?--prefix needs a directory}"; shift ;;
    --prefix=*) PREFIX="${1#*=}" ;;
    --system) PREFIX=/usr/local ;;
    --engine-prefix) ENGINE_PREFIX="${2:?--engine-prefix needs a directory}"; shift ;;
    --engine) DO_ENGINE=1 ;;
    --purge) PURGE=1 ;;
    -y|--yes) ASSUME_YES=1 ;;
    -n|--dry-run) DRY=1 ;;
    -h|--help) usage ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

confirm() {
  if [[ $ASSUME_YES -eq 1 || ! -r /dev/tty ]]; then return 0; fi
  local answer=""
  printf '    %s [y/N] ' "$1" > /dev/tty
  read -r answer < /dev/tty || answer=""
  [[ "$answer" =~ ^[Yy] ]]
}

# Removes every file listed in <root>/share/menagerie-install/<manifest>, then the folders
# that this leaves empty. Returns 1 when there is no such record.
remove_recorded() {
  local root="$1" name="$2" priv=() file dir count=0
  local record="$root/share/$APP-install/$name"
  [[ -f "$record" ]] || return 1
  if [[ ! -w "$root/share" ]]; then priv=(sudo); fi
  while IFS= read -r file; do
    [[ -n "$file" && -e "$root/$file" ]] || continue
    # The record is a plain text file; never follow a line that leaves the prefix.
    case "$file" in /* | *..*) continue ;; esac
    printf '%s    - %s%s\n' "$D" "$root/$file" "$Z"
    if [[ $DRY -eq 0 ]]; then ${priv[@]+"${priv[@]}"} rm -f -- "$root/$file"; fi
    count=$((count + 1))
    dir="$(dirname "$root/$file")"
    # Tidy up empty parents, but never the prefix itself.
    while [[ "$dir" != "$root" && "$dir" != "/" ]]; do
      if [[ $DRY -eq 0 ]]; then ${priv[@]+"${priv[@]}"} rmdir -- "$dir" 2> /dev/null || break; else break; fi
      dir="$(dirname "$dir")"
    done
  done < "$record"
  if [[ $DRY -eq 0 ]]; then
    ${priv[@]+"${priv[@]}"} rm -f -- "$record"
    ${priv[@]+"${priv[@]}"} rmdir -- "$root/share/$APP-install" 2> /dev/null || true
  fi
  good "removed $count files from $root"
}

step "Removing Menagerie"
if ! remove_recorded "$PREFIX" manifest; then
  warn "No install record under $PREFIX, so install.sh did not put the app there."
  if command -v "$APP" > /dev/null 2>&1; then
    warn "It is at $(command -v "$APP"). If it came from a package (.deb, .rpm, AUR), remove it with your package manager;"
    warn "if it is under another prefix, run this again with --prefix DIR."
  fi
fi

if [[ $DO_ENGINE -eq 1 ]]; then
  step "Removing wl_shimeji"
  if ! remove_recorded "$ENGINE_PREFIX" manifest-engine && ! remove_recorded "$PREFIX" manifest-engine; then
    warn "install.sh did not build wl_shimeji here (or it was installed as a package). If it is a package, remove that"
    warn "with your package manager, for example: sudo pacman -Rns wl_shimeji-git"
  fi
fi

if [[ $PURGE -eq 1 ]]; then
  step "Deleting the app's data"
  data="${XDG_DATA_HOME:-$HOME/.local/share}/$APP"
  if [[ -d "$data" ]] && confirm "Delete $data (settings, favorites, presets, cached pictures)?"; then
    printf '%s    - %s%s\n' "$D" "$data" "$Z"
    if [[ $DRY -eq 0 ]]; then rm -rf -- "$data"; fi
    good "deleted"
  else
    info "kept $data"
  fi
fi

step "Done"
info "Your characters are still in ${XDG_DATA_HOME:-$HOME/.local/share}/wl_shimeji"
if [[ $DRY -eq 1 ]]; then info "That was a dry run: nothing was changed."; fi
