# Architecture

How the app is put together, and what we learned about the engine while building it. Read
[development.md](development.md) first if you want to change something; read this to know why things are
the way they are.

## The shape of it

```
┌─ Menagerie ─────────────────────────────────────────────────────────────┐
│  UI: plain HTML/CSS/JS in a WebKitGTK window            src/                  │
│        │  invoke("command", args)                                             │
│  Rust: Tauri commands                                   src-tauri/src/        │
│        │                                                                      │
└────────┼──────────────────────────────────────────────────────────────────────┘
         │ runs, one at a time              reads/writes files
         ▼                                            ▼
   shimejictl (wl_shimeji's CLI, Python) ──►  shimeji-overlayd (the overlay: draws the characters)
         │                                            ▲
         └── a small Python helper that talks to the overlay directly, for batches
```

The app never draws a character. It installs them (files in the engine's folders), tells the overlay who to
show, and watches whether the overlay is still alive.

## Front end — `src/`

Plain ES modules, no framework and no bundler; the folder is compiled into the binary and served by Tauri
(`withGlobalTauri` gives the page `window.__TAURI__`).

| File | Job |
|---|---|
| `index.html`, `styles.css` | The pages and the design system (dark theme, CSS variables, one set of components). |
| `main.js` | Catalog, Collection, Scene, selection, presets, boot. |
| `cachomon.js` | The cachomon.com source: list, the browser hand-off, the Downloads watcher. |
| `startup.js` | The Startup tab: login script, key bindings, niri integration. |
| `appsettings.js` | The App tab: where things are kept, recovery switch. |
| `overlay.js` | Watches the overlay; the crash card; bringing everyone back. |
| `environment.js` | The first-run check: engine missing, or a compositor that cannot run it (judged by its protocols, not its name). |
| `dragdrop.js` | An archive dropped on the window, installed through the same flow as one picked by hand. |
| `dock.js` | The install progress panel. |
| `select.js`, `anim.js`, `palette.js` | Multi-select, animation preview, the `Ctrl+K` palette. |
| `util.js` | `invoke`, toasts, dialogs, busy states, the preference store, small helpers. |

Habits that keep it stable — each exists because something broke without it:

- **Show something real at once.** Lists render from a saved copy (`mine-cache`) or skeletons while the real answer
  loads, so a slow disk never shows an empty collection or shifts the layout when data arrives.
- **Loaders are ticketed.** `latest()` hands out a ticket per request, and only the newest one may touch the page; a slow
  answer to an old click cannot overwrite a newer one.
- **Summons are serialized and single-flight** (`inOrder`), and a failed refresh keeps the previous list instead of
  showing "nobody".
- **Big lists are chunked** and revealed with an `IntersectionObserver`; a pack of thousands of characters opens
  instantly.
- **Sprites heal.** A picture that fails to load is retried with a new URL (the failed one is cached by the browser)
  and `loading="eager"`.
- **No layout shift.** Buttons that change state keep their size; toasts and the bottom bar are centered without
  `transform`, which blurs text in WebKitGTK.
- **Preferences live in a file** (`prefs.json`, written atomically by Rust), not in `localStorage`, so they survive a
  change of the app's identifier and are visible to backups.

## Back end — `src-tauri/src/`

| File | Job |
|---|---|
| `main.rs` | The Tauri commands (the whole API the UI sees) and startup. |
| `shimejictl.rs` | Everything about the engine: running its CLI, the batch-summon helper, overlay liveness and crash records, repairing characters, config read-back, the two-step converter. |
| `catalog.rs` | The shimejis.xyz scraper and the global search index. |
| `cachomon.rs` | The cachomon.com list. Read-only, cached. It never downloads anything from the site. |
| `downloads.rs` | Finds fresh Shimeji `.zip` files in Downloads. |
| `pack.rs` | Builds a Shimeji-ee package from a sprite sheet (the engine will not take one without `actions.xml` / `behaviors.xml`), re-encodes QOI → PNG for the UI, and fills gaps in animation frames. |
| `library.rs` | Favorites, presets, sprite cache; atomic saves. |
| `prefs.rs` | The preference file. |
| `niri.rs` | Generates niri's config file and the generic login script; validates with `niri validate`; the `include` line and backup. |
| `system.rs` | Desktop helpers that belong to no feature: open a folder in the file manager (D-Bus first, never a blind `xdg-open`), move to Trash, find programs, tell the distribution family, extend `PATH`. |
| `wayland.rs` | Asks the compositor which protocols it has, over the Wayland wire protocol by hand (no library): what decides whether characters can appear at all, and whether the Opacity setting does anything. |
| `tray.rs` | The tray icon and its menu. Needs a StatusNotifier host, and quietly does nothing where there is none. |
| `autostart.rs` | Launch at login on any desktop that is not niri: one `~/.config/autostart` entry, written and removed by this app only (it carries a marker), with the script folded into a single `Exec=` line. |
| `relocate.rs` | Moving the characters to another folder: copy, verify every file, swap the path for a link, and only then let the old copy go — to the trash, never a delete. |

Two principles run through it: **the app never edits what is not its own** (niri's `config.kdl` gets one `include` line,
a backup and a validation, and nothing else; the engine's files are only added to), and **it says what happened**
instead of passing on a stack trace (`friendly()` turns the engine's Python errors into one sentence).

## The overlay's life

The overlay (`shimeji-overlayd`) is started by any `shimejictl` call, and **quits about 1.4 s after it starts if no
client is connected, and right after the last character is dismissed**. So "the overlay is gone" is normal at times and a
crash at others. The app tells them apart:

- liveness is judged from the process (`/proc`), not from a socket file, which a crash leaves behind;
- the app started the overlay, so it waits for that process: the exit status says whether a signal killed it. That works
  everywhere, unlike `coredumpctl`, which needs a crash handler the system may not have installed (and then reports
  nothing at all for a segmentation fault). Both are used, and the same crash is not counted twice;
- the UI polls every few seconds and, when a running overlay disappears while characters were on screen, asks the system's
  crash list (`coredumpctl`) whether it died on a signal; a deliberate dismissal (`quiet()`) is not news;
- the crash card offers to bring back who was on screen, which the app remembers (`last-scene`), and can do it by itself,
  at most three times in three minutes;
- an overlay whose desktop connection was dropped does not exit: it prints `Wayland connection closed` in a loop until it
  falls over, so the process looks alive. A thread of the app (not the page, which does not run while the window is hidden)
  reads the end of the log every two seconds, ends such an overlay with SIGKILL, and remembers that it did
  (`Crash.cut_off`), so the card can say why. The log is only ever read from its end (`read_tail`): it can be gigabytes.

**Character health.** A character whose animation refers to frames it does not ship crashes the overlay when it
appears. `pack::fill_gaps` fills the gaps when a character is built, and `shimejictl::repair_all` checks every installed
one at each start. A repair only adds files, but the overlay reads a character once, when it loads it; so the app tracks
which characters were repaired after the current overlay started (`stale`) and does not summon those until it restarts.

**Batch summon.** One `shimejictl` process per character costs about a second each, and every call starts a client,
connects, and makes the overlay send its full state. `summon_batch` runs one small Python helper that loads
`shimejictl` as a module, connects once, and queues all the `Spawn` packets: 12 characters in 1.2 s, 60 in 2.0 s. If the
helper cannot be used (the engine's internals changed), it falls back to one process per character.

## What we learned about wl_shimeji

Verified on a live system; useful if you are writing something similar. The engine is young and moves; if something
below has changed upstream, that is good news.

| Topic | Finding |
|---|---|
| Starting | Every `shimejictl` command except `config` starts the overlay if it is not running, and makes it dump its full state. |
| Crash: on its own | `shimeji-overlayd` segfaults now and then while characters are moving, with nothing being installed or summoned. Measured: down after 145 s with 94 characters, after 5 s with 30, and not at all in ten minutes with 40. Never under a debugger, which points at a race between its threads. Nothing the app can prevent; it notices and offers to bring everyone back. |
| An overlay nobody connected to | `shimeji-overlayd` started bare exits (status 0) as soon as it has read the characters if no client is connected; a client that connects at once (the socket is SOCK_SEQPACKET) keeps it alive, and it ends ~1.4 s after the last one leaves. With 84 characters reading takes ~1.8 s, with 7 it takes 0.18 s, so an app that looked "after a moment" saw a dead overlay. `start_overlay` connects within milliseconds and holds the connection 20 s. |
| Reinstalling a removed character | The app removes a character by deleting its folder; the running overlay keeps it in memory. `prototypes import -f` of that name then prints "Successfully imported" and writes **nothing**: no folder, and the list still shows the old entry (checked on an isolated overlay). With such an entry around, a whole import can also end with an error. After `shimejictl stop` the same import works. So the app compares `prototypes list` with the folders before importing, and if the engine remembers something that is gone it restarts the overlay and puts back who was on screen (not where they stood). |
| A call that never answers | `shimejictl prototypes export -i X` for a character the overlay still knows but whose folder is gone (a half-done install taken back) never returns: the process sits in a read on the overlay's socket, using no CPU, for ever. The app's calls share one lock, so it froze everything else too. Every call now has a time limit, and an export skips such a character. |
| Stopping | The overlay quits ~1.4 s after starting with no client, and right after the last character is dismissed. |
| Lost connection | When the compositor drops it (niri: `Data too big for buffer (1048576 + 8 > 1048576)`, which a crowd on a very busy machine provoked) it neither exits nor reconnects. It logs `Wayland connection closed` at 100–240 thousand lines a second — 2.1 GB in 99 s, a core at 100 % — and finally segfaults. The process stays "alive" throughout, and a recovery that waits for it to disappear waits minutes. |
| Crash: missing frames | A character that references animation frames it does not have crashes the overlay when it appears ("Action iterator called while behavior is NULL"). Nothing checks this before. |
| Crash: `prototypes reload-all` | Aborts the overlay (SIGABRT). It is the obvious way to pick up a removed prototype, and must not be used. |
| Removing a prototype | There is no command. The app deletes the prototype's files itself, and does not reload. |
| Parallel `shimejictl` | A dozen `summon` processes at once crashed the overlay in early tests; the app runs the CLI one call at a time (a lock) and batches summons over one connection. |
| `mascot dismiss --all` | It first waits for a mouse selection (`arguments.select or arguments.id is None`). Sending SIGINT once it prints its prompt cancels the selection and dismisses everyone in about 0.3 s. |
| Listing mascots | No command lists them, and `environment info --id` is broken. The app asks the overlay for its state through its own helper (`on_screen`), and falls back to counting what it summoned itself. |
| `config list` | With the overlay stopped it prints a different format (`KEY: value`, enums as numbers). The app understands both. |
| Mouse and stylus keys | `POINTER_*` and `ON_TOOL_*` are real and used: each says which of a character's three click actions (1 drag, 2 right-click, 4 middle-click — `POINTER_PRIMARY/SECONDARY/THIRD_BUTTON`) that button or stylus tool performs. |
| Settings that do nothing | `ie_interactions`, `cursor_data`, `tablets_enabled` and `ie_throw_policy` appear in `config list`, are stored, and are read nowhere in the engine's source. `mascot_limit` is read in exactly one place — `actions/breed.c` — so it limits breeding, not what a person can summon (839 by hand is 839 on screen). The app marks all of these accordingly. |
| `config set` | Accepted and stored for any key, including ones with no effect: `WINDOW_THROW_POLICY` is ignored, and options of the plugin group do nothing without the plugin. Each call takes ~0.55 s. The app reads every value back and shows what the engine really holds. |
| Value ranges | The engine clamps silently: `MASCOT_SCALE` to 0.25–2, `OPACITY` to 0–1 (`-1` means "default" for both). A slider that offers more only produces values it throws away, so the app's sliders are its ranges. |
| `MASCOT_SCALE` is a divisor | A character is drawn at `sprite / scale` (`wp_viewport_set_destination` in `environment.c`), so the engine's **2 is the smallest** size and 0.25 the biggest — the opposite of what the name suggests. The app's slider is a size, 0.5x to 4x, and sends the reciprocal. |
| Scale and opacity, live | Both are read where a character is drawn, so a change reaches each one on its next frame — a crowd changes over a second or two, not at once. |
| Opacity needs a protocol | It is applied through `wp_alpha_modifier_v1`, and where the compositor does not offer it (niri does not) the setting does nothing at all. The app asks the compositor and marks it unavailable. |
| `convert` vs `import` | `convert` takes a Shimeji-EE folder (`img/` + `conf/`) only; it refuses wl_shimeji's own `.wlshm` files with "not a valid Shimeji-EE instance". Those go to `prototypes import` instead — which is what makes this app's own export installable again. |
| Cancelling a selection | The Python client sends opcode `0x3C` for it; the overlay's table has the handler at `0x3D`, so it warns "Unhandled opcode 3c for object type 5" and ignores it. Harmless: what follows is a normal disconnect. |
| Prototype folders | `Shimeji.{name}`, for example `Shimeji.Mosscreep (Orange)` or `Shimeji..Hornet_Needle`, in `shimejis/` (older versions used `prototypes/`, so both are searched). Frames are `assets/*.qoi`, re-encoded to PNG for the UI. |
| `convert` | Only takes `-O` and `-f`. There is no non-interactive mode, so installing a local archive is a two-step flow. |
| Window interaction | Needs a compositor plugin (`.so`); upstream ships one, for KDE KWin. |

## Data

| What | Where |
|---|---|
| Characters | `~/.local/share/wl_shimeji/shimejis/` (the engine's; `XDG_DATA_HOME` honored) |
| App data | `~/.local/share/menagerie/`: `library.json` (favorites, presets), `prefs.json`, `catalog-index.json`, `cachomon-index.json`, `sprites/`, `conf/`, `overlay.log` |
| The webview's own storage | `~/.local/share/<identifier>/` (Tauri's), nothing important |

## Security notes

- The web view has a strict Content Security Policy: it can load images from the app itself, the two catalogs and
  `data:`, and nothing else.
- The asset protocol (how the UI shows a sprite from disk) is scoped to `~/.local/share/menagerie/sprites/*` only.
- Commands that delete refuse anything outside the app's own folders and the engine's `shimejis/`; "move the archive to the
  Trash" refuses anything that is not a `.zip` in Downloads, and it is the Trash, not a delete.
- The app makes network requests only to shimejis.xyz and cachomon.com. It sends no identifiers and no telemetry.
