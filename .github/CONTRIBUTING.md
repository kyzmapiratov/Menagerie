# Contributing

Thanks for looking. This is a small project, so the process is small too.

## Reporting a bug

Use the [bug report form](https://github.com/kyzmapiratov/Menagerie/issues/new?template=bug_report.yml); it asks for what
is needed: your distribution and compositor (`echo $XDG_CURRENT_DESKTOP`), how `wl_shimeji` was installed, what you did,
what you expected, what happened, and the app's log — run `menagerie` from a terminal to see it. The
[troubleshooting guide](../docs/troubleshooting.md) covers the usual causes and may save you the wait.

If characters never appear at all, check [the compatibility notes](../docs/install.md#will-it-work-on-my-system) first: GNOME and Hyprland cannot run
the engine, and that is not something this app can fix.

Reporting that it *works* on a desktop or distribution we list as untested is just as useful.

## Setting up

```bash
git clone https://github.com/kyzmapiratov/Menagerie.git
cd Menagerie
./install.sh --deps-only        # system packages (or see docs/install.md for the list)
npm install
npm run tauri dev
```

The UI has **no build step**. `src/` is plain ES modules; edit a `.js`, `.css` or `.html` file and the open window
reloads. Only the Rust side compiles, and `tauri dev` watches it for you. More in [docs/development.md](../docs/development.md),
and the shape of the code in [docs/architecture.md](../docs/architecture.md).

## Before you open a pull request

```bash
cd src-tauri
cargo clippy --all-targets -- -D warnings
cargo test
cd .. && for f in src/*.js; do node --check "$f"; done
python3 tools/ui-harness/run.py     # if you touched the interface: it checks it in the real engine, off screen
```

All of this runs in CI and must be clean. If you touched `install.sh`, `uninstall.sh` or `packaging/*.sh`, run
`shellcheck` on them too.

There is deliberately **no `cargo fmt` gate**. The Rust here is written a little wider than rustfmt's defaults, and
reformatting it would bury real changes in whitespace. Match the file you are editing.

`cargo test parallel_summons -- --ignored` and the other `#[ignore]`d tests exist but change your live session (they summon
and dismiss characters), so they are not part of the normal run.

The interface has a harness that runs it in the real WebKitGTK engine without a window (`tools/ui-harness/`, described in
[docs/development.md](../docs/development.md#checks)). If you fix a visual bug, add a step that would have caught it. If your change
is visual, a screenshot in the pull request helps more than a paragraph.

Keep pull requests to one thing, and put what changed for a user in [CHANGELOG.md](../CHANGELOG.md) under *Unreleased*.

## House style

The code is written to be read.

- **Comments explain why, not what.** If a line looks odd because of something `wl_shimeji` does, say so — most of the
  surprising code here exists because the engine surprised us first. [The table in docs/architecture.md](../docs/architecture.md#what-we-learned-about-wl_shimeji)
  is the running record of that; add to it.
- **No dead controls.** A button that cannot work in the current state is disabled or absent, with the reason visible.
  Features are not duplicated in two places under two names.
- **Say what happened, in words.** Errors shown to a person are a sentence, not a stack trace or the engine's Python
  output (`friendly()` in `shimejictl.rs`). The text is plain English, short, without jargon.
- **Nothing moves.** A control that changes state keeps its size; content that arrives later has a placeholder of the
  same size. Layout shift is treated as a bug.
- Plain HTML, CSS and JavaScript on the front end. No framework, no bundler. Keep it that way unless there is a real reason
  not to.
- Rust: clippy decides.

## Things worth knowing

- **Never run `shimejictl prototypes reload-all`.** It aborts the overlay. There is no delete command either; the app removes
  a character's files itself and does not reload.
- **Never run `shimejictl` processes in parallel.** Calls are queued (`lock()`), and summoning many characters goes over one
  connection (`summon_batch`).
- **Every `shimejictl` call except `config` starts the overlay** if it is not running, and the overlay quits by itself when
  it has nobody to show. Do not poll it with commands; ask the process (`overlay_pid()`).
- The app must never edit the user's `config.kdl` beyond one `include` line, and never without a backup and a successful
  `niri validate`.
- The app never downloads from cachomon.com. Its terms allow downloads on the site; we open the page in the browser and
  watch the Downloads folder instead.
- Opening a folder goes through the file manager's D-Bus interface (`system::open_folder`), not a blind `xdg-open`, which on
  some systems opens a code editor.
- Do not commit character art or anything from the catalogs.

## License

By contributing you agree that your work is published under the [MIT license](../LICENSE) that covers this project.
