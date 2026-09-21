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
2. **Coding Style & Commit Messages**:
   * Code is written to be read. Comments explain **why** something is done (especially workarounds for `wl_shimeji` quirks), not merely *what* it does.
   * Match the formatting of the existing file. Do not run mass reformatters (`cargo fmt`) that alter whitespace across untouched code.
   * **Commit Structure (Conventional Commits)**:
     A commit consists of a **Subject** line and, for all non-trivial changes, a **Body (Description)** separated by a blank line:

     ```gitcommit
     <prefix>: <short imperative summary>

     <Detailed description explaining WHY and context>
     - Background context and motivation (what problem is solved)
     - Platform / engine implications (wl_shimeji, WebKitGTK, layer-shell)
     - Non-obvious trade-offs, edge cases, and safety guarantees
     ```

   * **Prefixes Table**:
     | Prefix | Category | Impact on Future Release | Subject Example |
     |---|---|---|---|
     | **`fix:`** | Bug fix, crash prevention, logic fix | Triggers **PATCH** (`1.0.x`) | `fix: handle missing animation frames` |
     | **`feat:`** | New user-facing feature or option | Triggers **MINOR** (`1.x.0`) | `feat: add third character catalog` |
     | **`feat!:`** / **`fix!:`** | Breaking change (incompatible data/schema) | Triggers **MAJOR** (`x.0.0`) | `feat!: overhaul preset file schema` |
     | **`docs:`** | Documentation, guides, screenshots | No release trigger | `docs: clarify niri autostart steps` |
     | **`refactor:`** | Code restructuring with no behavior change | No release trigger | `refactor: clean up wayland wire parser` |
     | **`chore:`** / **`ci:`** | Dependencies, build tools, CI pipeline | No release trigger | `chore: update tauri build dependencies` |

   * **Subject Rules**:
     - Maximum 72 characters.
     - Imperative mood, present tense ("add", "fix", "clean", not "added", "fixes").
     - Lowercase after the prefix, no trailing period.

   * **Body (Description) Rules — What to Write**:
     - **Mandatory** for all `feat`, `fix`, `feat!`, non-trivial `refactor`, and architectural changes. Only trivial 1-line typo/link fixes may omit a body.
     - **Focus on the WHY**: Explain the motivation and rationale. Why was this change needed? What went wrong previously?
     - **Explain Engine & Platform Quirks**: Mention specific `wl_shimeji`, WebKitGTK, or Wayland quirks that informed the solution (e.g. why `reload-all` is avoided, why `filter: opacity` was used over `opacity`, why batch sockets prevent lock contention).
     - **Do NOT merely rephrase the diff**: Do not write "modified line 42 in foo.rs". The diff already shows *what* changed; the description explains *why* and *what consequences* it has.
     - **Format**: Wrap lines at 72-80 characters, use bullet points where multiple aspects are touched.

   * **Example Commit with Description**:
     ```gitcommit
     fix: prevent overlay crash when summoning characters with missing frames

     wl_shimeji terminates abruptly if a character prototype references
     animation frames that do not exist in its image directory. When
     importing community archives with incomplete frame sets, this caused
     silent overlay termination.

     - Automatically detect missing animation frames in pack.rs during import
     - Synthesize fallback frames from idle sprite to satisfy engine lookup
     - Add repair_all hook before batch summoning
     ```

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

## 4. Versioning Lifecycle: Commits vs. Releases

### Rule 1: Commits Are NOT Releases
* **Never bump versions on routine commits, bug fixes, or incremental features**.
* Normal git commits pushed to `main` trigger `ci.yml` (syntax checks, tests, clippy, package validation), but **they do NOT trigger `release.yml` and do NOT build binary release packages**.
* All incremental changes must be recorded under the `## [Unreleased]` section in [CHANGELOG.md](../../CHANGELOG.md). The version number in `Cargo.toml`, `tauri.conf.json`, and `package.json` remains untouched during daily development.

### Rule 2: How the Agent Determines Which Version to Apply
When the user explicitly instructs to prepare a release (e.g., "Let's release", "Prepare next version", "Bump version"), the agent inspects the accumulated changes under `## [Unreleased]` in `CHANGELOG.md` and commits since the last tag:

1. **MAJOR (`+1.0.0` — e.g., `1.0.0` → `2.0.0`)**:
   * *When to apply*: Breaking changes to configuration or saved data (`library.json`, `prefs.json`), removing deprecated features, fundamental architecture overhaul that is incompatible with previous installs.
2. **MINOR (`0.+1.0` — e.g., `1.0.0` → `1.1.0`)**:
   * *When to apply*: Substantial new user-facing features added in a backward-compatible way (e.g., adding a third character catalog, introducing new desktop presets/controls, adding a new compositor integration).
3. **PATCH (`0.0.+1` — e.g., `1.0.0` → `1.0.1`)**:
   * *When to apply*: Bug fixes, crash prevention, documentation updates, styling adjustments, dependency updates, and minor stability improvements.

### Rule 3: Execution of a Release
Only execute a version bump when **explicitly requested by the user**:
1. **Update `CHANGELOG.md`**: Convert the bullet points under `## [Unreleased]` into a dated release header:
   ```markdown
   ## [Unreleased]

   ## [X.Y.Z] — YYYY-MM-DD
   - [bullet points of changes]
   ```
2. **Synchronize All 5 Version Locations** (strict equality required by CI):
   * `package.json` ➔ `"version": "X.Y.Z"`
   * `src-tauri/Cargo.toml` ➔ `version = "X.Y.Z"`
   * `src-tauri/tauri.conf.json` ➔ `"version": "X.Y.Z"`
   * `packaging/arch/menagerie/PKGBUILD` ➔ `pkgver=X.Y.Z`, `pkgrel=1`
   * `packaging/arch/menagerie-bin/PKGBUILD` ➔ `pkgver=X.Y.Z`, `pkgrel=1`
3. **Triggering GitHub Releases**:
   * Committing and pushing the updated files updates the repository.
   * Creating and pushing a git tag (`git tag vX.Y.Z && git push origin main --tags`) triggers `.github/workflows/release.yml`, which compiles binaries, builds `.deb`, `.rpm`, and `.AppImage`, and publishes the GitHub Release.
