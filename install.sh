#!/usr/bin/env bash
#
# Menagerie — installer.
#
#   ./install.sh                  install the app, and wl_shimeji (the engine) if it is missing
#   curl -fsSL https://raw.githubusercontent.com/kyzmapiratov/Menagerie/main/install.sh | bash
#
# Where the app comes from:
#   --binary          the prebuilt release: takes seconds, needs no compiler
#   --source          build it here: needs Rust 1.88+ and Node.js 18+ (offers to fetch Rust)
#   (default: --source inside a checkout of the repository, --binary otherwise)
#
# What to do:
#   --engine-only     install only wl_shimeji
#   --no-engine       do not touch wl_shimeji
#   --no-deps         do not install system packages
#   --deps-only       install the system packages for building, then stop
#
# Where to put it:
#   --prefix DIR      the app goes to DIR/bin, DIR/share, ...      (default: ~/.local)
#   --system          same as --prefix /usr/local (asks for sudo when copying)
#   --engine-prefix DIR   where wl_shimeji goes when it has to be built (default: /usr/local)
#   --version X.Y.Z   the release to download or build                (default: latest)
#
# Other:
#   -y, --yes         do not ask questions
#   -n, --dry-run     show what would be done and change nothing
#   -h, --help        this text
#
# Every command that changes something is printed before it runs, and everything that
# needs root goes through sudo, so you see exactly what is being asked. To remove what
# this installed, run ./uninstall.sh.

set -euo pipefail

APP="menagerie"
REPO="kyzmapiratov/Menagerie"
RELEASES="${MENAGERIE_RELEASES:-https://github.com/$REPO/releases}"
REPO_URL="${MENAGERIE_REPO_URL:-https://github.com/$REPO.git}"
ENGINE_URL="https://github.com/CluelessCatBurger/wl_shimeji.git"
AUR_URL="https://aur.archlinux.org/wl_shimeji-git.git"

# The oldest versions the build needs. Rust: what the locked dependencies ask for.
MIN_RUST="1.88.0"
MIN_NODE="18"

PREFIX="${PREFIX:-$HOME/.local}"
ENGINE_PREFIX=""
MODE="auto"
WANT="latest"
DO_DEPS=1
DO_ENGINE=1
DO_APP=1
DEPS_ONLY=0
ASSUME_YES=0
DRY=0

# The directory this script lives in, when it is a file (not when it is piped into bash).
HERE=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
TMP=""
PATH_AT_START="$PATH"
SUDO=(sudo)   # empty when already root (a container)

# ------------------------------------------------------------------- output

if [[ -t 1 ]]; then
  B=$'\033[1m'; D=$'\033[2m'; Y=$'\033[33m'; R=$'\033[31m'; G=$'\033[32m'; C=$'\033[1;34m'; Z=$'\033[0m'
else
  B=""; D=""; Y=""; R=""; G=""; C=""; Z=""
fi

step() { printf '\n%s==>%s %s%s%s\n' "$C" "$Z" "$B" "$*" "$Z"; }
info() { printf '    %s\n' "$*"; }
good() { printf '    %s✓%s %s\n' "$G" "$Z" "$*"; }
warn() { printf '%s  ! %s%s\n' "$Y" "$*" "$Z" >&2; }
die() { printf '%serror:%s %s\n' "$R" "$Z" "$*" >&2; exit 1; }

# Shows a command and runs it. With --dry-run it only shows it.
run() {
  # %q quotes what needs it, so a printed line can be pasted into a shell as it is.
  printf '%s    $' "$D"; printf ' %q' "$@"; printf '%s\n' "$Z"
  if [[ $DRY -eq 1 ]]; then return 0; fi
  "$@"
}

usage() {
  if [[ -n "$HERE" ]]; then
    awk 'NR > 2 && /^#/ { sub(/^# ?/, ""); print; next } NR > 2 { exit }' "$HERE/install.sh"
  else
    echo "Menagerie installer. Options: --binary --source --engine-only --no-engine --no-deps"
    echo "--deps-only --prefix DIR --system --engine-prefix DIR --version X.Y.Z --yes --dry-run"
  fi
  exit 0
}

cleanup() { if [[ -n "$TMP" ]]; then rm -rf "$TMP"; fi; }
trap cleanup EXIT

scratch() { if [[ -z "$TMP" ]]; then TMP="$(mktemp -d)"; fi; }

# Asks a yes/no question (default yes). Without a terminal there is nobody to ask,
# and the person did run an installer on purpose, so the answer is yes.
confirm() {
  if [[ $ASSUME_YES -eq 1 || ! -r /dev/tty ]]; then return 0; fi
  local answer=""
  printf '    %s [Y/n] ' "$1" > /dev/tty
  read -r answer < /dev/tty || answer=""
  [[ -z "$answer" || "$answer" =~ ^[Yy] ]]
}

have() { command -v "$1" > /dev/null 2>&1; }

# True when this user can create $1: the nearest folder that exists is theirs to write to.
can_write() {
  local d="$1"
  while [[ ! -e "$d" && "$d" != / ]]; do d="$(dirname "$d")"; done
  [[ -w "$d" ]]
}

# True when version $1 is at least $2 (dotted numbers).
version_ge() { [[ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" == "$2" ]]; }

# ---------------------------------------------------------------- arguments

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) MODE=binary ;;
    --source) MODE=source ;;
    --engine-only) DO_APP=0 ;;
    --no-engine) DO_ENGINE=0 ;;
    --no-deps) DO_DEPS=0 ;;
    --deps-only) DEPS_ONLY=1 ;;
    --prefix) PREFIX="${2:?--prefix needs a directory}"; shift ;;
    --prefix=*) PREFIX="${1#*=}" ;;
    --system) PREFIX=/usr/local ;;
    --engine-prefix) ENGINE_PREFIX="${2:?--engine-prefix needs a directory}"; shift ;;
    --engine-prefix=*) ENGINE_PREFIX="${1#*=}" ;;
    --version) WANT="${2:?--version needs a number}"; shift ;;
    --version=*) WANT="${1#*=}" ;;
    -y|--yes) ASSUME_YES=1 ;;
    -n|--dry-run) DRY=1 ;;
    -h|--help) usage ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

# ------------------------------------------------------------ this machine

detect() {
  [[ "$(uname -s)" == "Linux" ]] || die "Menagerie runs on Linux only (wl_shimeji, the engine, is a Wayland program)."
  # Root is refused because everything the script builds would end up owned by root and
  # the app installed into root's home. Containers (and CI) have nothing else, so they can say so.
  [[ $EUID -ne 0 || -n "${MENAGERIE_ALLOW_ROOT:-}" ]] || die "Do not run this as root: it asks for sudo only where it has to."
  if [[ $EUID -eq 0 ]]; then SUDO=(); else SUDO=(sudo); fi

  ARCH="$(uname -m)"
  case "$ARCH" in
    amd64) ARCH=x86_64 ;;
    arm64) ARCH=aarch64 ;;
  esac

  # os-release is a shell file that also sets NAME, VERSION and more; read it in a
  # subshell and take only the three fields wanted.
  local os_release="${MENAGERIE_OS_RELEASE:-/etc/os-release}" OS_ID="" OS_LIKE="" OS_PRETTY=""
  if [[ -r "$os_release" ]]; then
    # shellcheck source=/dev/null  # provided by the distribution, not by this repository
    eval "$(. "$os_release" > /dev/null 2>&1; printf 'OS_ID=%q OS_LIKE=%q OS_PRETTY=%q' "${ID:-}" "${ID_LIKE:-}" "${PRETTY_NAME:-}")"
  fi
  DISTRO="${OS_PRETTY:-${OS_ID:-unknown}}"
  FAMILY=unknown
  local word
  for word in $OS_ID $OS_LIKE; do
    case "$word" in
      arch|cachyos|endeavouros|manjaro|garuda|artix|arcolinux) FAMILY=arch ;;
      fedora|rhel|centos|rocky|almalinux|nobara|bazzite) FAMILY=fedora ;;
      debian|ubuntu|linuxmint|pop|elementary|zorin|raspbian|kali|neon) FAMILY=debian ;;
      suse|sles|opensuse*) FAMILY=suse ;;
      nixos) FAMILY=nix ;;
    esac
    if [[ $FAMILY != unknown ]]; then break; fi
  done

  step "This machine"
  info "System:        $DISTRO ($ARCH)"
  if [[ -n "${WAYLAND_DISPLAY:-}" || "${XDG_SESSION_TYPE:-}" == wayland ]]; then
    info "Session:       Wayland"
  else
    warn "This does not look like a Wayland session. wl_shimeji draws through Wayland only,"
    warn "so the characters will not appear until you log in to a Wayland session."
  fi
  case "${XDG_CURRENT_DESKTOP:-}" in
    *GNOME*) warn "GNOME (Mutter) has no wlr-layer-shell, so wl_shimeji cannot run there." ;;
    *Hyprland*) warn "Hyprland is listed as unsupported by wl_shimeji (wl_subsurface clipping)." ;;
  esac
  if [[ -e /run/ostree-booted ]]; then
    warn "This is an image-based system (read-only /usr). Installing packages here will not work;"
    warn "use a toolbox/distrobox container, or install the packages yourself and pass --no-deps."
  fi
  if [[ $FAMILY == nix ]]; then
    warn "NixOS: packages are not installed imperatively. Take wl_shimeji from its flake, and see"
    warn "docs/install.md for the app. Continuing without touching system packages or the engine."
    DO_DEPS=0
    DO_ENGINE=0
  fi

  # A checkout of the repository builds from source; anything else takes the release.
  if [[ $MODE == auto ]]; then
    if [[ -n "$HERE" && -f "$HERE/src-tauri/Cargo.toml" ]]; then MODE=source; else MODE=binary; fi
  fi
  if [[ $MODE == binary && $ARCH != x86_64 && $ARCH != aarch64 ]]; then
    warn "There is no prebuilt release for $ARCH; building from source instead."
    MODE=source
  fi
  info "Install mode:  $MODE"
  info "App goes to:   $PREFIX"
}

# ------------------------------------------------------------ system packages

# Tauri's documented prerequisites for building the app, what the app needs to run, and
# what wl_shimeji needs to build (it needs Python 3.10+ with Pillow to run, too).
declare -A PKGS=(
  [runtime_arch]="webkit2gtk-4.1 gtk3 openssl"
  [runtime_debian]="libwebkit2gtk-4.1-0 libgtk-3-0"
  [runtime_fedora]="webkit2gtk4.1 gtk3 openssl-libs"
  [runtime_suse]="libwebkit2gtk-4_1-0 libgtk-3-0"

  [build_arch]="base-devel git curl wget file webkit2gtk-4.1 openssl xdotool libappindicator-gtk3 librsvg nodejs npm rust"
  [build_debian]="build-essential git curl wget file pkg-config libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev nodejs npm"
  [build_fedora]="gcc gcc-c++ make git curl wget file pkgconf-pkg-config webkit2gtk4.1-devel openssl-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel nodejs npm rust cargo"
  # openSUSE names differ between releases, so the libraries are asked for by what they provide.
  [build_suse]="gcc gcc-c++ make git curl wget file pkg-config pkgconfig(webkit2gtk-4.1) pkgconfig(gtk+-3.0) pkgconfig(openssl) pkgconfig(librsvg-2.0) nodejs npm"

  [engine_arch]="base-devel git python python-pillow wayland wayland-protocols libarchive uthash"
  [engine_debian]="build-essential git pkg-config python3 python3-pil libwayland-dev libwayland-bin wayland-protocols libarchive-dev uthash-dev"
  [engine_fedora]="gcc make which git pkgconf-pkg-config python3 python3-pillow wayland-devel wayland-protocols-devel libarchive-devel uthash-devel"
  [engine_suse]="gcc make which git pkg-config python3 python3-Pillow wayland-devel wayland-protocols-devel libarchive-devel uthash-devel"

)

# Installs the packages listed for <what> (runtime, build or engine) on this family.
packages() {
  local what="$1"
  if [[ $DO_DEPS -eq 0 ]]; then return 0; fi
  local list="${PKGS[${what}_${FAMILY}]:-}"
  if [[ -z "$list" ]]; then
    warn "Do not know the $what packages for this system. You need a C toolchain, git, Python 3.10+ with Pillow,"
    warn "the development files of wayland, wayland-protocols, libarchive and uthash and, for the app, webkit2gtk 4.1,"
    warn "gtk3, openssl, Node.js and Rust. Install them, then run this again with --no-deps."
    return 0
  fi
  local -a pkgs=() all
  read -r -a all <<< "$list"
  local pkg
  for pkg in "${all[@]}"; do
    # What is already there is left alone: swapping a rustup or nvm setup for the
    # distribution's package is what makes package managers ask about conflicts.
    case "$pkg" in
      rust|cargo) if have cargo; then continue; fi ;;
      nodejs|npm) if have node && have npm; then continue; fi ;;
      git|curl|wget|file) if have "$pkg"; then continue; fi ;;
    esac
    pkgs+=("$pkg")
  done
  if [[ ${#pkgs[@]} -eq 0 ]]; then return 0; fi
  if [[ $EUID -ne 0 ]] && ! have sudo; then die "sudo is missing. Run the package command yourself as root, then run this again with --no-deps."; fi
  case "$FAMILY" in
    arch)
      local flags=(); if [[ $ASSUME_YES -eq 1 ]]; then flags=(--noconfirm); fi
      run ${SUDO[@]+"${SUDO[@]}"} pacman -S --needed ${flags[@]+"${flags[@]}"} "${pkgs[@]}" ;;
    debian) run ${SUDO[@]+"${SUDO[@]}"} apt-get update; run ${SUDO[@]+"${SUDO[@]}"} apt-get install -y "${pkgs[@]}" ;;
    fedora) run ${SUDO[@]+"${SUDO[@]}"} dnf install -y "${pkgs[@]}" ;;
    suse) run ${SUDO[@]+"${SUDO[@]}"} zypper --non-interactive install "${pkgs[@]}" ;;
  esac
}

# --------------------------------------------------------------- the engine

engine_present() { have shimejictl && have shimeji-overlayd; }

# Where files were put, so uninstall.sh removes exactly those and nothing else. The record
# has a folder of its own: in a user install "share/menagerie" is also where the app
# keeps its data, and the two must not mix.
write_manifest() {
  local root="$1" name="$2" body="$3" priv=() dir
  dir="$root/share/$APP-install"
  if ! can_write "$dir"; then priv=(${SUDO[@]+"${SUDO[@]}"}); fi
  run ${priv[@]+"${priv[@]}"} mkdir -p "$dir"
  if [[ $DRY -eq 0 ]]; then printf '%s\n' "$body" | ${priv[@]+"${priv[@]}"} tee "$dir/$name" > /dev/null; fi
}

record_engine() {
  local eprefix="$1" list="" f
  for f in bin/shimejictl bin/shimeji-overlayd lib/libwayland-shimeji-plugins.so \
           include/wl_shimeji/plugins.h include/wl_shimeji/master_header.h \
           share/systemd/user/wl_shimeji.socket share/systemd/user/wl_shimeji.service; do
    if [[ -e "$eprefix/$f" ]]; then list+="$f"$'\n'; fi
  done
  write_manifest "$eprefix" manifest-engine "${list%$'\n'}"
}

install_engine() {
  step "The engine (wl_shimeji)"
  if engine_present; then
    good "already installed: $(command -v shimejictl)"
    return 0
  fi
  info "wl_shimeji draws the characters; this app only drives it."

  # Arch and its derivatives have it in the AUR. A package is cleaner than copied files.
  if [[ $FAMILY == arch && $DO_DEPS -eq 1 ]]; then
    local helper flags=()
    if [[ $ASSUME_YES -eq 1 ]]; then flags=(--noconfirm); fi
    for helper in yay paru; do
      if have "$helper"; then
        run "$helper" -S --needed ${flags[@]+"${flags[@]}"} wl_shimeji-git
        return 0
      fi
    done
    info "No AUR helper found; building the AUR package by hand."
    scratch
    run git clone --depth 1 "$AUR_URL" "$TMP/aur"
    (
      if [[ $DRY -eq 0 ]]; then cd "$TMP/aur"; fi
      run makepkg -si --needed ${flags[@]+"${flags[@]}"}
    )
    return 0
  fi

  # Anywhere else: build it from source, the way its own README says.
  packages engine
  local eprefix="${ENGINE_PREFIX:-/usr/local}" priv=()
  if ! can_write "$eprefix"; then
    if [[ $EUID -eq 0 ]] || have sudo; then
      priv=(${SUDO[@]+"${SUDO[@]}"})
    else
      warn "No permission for $eprefix and no sudo: installing the engine into $PREFIX instead."
      eprefix="$PREFIX"
    fi
  fi
  scratch
  run git clone --recursive --depth 1 "$ENGINE_URL" "$TMP/wl_shimeji"
  (
    if [[ $DRY -eq 0 ]]; then cd "$TMP/wl_shimeji"; fi
    run make -j"$(nproc)"
    run ${priv[@]+"${priv[@]}"} make install PREFIX="$eprefix"
  )
  if [[ $DRY -eq 1 ]]; then return 0; fi
  record_engine "$eprefix"
  PATH="$eprefix/bin:$PATH"
  if ! engine_present; then die "wl_shimeji was built but shimejictl is not on the PATH. Look at the output of make install."; fi
  good "installed into $eprefix"
  case ":$PATH_AT_START:" in
    *":$eprefix/bin:"*) ;;
    *)
      warn "$eprefix/bin is not on your PATH. The app finds the engine anyway, but your terminal and"
      warn "your compositor's autostart may not: add it to your session's PATH." ;;
  esac
}

# ------------------------------------------------------------------ the app

ensure_rust() {
  local v=""
  if have cargo; then v="$(cargo --version | awk '{print $2}')"; fi
  if [[ -n "$v" ]] && version_ge "$v" "$MIN_RUST"; then
    good "Rust $v"
    return 0
  fi
  warn "Building needs Rust $MIN_RUST or newer (found: ${v:-none}); distribution packages are often older."
  if have rustup; then
    run rustup toolchain install stable --profile minimal
    export RUSTUP_TOOLCHAIN=stable
    return 0
  fi
  if [[ $ASSUME_YES -eq 0 && ! -r /dev/tty ]]; then
    die "Fetching Rust needs your OK and there is no terminal to ask. Run again with --yes, or install rustup first (https://rustup.rs)."
  fi
  confirm "Install Rust with rustup (https://rustup.rs) into ~/.cargo?" || die "Install Rust $MIN_RUST+ yourself (https://rustup.rs) and run this again."
  printf '%s    $ curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal%s\n' "$D" "$Z"
  if [[ $DRY -eq 1 ]]; then return 0; fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  PATH="$HOME/.cargo/bin:$PATH"
}

ensure_node() {
  local major=""
  if have node; then major="$(node --version | sed 's/^v//; s/\..*//')"; fi
  if [[ -z "$major" || "$major" -lt "$MIN_NODE" ]]; then
    die "Building needs Node.js $MIN_NODE or newer (found: ${major:-none}). Install it (https://nodejs.org or your package manager) and run this again."
  fi
  good "Node.js $(node --version)"
}

build_from_source() {
  local src="$HERE"
  if [[ -z "$src" || ! -f "$src/src-tauri/Cargo.toml" ]]; then
    scratch
    local ref=()
    if [[ $WANT != latest ]]; then ref=(--branch "v${WANT#v}"); fi
    run git clone --depth 1 ${ref[@]+"${ref[@]}"} "$REPO_URL" "$TMP/src"
    src="$TMP/src"
  fi
  step "Building Menagerie"
  packages build
  ensure_rust
  ensure_node
  (
    if [[ $DRY -eq 0 ]]; then cd "$src"; fi
    run npm install --no-audit --no-fund
    # The incremental cache costs about 400 MB and buys nothing for a one-off build.
    CARGO_INCREMENTAL=0 run npm run tauri build -- --no-bundle
  )
  BINARY="${CARGO_TARGET_DIR:-$src/src-tauri/target}/release/$APP"
  STAGER="$src/packaging/stage.sh"
  if [[ $DRY -eq 0 && ! -x "$BINARY" ]]; then die "the build finished but $BINARY is missing"; fi
}

download() {
  if have curl; then
    curl -fsSL --retry 3 --connect-timeout 15 -o "$2" "$1"
  elif have wget; then
    wget -q -O "$2" "$1"
  else
    die "need curl or wget to download the release"
  fi
}

# Downloads and checks the release tarball. Fails quietly when there is none, so the
# caller can build from source instead.
fetch_release() {
  step "Downloading Menagerie"
  local name="$APP-linux-$ARCH" base
  if [[ $WANT == latest ]]; then base="$RELEASES/latest/download"; else base="$RELEASES/download/v${WANT#v}"; fi
  scratch
  info "$base/$name.tar.gz"
  RELEASE_DIR="$TMP/release"
  if [[ $DRY -eq 1 ]]; then return 0; fi
  download "$base/$name.tar.gz" "$TMP/$name.tar.gz" || return 1
  download "$base/$name.tar.gz.sha256" "$TMP/$name.tar.gz.sha256" || return 1
  (cd "$TMP" && sha256sum -c "$name.tar.gz.sha256" > /dev/null) || die "the download does not match its checksum; not installing it"
  good "checksum matches"
  mkdir -p "$RELEASE_DIR"
  tar -xzf "$TMP/$name.tar.gz" -C "$RELEASE_DIR" --strip-components=1
}

# Copies a staged tree into the prefix, records what was copied, and checks it can run.
put_in_place() {
  local tree="$1" priv=()
  if ! can_write "$PREFIX"; then priv=(${SUDO[@]+"${SUDO[@]}"}); fi
  step "Installing into $PREFIX"
  run ${priv[@]+"${priv[@]}"} mkdir -p "$PREFIX"
  # --remove-destination: an app that is running is "Text file busy" to a plain copy over it, which is what an upgrade
  # while the old version is open did. Unlinking first is allowed; the running one keeps its inode until it is closed.
  run ${priv[@]+"${priv[@]}"} cp -a --no-preserve=ownership --remove-destination "$tree/." "$PREFIX/"
  if [[ $DRY -eq 1 ]]; then return 0; fi
  write_manifest "$PREFIX" manifest "$(cd "$tree" && find . -type f -printf '%P\n')"

  local missing
  missing="$(ldd "$PREFIX/bin/$APP" 2> /dev/null | awk '/not found/ { printf "%s ", $1 }')"
  if [[ -n "$missing" ]]; then
    warn "The app cannot start yet, these libraries are missing: $missing"
    warn "Install webkit2gtk 4.1, gtk3 and openssl (docs/install.md lists the package names)."
  fi
}

install_app() {
  if [[ $MODE == binary ]]; then
    packages runtime
    if fetch_release; then
      # The launcher entry gets the full path, so it starts from a menu whose PATH lacks ~/.local/bin.
      if [[ $DRY -eq 0 ]]; then sed -i "s|^Exec=.*|Exec=$PREFIX/bin/$APP|" "$RELEASE_DIR/share/applications/$APP.desktop"; fi
      put_in_place "$RELEASE_DIR"
      return 0
    fi
    warn "Could not download a release (none published yet, or no network)."
    have git || die "no release to download, and git is missing to build from source instead"
    warn "Building from source instead."
  fi
  build_from_source
  scratch
  if [[ $DRY -eq 0 ]]; then "$STAGER" "$BINARY" "$TMP/stage" "$PREFIX/bin/$APP"; fi
  put_in_place "$TMP/stage"
}

# --------------------------------------------------------------------- main

detect

if [[ $DEPS_ONLY -eq 1 ]]; then
  packages build
  packages engine
  step "Done"
  exit 0
fi

if [[ $DO_ENGINE -eq 1 ]]; then install_engine; fi
if [[ $DO_APP -eq 1 ]]; then install_app; fi

step "Done"
if [[ $DO_APP -eq 1 ]]; then
  good "Run it from your application menu, or with: $APP"
  case ":$PATH_AT_START:" in
    *":$PREFIX/bin:"*) ;;
    *)
      warn "$PREFIX/bin is not on your PATH, so typing '$APP' in a terminal will not find it yet (the menu entry works)."
      warn "To add it, put this in your shell profile:  export PATH=\"$PREFIX/bin:\$PATH\"" ;;
  esac
fi
if [[ $DO_ENGINE -eq 1 && $DRY -eq 0 ]] && ! engine_present; then
  warn "wl_shimeji is still not available. The app will start and tell you how to install it."
fi
if [[ $DRY -eq 1 ]]; then info "That was a dry run: nothing was changed."; fi
