<p align="center">
  <img src="docs/img/logo.svg" alt="" width="96" height="96">
</p>

<h1 align="center">Menagerie</h1>

<p align="center">
  <b>A modern Shimeji companion manager for Linux & Wayland.</b><br>
  Find, install and summon animated desktop mascots that walk around your screen,<br>
  climb your windows and throw themselves off the edges.
</p>

<p align="center">
  Built with <b>Tauri v2</b>, <b>Rust</b>, and <b>wl_shimeji</b>
</p>

<p align="center">
  <a href="https://github.com/kyzmapiratov/Menagerie/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/kyzmapiratov/Menagerie/ci.yml?branch=main&style=flat-square&label=ci"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux%20%C2%B7%20Wayland-2563eb?style=flat-square">
  <a href="https://github.com/CluelessCatBurger/wl_shimeji"><img alt="Engine" src="https://img.shields.io/badge/engine-wl__shimeji-7c3aed?style=flat-square"></a>
  <a href="LICENSE"><img alt="License: GPL-2.0" src="https://img.shields.io/badge/license-GPL--2.0-blue?style=flat-square"></a>
  <a href="https://github.com/kyzmapiratov/Menagerie/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/kyzmapiratov/Menagerie?style=flat-square&color=2563eb&label=release"></a>
</p>

https://github.com/user-attachments/assets/058f1f1b-2129-426c-a748-adcb52928997

Menagerie is a modern graphical front end for [wl_shimeji](https://github.com/CluelessCatBurger/wl_shimeji) — the Wayland
engine that draws desktop mascots. It provides everything the engine lacks on its own: a catalog of thousands of
characters, one-click installation, a visual collection browser, saved scene presets, and login autostart.

## Installation

Install Menagerie and the `wl_shimeji` engine with a single command:

```bash
curl -fsSL https://raw.githubusercontent.com/kyzmapiratov/Menagerie/main/install.sh | bash
```

> [!TIP]
> Run `curl -fsSL https://raw.githubusercontent.com/kyzmapiratov/Menagerie/main/install.sh | bash -s -- --dry-run` to preview commands before anything is executed.

Or install using your distribution's package / archive:

| System | Command / Package |
|---|---|
| **Arch Linux**, CachyOS, EndeavourOS, Manjaro… | `yay -S menagerie-bin` *(AUR coming soon)* · `./install.sh` or local `makepkg -si` |
| **Debian, Ubuntu, Mint, Pop!_OS…** | `sudo apt install ./menagerie_*.deb` from the [latest release](https://github.com/kyzmapiratov/Menagerie/releases/latest) |
| **Fedora**, openSUSE… | `sudo dnf install ./menagerie-*.rpm` (`zypper install` on openSUSE) |
| **Anything else** | The `.AppImage` from the release, or `./install.sh --source` to build it |

> [!IMPORTANT]
> Menagerie requires a Wayland compositor with `wlr-layer-shell` support. Developed and tested on **niri**, supported on **KDE Plasma**, and usable on **Hyprland** (with known clipping quirks).

> [!WARNING]
> **GNOME (Mutter)** is unsupported because Mutter does not implement `wlr-layer-shell`. The app detects GNOME on launch and displays an informative notice.

Every route, the engine for each distribution, updating and removal: **[docs/install.md](docs/install.md)**.

## Features

<table>
  <tr>
    <td width="50%" valign="top">
      <a href="https://raw.githubusercontent.com/kyzmapiratov/Menagerie/main/docs/img/catalog.png"><img src="docs/img/catalog.png" alt="Catalog"></a>
      <h3>Catalog</h3>
      Two built-in sources (<code>shimejis.xyz</code> and <code>cachomon.com</code>), live animated previews on hover, fuzzy search across everything, and batch installation.
    </td>
    <td width="50%" valign="top">
      <a href="https://raw.githubusercontent.com/kyzmapiratov/Menagerie/main/docs/img/scene.png"><img src="docs/img/scene.png" alt="Scene & Presets"></a>
      <h3>Scene & Presets</h3>
      Active mascot management powered by <code>wl_shimeji</code> via <code>wlr-layer-shell</code>. Real window physics, climbable borders, and one-click crowd presets.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <a href="https://raw.githubusercontent.com/kyzmapiratov/Menagerie/main/docs/img/collection.png"><img src="docs/img/collection.png" alt="Collection"></a>
      <h3>Collection</h3>
      Local mascot library grouped by universe with favorites, sorting, and multi-selection. Drop any Shimeji <code>.zip</code> archive on the window to install.
    </td>
    <td width="50%" valign="top">
      <a href="https://raw.githubusercontent.com/kyzmapiratov/Menagerie/main/docs/img/settings.png"><img src="docs/img/settings.png" alt="Settings & Startup"></a>
      <h3>Settings & Startup</h3>
      Automatic login autostart setup for <b>niri</b>, <b>Hyprland</b>, and <b>KDE Plasma</b>. Engine scale tuning, process monitoring, and instant crash recovery.
    </td>
  </tr>
</table>

## Compositor Support

| Compositor | Support Status | Notes |
|---|---|---|
| **niri** | **Full support (Tested)** | Developed and tested daily on niri. Clean rendering with no surface clipping, automatic config integration (`include "menagerie.kdl"`), backup and validation. |
| **KDE Plasma** | **Supported** | Launch at login via XDG autostart entry; shortcuts configurable in System Settings. Mascots can walk on windows using the engine's KWin plugin. |
| **Hyprland** | **Working with limitations (unsupported by engine)** | Automatic config generation (`source` line). `wl_shimeji` officially lists Hyprland as unsupported due to compositor-side `wl_subsurface` clipping (mascot edges get cut off by window borders/panels). |
| **GNOME** | **Unsupported (Does not work)** | Mutter does not implement `wlr-layer-shell`. The app detects GNOME on startup and displays an informative notice. |

> [!NOTE]
> **Hyprland users:** While mascots will spawn and move, `wl_shimeji` is officially unsupported on Hyprland because Hyprland clips subsurfaces differently. Mascots may appear sliced or cut off when crossing window borders or panels.

Details: [Will it work on my system?](docs/install.md#will-it-work-on-my-system).

## Quick Start

1. **Start it** from your application menu (`menagerie` in a terminal). If `wl_shimeji` is missing, a card tells you how to install it and provides a *Check again* button.
2. **Catalog** → pick a pack or a character → **Install**. Characters you already have are marked and never replaced without asking. Prefer a file? Drop a `.zip` on the window.
3. **Collection** → click the circle in a card's corner to select (`Shift` for a range, `Ctrl+A` for all) → **Summon**.
4. **Scene** → **Save as preset…** to remember the crowd; the preset brings it back in one click.
5. **Settings → Startup** → **Turn on**, so they are there when you log in.

> [!TIP]
> Press `Ctrl+K` to open the command palette anywhere, or `Ctrl+1`…`4` to switch tabs.

## Where Things Are Kept

| What | Where |
|---|---|
| Characters (owned by the engine) | `~/.local/share/wl_shimeji/shimejis/` — movable from **Settings → App** |
| The app's data: favorites, presets, caches, the overlay's log | `~/.local/share/menagerie/` |
| Downloaded `.zip` archives | your Downloads folder — moved to the Trash after a successful install (switchable) |

`XDG_DATA_HOME` is honored. There is no telemetry: the app talks to the two catalogs and to nothing else.

## Documentation

| Document | Description |
|---|---|
| [docs/install.md](docs/install.md) | Every way to install, the engine per distribution, compatibility, updating and removing |
| [docs/troubleshooting.md](docs/troubleshooting.md) | Nothing appears · the overlay crashed · blank window · settings that "do nothing" |
| [docs/architecture.md](docs/architecture.md) | How it is built, and what was learned about the engine the hard way |
| [docs/development.md](docs/development.md) | Setting up, testing, packaging and releasing |
| [AGENTS.md](AGENTS.md) | The same, condensed for AI coding agents |
| [CHANGELOG.md](CHANGELOG.md) | What changed, release by release |

## Contributing

Bug reports and patches are welcome — start with [.github/CONTRIBUTING.md](.github/CONTRIBUTING.md). If your desktop is not in the compatibility table, saying whether it worked is genuinely useful.

## License and Credits

Menagerie is [GPL-2.0 licensed](LICENSE).

- [wl_shimeji](https://github.com/CluelessCatBurger/wl_shimeji) by CluelessCatBurger — the engine, GPL-2.0. It runs as a separate program; nothing of it is linked into this app.
- Shimeji was created by Yuki Yamada of Group Finity; [Shimeji-ee](https://github.com/TigerHix/shimeji-ee) is the English branch whose default configuration files ship with this app.
- Character art belongs to whoever made it. The pictures above show characters from the two catalogs; none is stored in this repository — they are downloaded by you, when you ask for it.

Full details in [docs/third-party.md](docs/third-party.md).
