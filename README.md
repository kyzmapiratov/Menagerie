<p align="center">
  <img src="docs/img/logo.svg" alt="" width="88" height="88">
</p>

<h1 align="center">Menagerie</h1>

<p align="center">
  Find, install and summon <b>Shimeji</b> — the little animated mascots that walk around your screen,<br>
  climb your windows and throw themselves off the edges — on Wayland.
</p>

<p align="center">
  <a href="https://github.com/kyzmapiratov/Menagerie/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/kyzmapiratov/Menagerie/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: GPL-2.0" src="https://img.shields.io/badge/license-GPL--2.0-blue.svg"></a>
  <img alt="Linux · Wayland" src="https://img.shields.io/badge/platform-Linux%20%C2%B7%20Wayland-informational">
  <a href="https://github.com/kyzmapiratov/Menagerie/releases/latest"><img alt="Latest Release" src="https://img.shields.io/github/v/release/kyzmapiratov/Menagerie?color=blue&label=release"></a>
</p>

<p align="center">
  <img src="docs/img/showcase-preview.png" alt="Menagerie desktop session" width="880">
</p>

Menagerie is the desktop app that [wl_shimeji](https://github.com/CluelessCatBurger/wl_shimeji) — the Wayland
engine that actually draws the characters — never had: a catalog of a few thousand characters, one-click installs, a
collection you can browse, a scene you can save and restore, and a way to bring everyone back at login.

## Install

One line. It installs the app, and `wl_shimeji` too if you do not have it:

```bash
curl -fsSL https://raw.githubusercontent.com/kyzmapiratov/Menagerie/main/install.sh | bash
```

Prefer to read a script first? It is [install.sh](install.sh), it prints every command before it runs,
and `--dry-run` shows what it would do without doing it.

Or use your distribution's own package:

| System | How |
|---|---|
| **Arch Linux**, CachyOS, EndeavourOS, Manjaro… | `yay -S menagerie-bin` (prebuilt) or `menagerie` (builds it) |
| **Debian, Ubuntu, Mint, Pop!_OS…** | `sudo apt install ./menagerie_*.deb` from the [latest release](https://github.com/kyzmapiratov/Menagerie/releases/latest) |
| **Fedora**, openSUSE… | `sudo dnf install ./menagerie-*.rpm` (`zypper install` on openSUSE) |
| **Anything else** | The `.AppImage` from the release, or `./install.sh --source` to build it |

Every route, the engine for each distribution and how to remove it again: **[docs/install.md](docs/install.md)**.

## Features

### Catalog

<p align="center">
  <img src="docs/img/catalog.png" alt="Menagerie Catalog" width="880">
</p>

Two catalog sources, no account needed:
- **shimejis.xyz**: Around sixty franchise packs with live animation previews on hover, search across everything, and batch installation.
- **cachomon.com**: Free characters organized by franchise.
- Existing characters are recognized and never overwritten without asking.

### Collection

<p align="center">
  <img src="docs/img/collection.png" alt="Menagerie Collection" width="880">
</p>

Your local mascot library:
- **Universe Grouping**: Group characters into clean franchises (Pokémon, Mario, Studio Ghibli, Anime, and more).
- **Multi-select & Batch Summon**: Select multiple cards (`Shift` for a range, `Ctrl+A` for all) to summon entire groups at once.
- **Drag & Drop**: Drop any `.zip` Shimeji archive directly on the window to extract, patch missing animation frames, and install.

### Scene & Presets

<p align="center">
  <img src="docs/img/scene.png" alt="Menagerie Scene" width="880">
</p>

Active desktop mascots and screen physics:
- **Real Window Physics**: Mascots walk along window titlebars, scale borders, and fall under gravity using Wayland `wlr-layer-shell`.
- **One-Click Presets**: Save current mascot formations under custom names and bring them back anytime with one click.
- **Screen Controls**: Summon random characters, inspect active mascots, or dismiss the crowd instantly.

### Settings & Startup

<p align="center">
  <img src="docs/img/settings.png" alt="Menagerie Settings" width="880">
</p>

- **Startup at Login**: Automatic configuration for **niri** (`include "menagerie.kdl"` with validation), **Hyprland** (`source` line), and **KDE Plasma** / XDG autostart.
- **Engine Options**: Mascot scale divisors and physics adjustments, read back live from the engine.
- **Crash Resilient**: Actively monitors the overlay process; if `wl_shimeji` stops, Menagerie alerts you and restores the crowd.
- **Tray Icon**: Control the screen without keeping a window open.

## Compositor Support

Menagerie requires a Linux Wayland session with a compositor implementing `wlr-layer-shell`:

| Compositor | Support Status | Notes |
|---|---|---|
| **niri** | **Full support** | Developed and tested daily on niri. Automatic config setup (`include "menagerie.kdl"`) with backup and validation. |
| **KDE Plasma** | **Supported** | Launch at login via XDG autostart entry; shortcuts configurable in System Settings. Mascots can walk on windows using the engine's KWin plugin. |
| **Hyprland** | **Supported (with clipping limit)** | Automatic config generation (`source` line). Engine authors note that Hyprland clips subsurfaces differently, so edges may look cut off. |
| **GNOME** | **Unsupported** | Mutter does not implement `wlr-layer-shell`. The app detects GNOME on startup and displays an informative notice. |

It does **not** work on GNOME (Mutter has no layer-shell). The app checks this on start and says so, instead of leaving you wondering why nothing appears. Details: [Will it work on my system?](docs/install.md#will-it-work-on-my-system).

## Quick start

1. **Start it** from your application menu (`menagerie` in a terminal). If `wl_shimeji` is missing, a card tells you how to install it and provides a *Check again* button.
2. **Catalog** → pick a pack or a character → **Install**. Characters you already have are marked and never replaced without asking. Prefer a file? Drop a `.zip` on the window.
3. **Collection** → click the circle in a card's corner to select (`Shift` for a range, `Ctrl+A` for all) → **Summon**.
4. **Scene** → **Save as preset…** to remember the crowd; the preset brings it back in one click.
5. **Settings → Startup** → **Turn on**, so they are there when you log in.

`Ctrl+K` opens a command palette anywhere; `Ctrl+1…4` switch tabs.

## Where things are kept

| What | Where |
|---|---|
| Characters (owned by the engine) | `~/.local/share/wl_shimeji/shimejis/` — movable from **Settings → App** |
| The app's data: favorites, presets, caches, the overlay's log | `~/.local/share/menagerie/` |
| Downloaded `.zip` archives | your Downloads folder — moved to the Trash after a successful install (switchable) |

`XDG_DATA_HOME` is honored. There is no telemetry: the app talks to the two catalogs and to nothing else.

## Documentation

| | |
|---|---|
| [docs/install.md](docs/install.md) | Every way to install, the engine per distribution, compatibility, updating and removing |
| [docs/troubleshooting.md](docs/troubleshooting.md) | Nothing appears · the overlay crashed · blank window · settings that "do nothing" |
| [docs/architecture.md](docs/architecture.md) | How it is built, and what was learned about the engine the hard way |
| [docs/development.md](docs/development.md) | Setting up, testing, packaging and releasing |
| [AGENTS.md](AGENTS.md) | The same, condensed for AI coding agents |
| [CHANGELOG.md](CHANGELOG.md) | What changed, release by release |

## Contributing

Bug reports and patches are welcome — start with [.github/CONTRIBUTING.md](.github/CONTRIBUTING.md). If your desktop is not in the compatibility table, saying whether it worked is genuinely useful.

## License and credits

Menagerie is [GPL-2.0 licensed](LICENSE).

- [wl_shimeji](https://github.com/CluelessCatBurger/wl_shimeji) by CluelessCatBurger — the engine, GPL-2.0. It runs as a separate program; nothing of it is linked into this app.
- Shimeji was created by Yuki Yamada of Group Finity; [Shimeji-ee](https://github.com/TigerHix/shimeji-ee) is the English branch whose default configuration files ship with this app.
- Character art belongs to whoever made it. The pictures above show characters from the two catalogs; none is stored in this repository — they are downloaded by you, when you ask for it.

Full details in [docs/third-party.md](docs/third-party.md).
