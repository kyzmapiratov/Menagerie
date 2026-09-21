# Installing

Menagerie is two programs:

- **the app** (this repository): the catalog, the collection, the scene, the settings;
- **the engine, [wl_shimeji](https://github.com/CluelessCatBurger/wl_shimeji)**: the program that draws
  the characters on your screen. It is a separate project, and the app only drives it.

You need both. The installer below handles both; the package routes handle the app and tell you how to
get the engine.

Before anything else, check that your desktop can run the engine at all —
[Will it work on my system?](#will-it-work-on-my-system). In short: a Wayland session, and not GNOME.

- [Will it work on my system?](#will-it-work-on-my-system) — desktops, distributions, architectures
- [The installer](#the-installer) — one command, any distribution
- [Packages](#packages) — `.deb`, `.rpm`, AppImage, Arch
- [The engine](#the-engine-wl_shimeji) — by distribution
- [Building it yourself](#building-it-yourself)
- [Moving your collection to another computer](#moving-your-collection-to-another-computer)
- [Updating and removing](#updating-and-removing)

## Will it work on my system?

What the app runs on is decided by the engine, [wl_shimeji](https://github.com/CluelessCatBurger/wl_shimeji), not by the app: the app is an ordinary GTK/WebKitGTK window and starts anywhere; the characters are drawn by the engine, on a layer of the Wayland compositor that sits above your windows.

### Desktops

The engine needs the compositor to provide `xdg-shell`, `wlr-layer-shell` and the `wl_subcompositor` interface.

| Desktop / compositor | Characters appear | Notes |
|---|---|---|
| **niri** | yes | What the app is developed on and used on every day. The *Startup* tab can write niri's config for you. |
| **sway**, river, labwc, Wayfire and other wlroots-based | yes, per the engine's documentation | Not tried by us. Put the script from the *Startup* tab in your compositor's `exec` lines. |
| **KDE Plasma** (KWin) | yes, per the engine's documentation | Launch at login goes through an autostart entry; shortcuts are set in *System Settings → Shortcuts* (the *Startup* tab lists the commands, and the launcher has *Dismiss all* and *Stop the overlay* actions you can give a shortcut). The app side is tested; a real Plasma session has not been. The engine ships a KWin plugin for window interaction (walking on windows, throwing): `make build-plugins`. |
| **GNOME** (Mutter) | **no** | Mutter does not implement `wlr-layer-shell`. There is no workaround on our side. |
| **Hyprland** | **with limits (unsupported upstream)** | Mascots spawn and move, but the engine's authors explicitly list Hyprland as unsupported: Hyprland's renderer aggressively clips `wl_subsurface` elements, causing mascot sprites to appear cut off/sliced around window borders and panels. Startup config integration is supported. |
| **Gamescope** | no | No `wl_subcompositor`. |
| **COSMIC**, others | untested | The protocols are there in COSMIC as far as we know. Tell us what happens. |
| Any **X11** session | no | The engine draws through Wayland only. Log in to a Wayland session. |

The app checks this when it starts: on GNOME, on X11 and on Gamescope it says so, and on Hyprland it names the clipping limit in a card in
the corner, once, instead of leaving you wondering why nothing appears.

### Window interaction needs a plugin

Some options only work with a compositor plugin: characters interacting with your windows, being thrown, knowing the
global cursor position. The engine ships one, for KWin. On any other compositor those options do nothing, and the
*Settings* tab marks them as needing a plugin and does not let you switch them on.

### Distributions

The app needs WebKitGTK 4.1 (`webkit2gtk-4.1`), GTK 3 and OpenSSL — present on every distribution from the last few
years: Debian 12, Ubuntu 22.04, Fedora 38, current openSUSE and Arch. Older releases that only have WebKitGTK 4.0 cannot
run it.

| | Status |
|---|---|
| **Arch Linux** | The one it is developed and tested on, by hand. |
| Debian 13, Ubuntu 24.04, Fedora, openSUSE Tumbleweed | The installer's package names and the engine's build are checked by CI on each of them, in a container. Nobody has yet used the app on a full desktop of those; reports welcome. |
| Debian 12 | The app (the `.deb`) runs. The *engine* cannot be built from source there as it is: it needs `wayland-protocols` 1.32 or newer and Debian 12 has 1.31. Install a newer `wayland-protocols` first, or use Debian 13. |
| Arch derivatives (CachyOS, EndeavourOS, Manjaro…) | Same packages as Arch, so it should just work. |
| NixOS | No package yet. The engine has a flake; a `flake.nix` for the app would be a very welcome contribution. |
| Image-based (Silverblue, Bazzite, SteamOS) | Untested. See [Image-based systems](#image-based-systems-silverblue-kinoite-bazzite-steamos). |
| Flatpak / Snap | Not planned. The engine and the overlay are host programs the app must start and talk to; a sandbox would have to be opened so far that it would protect nothing. |

### Architectures

x86-64 is what is used. aarch64 packages are built by CI, from the same code, on an ARM runner; they have not been
run on real hardware by the maintainers. Other architectures build from source (`./install.sh --source`) if the
engine does.

### Not on Windows or macOS

The engine is a Wayland program, and Wayland is a Linux thing. The catalog and the archive handling could work
anywhere, but there is nothing on those systems to draw the characters, so a port would have nothing to be a
front end for.

### What has been tried, and what to tell us

If you run it on something not listed as working, the most useful report is: your distribution, compositor
(`echo $XDG_CURRENT_DESKTOP`), how you installed the engine, and whether a single character summoned from a terminal
(`shimejictl summon <name>`) appears. That separates "the engine cannot run here" from "the app has a bug".
The [bug report form](https://github.com/kyzmapiratov/Menagerie/issues/new?template=bug_report.yml) asks for exactly that.

## The installer

```bash
curl -fsSL https://raw.githubusercontent.com/kyzmapiratov/Menagerie/main/install.sh | bash
```

or, from a clone, `./install.sh`. It:

1. looks at the machine (distribution, architecture, Wayland or not, compositor) and warns about anything that
   will not work;
2. installs the packages the app needs to run, with your package manager (`sudo`, and it prints the exact command);
3. installs `wl_shimeji` if `shimejictl` is not already on your `PATH`;
4. downloads the prebuilt app from the latest release and verifies its checksum — or, inside a clone of the
   repository, builds it from source;
5. puts it under `~/.local` (no root needed), adds a launcher entry and icon, and records every file it copied.

It changes nothing without printing the command first. `--dry-run` prints everything and does nothing.

| Option | |
|---|---|
| `--binary` / `--source` | the prebuilt release, or build here (default: build inside a clone, download otherwise) |
| `--engine-only` / `--no-engine` | only `wl_shimeji`, or leave it alone |
| `--no-deps` | do not install system packages (you did it, or you are on a system where this cannot work) |
| `--deps-only` | install what is needed for building, then stop |
| `--prefix DIR` / `--system` | install under `DIR` instead of `~/.local`, or into `/usr/local` (uses `sudo` for the copy) |
| `--engine-prefix DIR` | where a source-built `wl_shimeji` goes (default `/usr/local`) |
| `--version X.Y.Z` | that release instead of the latest |
| `-y`, `--yes` / `-n`, `--dry-run` | no questions / change nothing |

Building from source needs Rust 1.88+ and Node.js 18+. Distribution packages of Rust are often older than
that; if yours is, the installer offers to fetch Rust with [rustup](https://rustup.rs) — it asks first.

Where each part goes, and why: the app goes to `~/.local` so that installing it never needs root; the engine,
when it has to be built, goes to `/usr/local` (as its own instructions say) because that folder is on
the `PATH` of every session, including the one your compositor starts programs from at login.

## Packages

Each [release](https://github.com/kyzmapiratov/Menagerie/releases/latest) has:

| File | For | Notes |
|---|---|---|
| `menagerie_*_amd64.deb`, `_arm64.deb` | Debian 12+, Ubuntu 22.04+, Mint, Pop!_OS… | uses the system's WebKitGTK |
| `menagerie-*.x86_64.rpm`, `.aarch64.rpm` | Fedora 38+, openSUSE… | uses the system's WebKitGTK |
| `menagerie_*.AppImage` | anything else | carries its own WebKitGTK; big, and see the note below |
| `menagerie-linux-x86_64.tar.gz`, `-aarch64` | what `install.sh --binary` downloads | the binary and its launcher files |
| `SHA256SUMS` | | check what you downloaded |

```bash
sudo apt install ./menagerie_*.deb        # Debian, Ubuntu and relatives
sudo dnf install ./menagerie-*.rpm        # Fedora
sudo zypper install ./menagerie-*.rpm     # openSUSE
chmod +x menagerie_*.AppImage && ./menagerie_*.AppImage
```

None of these installs `wl_shimeji`: it is not in the distributions' repositories, so there is nothing for a
package to depend on. The app tells you on its first start, and shows the commands. Or run
`./install.sh --engine-only`, or follow [the engine](#the-engine-wl_shimeji) below.

The AppImage bundles WebKitGTK, which sometimes fights the graphics drivers of a newer system (a blank or
black window, or `EGL` errors in the terminal). If it does, start it with
`WEBKIT_DISABLE_DMABUF_RENDERER=1`. Prefer the `.deb`, `.rpm` or the installer when you can.

### Arch Linux and derivatives

Two packages for the AUR, both in [`packaging/arch/`](../packaging/arch):

```bash
yay -S menagerie-bin     # the prebuilt release: seconds
yay -S menagerie         # builds it from source
```

Both depend on `wl_shimeji-git`, so the engine comes with them. `libayatana-appindicator` is optional: it is only for the tray icon. Until they are published, build the
package from a clone: `cd packaging/arch/menagerie-bin && makepkg -si`.

## The engine (wl_shimeji)

Check first: `command -v shimejictl shimeji-overlayd` prints two paths if it is installed.

**Arch and derivatives** — it is in the AUR:

```bash
yay -S wl_shimeji-git           # or paru, or: git clone https://aur.archlinux.org/wl_shimeji-git.git && cd wl_shimeji-git && makepkg -si
```

**Everywhere else** — build it. It needs a C compiler, `make`, `git`, Python 3.10+ with Pillow (the engine's own
converter uses it) and the development files of wayland, wayland-protocols, libarchive and
[uthash](https://troydhanson.github.io/uthash/):

| | Packages |
|---|---|
| Debian, Ubuntu | `sudo apt install build-essential git pkg-config python3 python3-pil libwayland-dev libwayland-bin wayland-protocols libarchive-dev uthash-dev` |
| Fedora | `sudo dnf install gcc make which git pkgconf-pkg-config python3 python3-pillow wayland-devel wayland-protocols-devel libarchive-devel uthash-devel` |
| openSUSE | `sudo zypper install gcc make which git pkg-config python3 python3-Pillow wayland-devel wayland-protocols-devel libarchive-devel uthash-devel` |

```bash
git clone --recursive https://github.com/CluelessCatBurger/wl_shimeji.git
cd wl_shimeji
make -j"$(nproc)"
sudo make install               # into /usr/local; or: make install PREFIX="$HOME/.local"
```

`./install.sh --engine-only` does exactly this. If you install it under `~/.local`, that folder's `bin` has to be on
the `PATH` of the session your compositor starts programs from, or the *Startup* tab's commands will not find it at
login (the app itself always looks there).

**NixOS** — `wl_shimeji` ships a `flake.nix`; add it as an input of your configuration.

**Compositor plugins.** Window interaction (characters walking on your windows, being thrown) needs a plugin for
your compositor. The engine ships one, for KDE's KWin: `make build-plugins && sudo make install-plugins`. The
app's *Settings* tab says which options need a plugin and does not offer to switch on what cannot work.

**Start with the session.** Optional: the engine has a systemd user socket, so the overlay can be started on
demand: `systemctl --user enable --now wl_shimeji.socket`. The app's *Startup* tab does the more usual thing —
summons your characters at login — and does not need it.

## Building it yourself

```bash
git clone https://github.com/kyzmapiratov/Menagerie.git
cd Menagerie
./install.sh --source            # or by hand, below
```

By hand, after installing the dependencies for your system:

```bash
npm install
npm run tauri build -- --no-bundle      # → src-tauri/target/release/menagerie
npm run tauri build                     # → the same plus .deb, .rpm and AppImage under src-tauri/target/release/bundle/
```

The user interface is plain HTML, CSS and JavaScript loaded from `src/` and compiled into the binary; there is no
web build step. Node is only there for Tauri's command-line tool.

Dependencies to build (these are [Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/#linux)), with
Rust 1.88+ and Node.js 18+:

| | Packages |
|---|---|
| Arch | `base-devel git curl wget file webkit2gtk-4.1 openssl xdotool libappindicator-gtk3 librsvg nodejs npm rust` |
| Debian, Ubuntu | `build-essential git curl wget file pkg-config libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev nodejs npm` |
| Fedora | `gcc gcc-c++ make git curl wget file pkgconf-pkg-config webkit2gtk4.1-devel openssl-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel nodejs npm rust cargo` |
| openSUSE | the same libraries, by what they provide: `zypper install "pkgconfig(webkit2gtk-4.1)" "pkgconfig(gtk+-3.0)" "pkgconfig(openssl)" "pkgconfig(librsvg-2.0)"`, plus `gcc gcc-c++ make git nodejs npm` |

To run it, WebKitGTK 4.1, GTK 3 and OpenSSL are enough (`webkit2gtk-4.1 gtk3 openssl` on Arch,
`libwebkit2gtk-4.1-0 libgtk-3-0` on Debian, `webkit2gtk4.1 gtk3 openssl-libs` on Fedora).

A release build takes several minutes and about 1.5 GB of disk in `src-tauri/target`. `CARGO_INCREMENTAL=0` in
front of the command saves 400 MB of that; `CARGO_TARGET_DIR=/somewhere/else` moves it all.

## Image-based systems (Silverblue, Kinoite, Bazzite, SteamOS…)

`/usr` is read-only there, so system packages cannot be installed the usual way. Install inside a
[distrobox](https://distrobox.it) or toolbox container, or use the AppImage for the app. The engine has to
reach your compositor's Wayland socket, which containers of that kind share; we have not tried it, so reports
are welcome.

## Moving your collection to another computer

1. **Collection → Export…** writes every character into one `.zip`. It takes about a second and a half per character, and the panel at
   the bottom right shows which one it is on, with a Cancel button. A character the engine does not answer for is skipped and named.
2. Copy the `.zip` over.
3. On the other computer, install Menagerie and `wl_shimeji`, then **drop the `.zip` on the window** (or Catalog → *From file…*). All of
   it is installed; what you already have there is left alone.

Inside the `.zip` are the engine's own `.wlshm` files, one per character, so without Menagerie you can unzip it and run
`shimejictl prototypes import -f FILE.wlshm` for each. They are made by `wl_shimeji`, so both computers should run versions of it that
read each other's format; if the engine changes its format in a future version, export from the newer one.

## Updating and removing

**Updating.** Packages: your package manager, as for anything else. The installer: run it again — it replaces
the files it installed and leaves your data alone. A new version of the app never touches your characters.

**Removing.**

```bash
./uninstall.sh                # the app, from ~/.local: only the files install.sh recorded
./uninstall.sh --engine       # also wl_shimeji, if install.sh built it
./uninstall.sh --purge        # also the app's own data (settings, favorites, presets), after asking
```

For packages: `sudo pacman -Rns menagerie`, `sudo apt remove menagerie`, `sudo dnf remove menagerie`.

Your characters live in `~/.local/share/wl_shimeji/` and belong to the engine; nothing here removes them.
The app's own data is in `~/.local/share/menagerie/`.
