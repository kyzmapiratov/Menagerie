# Changelog

All notable changes to this project are written here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.1] — 2026-09-22

- Added high-resolution visual showcase assets, documentation hero banner, and video walkthroughs.
- Relicensed project under GNU General Public License v2 (`GPL-2.0-only`), aligning with `wl_shimeji`.
- Removed `.github/CODE_OF_CONDUCT.md`.
- Improved compositor compatibility overview in documentation.
- Fixed Python environment variable inheritance (`PYTHONHOME`, `PYTHONPATH`) when running from AppImage packages, preventing `shimejictl` startup failures (`ModuleNotFoundError: No module named 'encodings'`).
- Fixed default value detection in settings range sliders when the engine returns formatted float values (e.g. `-1.000000` for `OPACITY`), preventing erroneous `-100%` slider state.
- Ensured default `shimeji-overlayd.conf` initialization before first mascot spawn, allowing the Settings tab to open cleanly on fresh installations with standard defaults (1x size, Monitor sync, 100% opacity).
- Added default metadata handling for motion smoothing (`INTERPOLATION_FRAMERATE`) to reliably display Monitor sync (`-1`) and Off (`0`).

## [1.0.0] — first public release

### The app

- **Catalog** from two sources. *shimejis.xyz*: around sixty franchise packs and a few thousand characters, animated preview
  on hover, search across all packs, batch install, and how much of each pack you already have. *cachomon.com*: free
  characters by franchise; the app opens the page in your browser and installs the `.zip` that lands in Downloads. A
  character you already have is skipped, never replaced without asking. Both catalogs refresh the same way, keep what is on
  screen while they do, and show placeholders that fill the page while a list loads.
- **Collection**: grouped by universe, *Recently added*, favorites, sorting, multi-select (`Shift`, `Ctrl+A`), export to one
  `.zip` (only the selected characters when some are selected, with a progress row and a Cancel) — which can be installed back, one
  character or the whole collection. An install reports what really happened: the engine says "done" even when it refused a file,
  so the app reads its complaints and checks each character afterwards. Clicking an installed character in a catalog
  takes you to it. **Drop an archive on the window** to install it.
- **Scene**: who is really on screen, *Summon random*, *Dismiss all*, and presets that bring a saved crowd back. *Dismiss all*
  also stops a summon that is under way, however it is being done.
- **Settings**: the overlay's options in plain words, each read back after it is written so the control shows what
  wl_shimeji really holds. The ranges are the engine's own; a setting your compositor cannot honour (See-through needs a
  protocol many, niri included, do not have) says so; settings the engine stores and never reads are left out; and the mouse
  and stylus rows name the three click actions instead of showing raw numbers.
- **Startup** brings characters back at login on any desktop: through the compositor's own config (with key bindings, a backup
  and a check) on niri and on Hyprland, and through the standard autostart entry everywhere else, KDE Plasma included (whose
  launcher also offers *Dismiss all* and *Stop the overlay* as actions you can give a shortcut). Hyprland's side has not yet
  been run on a real Hyprland: see the README. It can add a random character instead of a
  fixed one, which is also the first tile of the *Add characters* picker.
- **Settings → App**: where everything is kept (open in the file manager), **move the characters to another disk** (copied and
  checked before the old copy goes to the Trash), a folder of your own to watch for downloads, the overlay's log with a Copy
  button, the recovery switch, and the tray icon switch.
- **A tray icon** to summon someone or clear the screen without a window, on desktops that show tray icons.
- `Ctrl+K` command palette; `Ctrl+1…4` switch tabs.

### Stability

- **Summoning many characters at once does not crash the overlay, and is fast.** A character that references animation frames
  it does not have brought the overlay down when it appeared; such characters are repaired on install and at every start. A
  crowd goes over one connection: sixty characters in about two seconds instead of about a minute. If the quick way ever
  fails, they come one at a time and the app says why.
- **The overlay is watched.** When it disappears the app tells a crash from a quiet exit — from the process's own exit status,
  which works where no crash handler is installed — names the character responsible when the log says so, and offers to bring
  everyone back, or does it by itself (at most three times in three minutes). Before a crowd large enough to make wl_shimeji
  unstable, it says so.
- **An overlay whose connection to the desktop is dropped is noticed and ended.** It used to go on for minutes, printing
  one line at a hundred thousand a second (a 2 GB log, a full processor core) while the characters were gone and nothing
  in the app looked wrong. Now a background thread sees it within two seconds, ends it, and the usual recovery follows,
  with a card that says what happened. The log is only read from its end, so a huge one no longer holds a recovery up,
  and a log that passes 32 MB is emptied.
- **An export shows its progress and can no longer hang for ever.** It is a row in the same panel installs use (which character, how
  many of how many, a percentage) with a Cancel button, and it ends on "Collection exported". Before, the Export… button spun with
  nothing to say for minutes. One character the engine had half forgotten never got an answer, and the export waited for it
  for good. Now a character that does not answer within 40 seconds is skipped (and named in the result), one that is no longer
  installed is skipped without asking, an empty file is not counted as an export, and after three silent ones in a row the export
  stops and saves what it has.
- **Every call to the engine has a time limit** (60 seconds; 40 for one character's export). All calls take one lock and the others
  queue behind it, so a single call that never got an answer froze summoning, settings and the scene check for the rest of the
  session, not only the export.
- Summon in the selection bar shows its spinner and keeps the bar until the characters are on screen, and only then clears the
  selection (it used to clear it at the click, which hid the bar and left nothing to show that anything was happening).
- **Installing no longer ends in "Permission denied (os error 13)" on a machine with more than one user.** Converted archives,
  exports and probes were kept in `/tmp/menagerie`, one folder for everybody, so the second user to run the app found the first
  one's folder and could not write to it. Scratch files now live in a private folder of the user's own
  (`$XDG_RUNTIME_DIR/menagerie`, mode 0700, never a link, never someone else's), and an error names the folder it could not use.
- **The catalog's reload button now renews the search index too**, as cachomon's renews its list. The saved index (which says
  which characters exist and how many a pack has) was never renewed by anything; one on disk was three days old and search and the
  "how much of this pack do I have" counts were quietly wrong. It also renews itself after half a day, and an offline start keeps
  using the saved copy.
- The waiting spinner cannot be resized or thinned by a page rule that styles every `svg` (in the empty scene's "Bring back" button
  it came out 30 px, thin and off centre).
- The tray icon needs a library that a bare system may lack. Without it the app starts as usual, the switch stays off and
  says what is missing (the underlying icon code panics when the library is not there, which would have ended the app at start).
  The `.deb` and `.rpm` depend on the ayatana library, which every current distribution has.
- Deleting characters from disk no longer takes the overlay down (the engine's `prototypes reload-all`, which aborts it, is
  not used).
- Lists render from a saved copy and a failed refresh keeps the previous list, so the collection and the scene never flash
  empty; pictures that fail to load are retried.

### First run and installation

- A card on start says so when `wl_shimeji` is not installed (with the commands for your distribution and a *Check again*
  button), on X11 sessions, and on compositors that cannot run the engine — judged by asking the compositor which protocols it
  offers, not by its name.
- `install.sh` installs the app and the engine on Arch, Debian/Ubuntu, Fedora and openSUSE families, from a prebuilt release or
  from source, with `--dry-run`; `uninstall.sh` removes exactly what it installed.
- Packages: `.deb`, `.rpm`, AppImage, a plain tarball, and PKGBUILDs for the AUR.
- "Open Downloads" opens your file manager, not whichever program the system has registered for folders. Archives are moved to
  the Trash after a successful install (Trash, not delete; can be switched off).

### Design

- One waiting mark everywhere, a spinner: a crisp arc that turns and, as it turns, grows and shortens. On a button it takes the
  label's place (the label fades out, nothing changes size), on a card it lies over the dimmed content, on a preset its round
  play button becomes it, before a link it waits beside the word. It shows after a beat, so a quick task shows nothing, and
  stands still as a third of a ring when the system asks for reduced motion.
- Nothing moves: controls keep their size when their state changes, every slider in Settings keeps its length whatever its
  value chip says, and preset cards give the name the whole width.
