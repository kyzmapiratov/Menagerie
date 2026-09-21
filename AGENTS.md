# AGENTS.md

Guidance for AI coding agents working on this repository. Humans: see [README.md](README.md) and
[.github/CONTRIBUTING.md](.github/CONTRIBUTING.md); this file is the same knowledge, condensed and made checkable.

## What this is

**Menagerie** is a Linux/Wayland desktop app (Tauri v2: Rust back end, plain HTML/CSS/JS front end) that is a
front end for **wl_shimeji** — a separate program that draws "shimeji" mascots on the screen through Wayland's
layer-shell. The app installs characters (catalog scrapers, archive conversion), summons and dismisses them by driving the
engine's `shimejictl` CLI, watches whether the engine's overlay process is alive, and sets up start-at-login.

It never draws a character itself. Everything about *how the engine behaves* is recorded, with measurements, in
[docs/architecture.md](docs/architecture.md) — read its "What we learned about wl_shimeji" table before touching anything that
talks to the engine.

## Map

```
src/                       the UI: ES modules, no build step, no framework. window.__TAURI__ is the bridge.
  index.html, styles.css   pages and the whole design system (CSS variables, dark theme)
  main.js                  Catalog, Collection, Scene, presets, selection, boot
  overlay.js               watches the overlay process; the crash card; "bring them back"
  environment.js           first-run check: engine missing / compositor cannot run it
  startup.js               login setup (niri config, or an XDG autostart entry elsewhere)
  appsettings.js           App tab: paths, move characters, tray switch, log view
  cachomon.js dock.js dragdrop.js select.js anim.js palette.js util.js
src-tauri/src/             Rust: main.rs holds every Tauri command (the API the UI sees)
  shimejictl.rs            all engine access: CLI, batch-summon helper, liveness, crashes, repair, config
  wayland.rs               asks the compositor which protocols it offers (raw wire protocol, no crate)
  catalog.rs cachomon.rs downloads.rs pack.rs   sources, archives, sprite handling
  library.rs prefs.rs      favorites/presets, preference file (NOT localStorage)
  niri.rs hyprland.rs autostart.rs   login setup: niri config / Hyprland config (`source` line, hyprctl check) / XDG autostart file
  desktop.rs               which compositor this is (niri, Hyprland, KDE, sway, GNOME): variables first, desktop name second
  relocate.rs system.rs tray.rs
packaging/                 desktop entry, icon, AppStream, stage.sh, make-tarball.sh, Arch PKGBUILDs
install.sh uninstall.sh    the installer (prebuilt or source; arch/debian/fedora/suse families)
tools/ui-harness/          checks the UI in the real WebKitGTK engine, off screen: run.py, server.py, driver.py, mock.js, suites/
docs/                      install, troubleshooting, architecture, development, third-party, img/
.github/                   CI, release workflow, issue/PR templates, CONTRIBUTING, SECURITY
```

## Run, check, build

```bash
npm install && npm run tauri dev            # live: every saved file under src/ reloads the open window

cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && for f in src/*.js; do node --check "$f"; done
shellcheck install.sh uninstall.sh packaging/*.sh
python3 tools/ui-harness/run.py             # the UI in the real engine, on made-up data; ~100 s, must be 100% passing

npm run tauri build -- --no-bundle          # release binary → src-tauri/target/release/menagerie
```

All of these run in CI and must be clean. Rust needs 1.88+, Node 18+. **Disk is often tight**: a debug build needs ~4 GB — set
`CARGO_TARGET_DIR=/tmp/somewhere CARGO_INCREMENTAL=0`. Tests that need a live engine return early when it is missing; `#[ignore]`
ones change a live session, so do not run them casually.

## Rules that are not obvious from the code

**The engine (wl_shimeji)**
- Never run `shimejictl prototypes reload-all`: it aborts the overlay. There is no delete command; the app removes files itself.
- Never run `shimejictl` processes in parallel; they queue on a lock. Summon many characters with `summon_batch` (one connection).
- Every `shimejictl` call except `config` **starts the overlay** if it is down, and the overlay quits ~1.4 s after starting with no
  client. Ask the process (`overlay_pid()`), not the CLI, whether it is alive.
- Every `shimejictl` call has a time limit (`run_program` in shimejictl.rs; `run_timeout` for a longer or shorter one). All calls hold one
  lock, so a call that can wait for ever freezes every other one. Never add a bare `Command::output()` on the engine.
- The overlay segfaults on its own sometimes while crowds move. That is an engine bug the app cannot fix; it detects it (child exit
  status, plus `coredumpctl` where present) and offers recovery. Do not "fix" it by throttling.
- The overlay's log can be gigabytes (a lost desktop connection makes it print one line at 100k+ a second). Read it only from the
  end (`read_tail`, `log_tail_of`), never with `read_to_string`. `look_at_log` (a thread started in `setup`) ends such an overlay and
  caps the file; it only acts on a process whose stdout *is* that file (`writes_to`).
- Settings are clamped and sometimes ignored by the engine. Read values back (`config_set` returns what the engine holds).
  `MASCOT_SCALE` is a *divisor* (2 = half size); the UI shows a size and sends the reciprocal.

**The user's machine and data**
- Experiments must not disturb a running overlay: start your own with a **short** socket path (`-s /tmp/x.sock`, AF_UNIX limits paths to
  ~107 bytes) and its own config root (`-cd DIR`, real copies — the engine ignores symlinked prototype folders). For anything
  visible, run a nested compositor (`niri` inside the session) and point `WAYLAND_DISPLAY` at it.
- Scratch files (converted archives, exports, probes) go in `system::scratch_dir()`: a private per-user folder. Never a fixed name in the
  shared `/tmp` (`/tmp/menagerie` once belonged to another user and every install failed with "Permission denied").
- Anything that deletes goes to the Trash (`system::trash`) and is guarded to a known folder; a name is never a path.
- niri's `config.kdl` gets one `include` line, a backup and `niri validate`; nothing else. Hyprland's `hyprland.conf` gets one `source =` line
  and a backup, then `hyprctl reload` + `configerrors`; only an error naming our file rolls it back. The app is built for niri, Hyprland and KDE
  Plasma and the docs say how far each was checked: never write "tested" for a compositor you did not run it on (only niri has been). Only files this app wrote (marked) are
  ever removed from `~/.config/autostart`.
- The app never downloads from cachomon.com (its terms allow it on the site only). Do not commit character art.

**The UI**
- **Nothing moves.** A control keeps its size when its state changes; content that arrives later has a placeholder of the same size.
  Layout shift is treated as a bug. Toasts and bars are centred without `transform` (it blurs text in WebKitGTK).
- **One waiting mark: a spinner**, via `withBusy()` / `setBusy()` (util.js) or `spinner()` for a line of text. It is a real SVG arc that
  turns and grows and shortens; the label fades out under it, a card dims a little, a preset's play button turns into it. Content on its
  way gets skeletons (`fillSkeletons`), real progress a bar. Do not add another style (a line under the button, dots and a
  conic-gradient ring were each tried and rejected). One spinner per action: a load that a spinning button started puts plain text
  in the status line (`note()`), not a second spinner (`busy()`). Dim card content with `filter: opacity()`, never `opacity`, or hover-only controls appear.
- **No dead controls.** A setting that cannot work is hidden or shown unavailable with the reason. Words are plain English, short;
  an error is a sentence (`friendly()` in shimejictl.rs), never a stack trace or the engine's Python output.
- Preferences live in `prefs.json` through Rust (`store` in util.js), not `localStorage`.
- Render target is **WebKitGTK 4.1**, not Chromium. A layout that is right in Chrome can be wrong here.

**Code & Commits**
- Comments say *why*, especially where the engine surprised us. No framework or bundler on the front end. No `cargo fmt` gate —
  match the file you are editing.
- **Commit messages follow Conventional Commits**: `<prefix>: <subject>` followed by a blank line and an explanatory **body (description)** for all non-trivial changes:
  - Subject line: imperative mood, lowercase, <= 72 chars. Prefixes: `fix:` (future patch), `feat:` (future minor), `feat!:` / `fix!:` (future major), `docs:`, `refactor:`, `chore:`, `ci:`.
  - Body/description: explains **why** the change was made, rationale, and platform/engine context (`wl_shimeji` quirks, WebKitGTK constraints, Wayland protocols). Do not merely rephrase the diff.
- **Commits are NOT releases**: Never bump version numbers on routine commits or fixes. Keep accumulating changes in
  `[Unreleased]` in `CHANGELOG.md`. Versions are bumped across all 5 files (`Cargo.toml`, `tauri.conf.json`, `package.json`,
  both `PKGBUILD`s) only upon explicit instruction to release.

## Verifying a UI change without opening a window

`python3 tools/ui-harness/run.py` loads the real `src/` in **WebKitGTK 4.1** (the engine Tauri renders with) off screen, with a stand-in
for `window.__TAURI__` (`mock.js`) and invented data (`data.py`), and runs the JSON suites in `tools/ui-harness/suites/`. A suite step is
an async function body whose return value is checked against `expect` (equality, `min`, `max`, `has`, `re`, `every`); `shot` steps save
PNGs to `tools/ui-harness/out/`, which you can then open and look at. Needs `python-gobject` + the `WebKit2 4.1` typelib (and
`xvfb-run` without a display).

- **Adding a Tauri command?** Add a line for it to `mock.js`; an unmocked command shows up as a page error and fails the suite.
- **Fixed a visual bug?** Add a step that would have caught it (a measurement, not "looks right") to the nearest suite in `suites/`.
- **Want to see a state?** Write a small suite with `shot` steps, run it, look at the image. `:hover` needs `__fhInit()` and the `fh` class.
- Failures are injected with `window.__lat`, `__failNext`, `__chaos`; see the top of `mock.js`. Full description: docs/development.md.

## Pitfalls that cost time before

- `tauri dev` reloads the window on **every save**, so a screenshot can show new CSS with old JS. Finish related edits, then reload.
- `pkill -f PATTERN` kills your own shell if the pattern is in your own command line; match on `ps -eo comm` or a listening PID.
- A backgrounded `( cmd ) &` inside a task reports "completed" at once; check the real process.
- `library.json` keeps several entries per character (`Shimeji.X`, `X_Y`, `X Y`); the truth for "what is installed" is the folders.
- A character that references animation frames it does not have crashes the overlay when it appears; `pack::fill_gaps` and
  `shimejictl::repair_all` fill them in. The overlay reads a character once, so a repaired one waits for the next overlay start.

## Before you say it is done

1. Clippy with `-D warnings`, `cargo test`, `node --check` on every module: clean.
2. Changed the UI? The harness passes, and you looked at a screenshot of the states around the change (empty, loading, error, narrow window) and added a step for a bug you fixed.
3. Changed how the engine is driven? Tried it on an isolated overlay, not the user's.
4. Wrote a line in [CHANGELOG.md](CHANGELOG.md) under *Unreleased* if a person would notice.
5. Touched package names or dependencies? `docs/install.md` and `install.sh` must agree (CI installs them for real in containers).

The repository is `kyzmapiratov/Menagerie` and the application id `io.github.kyzmapiratov.Menagerie`; see
[docs/development.md](docs/development.md#releasing) for where each one is written.
