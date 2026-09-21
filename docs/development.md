# Development

## Set up

```bash
git clone https://github.com/kyzmapiratov/Menagerie.git
cd Menagerie
./install.sh --deps-only        # or install the packages listed in install.md by hand
npm install
npm run tauri dev
```

You also want `wl_shimeji` installed ([install.md](install.md#the-engine-wl_shimeji)) and a Wayland session that can run it.
Without them the app still starts and the UI can be worked on: it shows the first-run card and everything that does not
need the engine.

`npm run tauri dev` serves `src/` straight from disk: **every file you save reloads the open window**, so a screenshot of
it can show a state where the new CSS meets the old JavaScript. Finish related edits together, then reload (`Ctrl+R`).
Rust changes rebuild and restart the app.

## The tree

```
README.md, CHANGELOG.md, LICENSE   the front page, what changed, the license
AGENTS.md                          the same knowledge, condensed for coding agents
install.sh, uninstall.sh           the installer
src/                               the UI: HTML, CSS and ES modules, no build step
src-tauri/src/                     the Rust side (see architecture.md for who does what)
src-tauri/assets/                  Shimeji-ee's default actions.xml / behaviors.xml, added to packages built from sprites
src-tauri/tauri.conf.json          window, security policy, bundle settings (.deb / .rpm / AppImage)
packaging/                         launcher entry, icon, AppStream metadata, stage.sh, make-tarball.sh, Arch PKGBUILDs
tools/ui-harness/                  checks the interface in the real WebKitGTK engine, off screen (server, driver, mock, suites)
docs/                              install, troubleshooting, architecture, development, third-party, and img/
.github/                           CI, release workflow, issue and PR templates, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY
```

## Checks

```bash
cd src-tauri
cargo clippy --all-targets -- -D warnings
cargo test
cd .. && for f in src/*.js; do node --check "$f"; done
shellcheck install.sh uninstall.sh packaging/*.sh
python3 tools/ui-harness/run.py        # the interface, in the real engine (see below)
```

A debug `cargo test` needs about 4 GB in `src-tauri/target`. If your disk is tight, put it somewhere else:
`CARGO_TARGET_DIR=/tmp/shimeji-target CARGO_INCREMENTAL=0 cargo test`.

Some tests need a live `wl_shimeji`, an installed character or the `niri` binary; they check for them and return early, so
the suite is green on a bare machine. The `#[ignore]`d ones change your live session (they summon and dismiss characters):
`cargo test parallel_summons -- --ignored`.

**Trying things without touching your own overlay.** The overlay takes `-s` (its socket) and `-cd`/`-cr` (its config root),
so an experiment can run a second, isolated instance with its own folder and never disturb the one you use. Doing that is
how the crash causes in [architecture.md](architecture.md) were found.

**The UI in the real engine.** The app renders with WebKitGTK, not Chromium; a layout that is right in Chrome can be wrong here.
`tools/ui-harness/` checks the interface in the real engine without opening a window:

```bash
python3 tools/ui-harness/run.py                    # every suite, on made-up data (about 100 s)
python3 tools/ui-harness/run.py settings presets   # only these
python3 tools/ui-harness/run.py --list
```

It needs `python-gobject` and the WebKit2 4.1 typelib (Arch: `python-gobject webkit2gtk-4.1`; Debian: `python3-gi gir1.2-webkit2-4.1`),
and `xvfb-run` in front of it on a machine with no display. How it works, so you can extend it:

- `server.py` serves `src/` untouched and injects three scripts: the data (`data.py`, invented characters by default, so a run is the
  same everywhere and shows no copyrighted art), the **bridge** (`mock.js`, a stand-in for `window.__TAURI__` that answers every
  command the interface uses and has knobs for making things fail), and helpers (`hover.js`).
- `driver.py` loads the page in a `WebKit2.WebView` inside a `Gtk.OffscreenWindow` and runs a suite: a JSON list of steps. A `js` step
  is an async function body whose returned value is checked against `expect`; `wait`, `size` and `shot` steps do the obvious.
- `suites/*.json` are the checks. Each one guards something that broke once: sliders that jumped, a summon that could not be
  stopped, preset names cut to a letter, a collection that flashed empty when the engine did not answer, controls that changed size
  while working.

Things to know when writing a step: a step must not return a Promise (return plain data; the driver awaits it); `:hover` cannot be
triggered by script, so call `__fhInit()` once and add the class `fh` to an element to see it hovered; failures are made testable with
`window.__lat`, `window.__failNext`, `window.__chaos` (see the top of `mock.js`); and a bridge command the interface calls but the mock
lacks is reported as a page error, so a new command needs a line in `mock.js` — that is intended.

## Screenshots for the README

`docs/img/` holds four pictures (`collection`, `catalog`, `scene`, `settings`, 1200 px wide) of the real interface, with the defaults a
fresh install would show. They come from the `screenshots` suite, run against the harness's invented characters, so the repository carries no one else's art:

```bash
python3 tools/ui-harness/run.py screenshots    # → tools/ui-harness/out/*.png
magick tools/ui-harness/out/scene.png -crop 1200x500+0+0 +repage docs/img/scene.png   # the Scene page has empty room below its content
```

Copy the rest over as they are. `--real` (your own collection) is for looking at the app as it really is; do not commit
what it produces, because the characters in it belong to their creators.

## Packaging

| File | What it is |
|---|---|
| `menagerie.desktop` | The launcher entry. |
| `menagerie.svg` | The icon; the PNG sizes in `src-tauri/icons/` are rendered from it (`rsvg-convert -w 128 …`). |
| `io.github.kyzmapiratov.Menagerie.metainfo.xml` | AppStream metadata, for software centers. Validate with `appstreamcli validate`. |
| `stage.sh` | Lays out an installed tree (binary, entry, icons, metadata, licenses) under a prefix. **Every way of installing goes through it**: `install.sh`, the release tarball, both PKGBUILDs. |
| `make-tarball.sh` | Packs that tree into `menagerie-linux-<arch>.tar.gz` plus a checksum, which is what `install.sh --binary` downloads. |
| `arch/menagerie/` | AUR package that builds from the release tag. |
| `arch/menagerie-bin/` | AUR package that installs the prebuilt release. |

The `.deb`, `.rpm` and AppImage are made by Tauri's bundler from `src-tauri/tauri.conf.json` (`bundle`), not from anything here,
except that the AppStream file is added to the first two.

The bundler also writes the tray library into the dependencies of the `.deb` and `.rpm`, and it picks whichever it finds first on the
build machine. On Arch that is the old `libappindicator3-1`, a package Debian does not have, so a `.deb` built there cannot be
installed on Debian. The release workflow therefore sets `TAURI_LINUX_AYATANA_APPINDICATOR=true` (needs `libayatana-appindicator3-dev`
on the runner) and a step afterwards reads the dependencies back and fails the release if the old name is in them. If you build a
`.deb` by hand for someone else, do the same. The binary opens whichever of the two libraries exists at run time, and starts without
either.

Flatpak and Snap are not provided: the app has to start and talk to host programs (`shimejictl`, the overlay), which a sandbox would
have to be opened wide for.

## Releasing

For the maintainer. Nothing here is needed to use or to contribute.

### Once, when the repository is published

1. **The repository's address** is `kyzmapiratov/Menagerie` in `README.md`, `install.sh`, the docs, the issue templates, `Cargo.toml`,
   `tauri.conf.json`, the AppStream file, both PKGBUILDs and the HTTP client's User-Agent. If the repository is ever moved or renamed,
   `grep -rl kyzmapiratov/Menagerie . --exclude-dir=node_modules --exclude-dir=target --exclude-dir=.git` finds every place.
2. **The application id** is `io.github.kyzmapiratov.Menagerie`, the usual form for a project on GitHub. It is in
   `src-tauri/tauri.conf.json` (and the `files` entries there), in the AppStream file's name and `<id>`, in `packaging/stage.sh` and in
   the CI workflow. Changing it later loses nothing: the app's preferences are kept in a file of its own, and only the webview's
   throw-away storage starts over.
3. **Repository settings.** Turn on *Private vulnerability reporting* (Settings → Code security), which `.github/SECURITY.md` points to;
   add topics (`shimeji`, `wayland`, `tauri`, `desktop-pet`); set the description.
4. **Try the release pipeline** without publishing: Actions → *Release* → *Run workflow*. It builds everything for both
   architectures and attaches it to the run.
5. **AUR** (optional): make an account, add an SSH key, and publish `packaging/arch/menagerie` and
   `packaging/arch/menagerie-bin` as two AUR packages (`git clone ssh://aur@aur.archlinux.org/<name>.git`, copy the
   `PKGBUILD` in, `makepkg --printsrcinfo > .SRCINFO`, commit, push). Fill in the maintainer line first.

### Each release

1. Update the version in **all three** of `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` and `package.json`, then run
   `cargo check` (in `src-tauri`) so `Cargo.lock` follows. CI fails if they disagree.
2. Move the *Unreleased* notes in `CHANGELOG.md` under the new version and date, and add a `<release>` to
   `packaging/io.github.kyzmapiratov.Menagerie.metainfo.xml`.
3. Commit, then tag and push:

   ```bash
   git tag -a v0.6.0 -m "0.6.0" && git push origin main v0.6.0
   ```

4. The *Release* workflow builds the `.deb`, `.rpm`, AppImage and tarball for x86-64 and aarch64, adds `SHA256SUMS`, and publishes a
   GitHub release with generated notes. Edit the notes if you like.
5. AUR: bump `pkgver` in both PKGBUILDs, run `updpkgsums`, regenerate `.SRCINFO`, push.

`install.sh` downloads from `releases/latest/download/…`, so the newest release is what a one-line install gets; nothing else
has to be updated.
