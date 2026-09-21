---
name: menagerie-workflow
description: >-
  Standardized development workflow, strict architectural invariants, WebKitGTK UI guidelines,
  and verification runbooks for Menagerie. Use this skill whenever inspecting, modifying,
  refactoring, testing, or releasing features in the Menagerie codebase.
---

# Menagerie Development & Engineering Standards

This skill defines the mandatory engineering standards, architectural invariants, UI rules, and verification pipelines for the **Menagerie** project. Any agent working on this codebase must adhere to these guidelines to ensure consistency, stability, and predictable output.

---

## 1. Architectural & Platform Invariants

### Front End (`src/`)
* **Pure ES Modules, No Bundler**: There is no Vite, Webpack, React, or build step. `src/` is plain JavaScript, CSS, and HTML served directly by Tauri v2.
* **Target Engine is WebKitGTK 4.1**: Do NOT assume Chromium/Blink behavior. WebKitGTK 4.1 has distinct rendering traits:
  * **No `transform` for centering**: Never use CSS `transform: translate(...)` on toasts, bars, or dialogs. It blurs text rendering in WebKitGTK. Center with Flexbox or Grid.
  * **Zero Layout Shift**: A control must retain its exact physical dimensions when changing state (e.g. idle → loading → active). Content that arrives asynchronously must have skeletons of matching dimensions (`fillSkeletons`).
  * **The Single Spinner Rule**: Only one waiting indicator style is permitted: the SVG arc spinner via `withBusy()` / `setBusy()` (in `src/util.js`). Never introduce CSS dots, underlines, progress rings, or dual spinners.
  * **Card Opacity**: Dim card content using CSS `filter: opacity(...)`, never bare `opacity` (which exposes hover-only controls in WebKitGTK).
* **Storage**: App preferences live in `prefs.json` and `library.json` managed atomically by Rust (`prefs.rs`, `library.rs`). **Never use `localStorage`**.

### Back End & Engine (`src-tauri/` & `wl_shimeji`)
* **Never call `shimejictl prototypes reload-all`**: It aborts the overlay process with `SIGABRT`. Prototypes are deleted directly from disk.
* **Serialized CLI Calls**: Never run `shimejictl` commands in parallel; they contend on a socket lock. Use `summon_batch` for crowds (talking directly to the overlay's domain socket).
* **Strict Timeouts**: Every `shimejictl` invocation must go through `run_program` or `run_timeout` with a hard limit. A hanging engine process must never freeze the application.
* **Destructive Actions Go to Trash**: Deleting characters or packages must use `system::trash()` (`gio trash` / XDG trash). Never execute unrecoverable `rm -rf` on user files.
* **Safe Scratch Space**: Temporary files must be created in `system::scratch_dir()`. Never create shared hardcoded folders in `/tmp`.

---

## 2. Standard Change & Review Workflow

When making modifications to the codebase:

1. **Check Disk & Target Directory**:
   * Always compile Rust using an external target dir with incremental compilation disabled to prevent filling user disk space:
     ```bash
     CARGO_TARGET_DIR=/tmp/menagerie-target CARGO_INCREMENTAL=0 cargo check
     ```
2. **Coding Style**:
   * Code is written to be read. Comments explain **why** something is done (especially workarounds for `wl_shimeji` quirks), not merely *what* it does.
   * Match the formatting of the existing file. Do not run mass reformatters (`cargo fmt`) that alter whitespace across untouched code.
3. **Compositor & Wayland Integrity**:
   * Never claim a compositor is "tested" unless verified on a live running session (niri is the primary tested reference; Hyprland and KDE are checked per documentation).
   * Respect protocol checks in `wayland.rs`. If a protocol (`wp_alpha_modifier_v1`, `wlr-layer-shell`) is missing, gracefully disable the feature and inform the user.

---

## 3. Mandatory Verification Pipeline (Pre-Commit Gate)

Before declaring any task or PR complete, execute and pass all verification steps:

```bash
# 1. Rust backend linters and unit tests (clean, 0 warnings)
cd src-tauri
CARGO_TARGET_DIR=/tmp/menagerie-target CARGO_INCREMENTAL=0 cargo clippy --all-targets -- -D warnings
CARGO_TARGET_DIR=/tmp/menagerie-target CARGO_INCREMENTAL=0 cargo test
rm -rf /tmp/menagerie-target
cd ..

# 2. Front-end syntax and import resolution
for f in src/*.js; do node --check "$f"; done

# 3. Packaging & script validation
desktop-file-validate packaging/menagerie.desktop
appstreamcli validate --no-net packaging/io.github.kyzmapiratov.Menagerie.metainfo.xml

# 4. Off-screen WebKitGTK UI test harness (if UI files were touched)
python3 tools/ui-harness/run.py
```

---

## 4. Releases and Version Bumping

When releasing a new version or introducing user-facing features:
1. **Update `CHANGELOG.md`**: Add concise, user-centric bullet points under `## [Unreleased]`.
2. **Version Synchronization**: If bumping version `vX.Y.Z`, verify strict equality across:
   * `src-tauri/Cargo.toml` (`version = "X.Y.Z"`)
   * `src-tauri/tauri.conf.json` (`"version": "X.Y.Z"`)
   * `package.json` (`"version": "X.Y.Z"`)
   * `packaging/arch/menagerie/PKGBUILD` and `menagerie-bin/PKGBUILD`
