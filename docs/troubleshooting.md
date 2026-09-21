# Troubleshooting

Start here if something does not work. If none of it helps, [open an issue](https://github.com/kyzmapiratov/Menagerie/issues/new/choose)
and say what the *Overlay* pill at the bottom of the sidebar shows and what `shimejictl summon <name>` does in a terminal.

- [Nothing appears when I summon a character](#nothing-appears-when-i-summon-a-character)
- ["The overlay crashed" / "The overlay stopped"](#the-overlay-crashed--the-overlay-stopped)
- ["Not a valid Shimeji-EE instance" when installing from a file](#not-a-valid-shimeji-ee-instance-when-installing-from-a-file)
- [Errors that mention a "packet"](#errors-that-mention-a-packet)
- ["Permission denied (os error 13)" when installing a character](#permission-denied-os-error-13-when-installing-a-character)
- [The app's window is blank, black or glitchy](#the-apps-window-is-blank-black-or-glitchy)
- [The app cannot find wl_shimeji, though it is installed](#the-app-cannot-find-wl_shimeji-though-it-is-installed)
- [Characters do not come back at login](#characters-do-not-come-back-at-login)
- [Small things](#small-things)
- [Resetting](#resetting)
- [Getting a log](#getting-a-log)

## Nothing appears when I summon a character

Work down this list; the first "no" is your problem.

1. **Is the engine installed?** `command -v shimejictl shimeji-overlayd` should print two paths. If not, the app shows a card
   with the commands on start; see [install.md](install.md#the-engine-wl_shimeji).
2. **Is this a Wayland session?** `echo $XDG_SESSION_TYPE` should say `wayland`. The engine cannot draw on X11.
3. **Can this desktop run it?** GNOME and Gamescope cannot, and Hyprland has a known clipping limit: see
   [install.md](install.md#will-it-work-on-my-system). The app says so in a card on start.
4. **Does it work without the app?** In a terminal: `shimejictl summon <name>` (use a name from the Collection). If nothing
   appears there either, the problem is between the engine and your compositor, not in this app. Run
   `shimeji-overlayd` in a terminal to read what it prints; a line about a missing protocol names what your compositor lacks.
5. **Is the overlay running?** The pill in the sidebar says *Overlay running* or *Overlay idle*. Idle is normal when nobody
   is on screen: the overlay starts by itself when you summon someone, and quits when the last one is gone.

## "The overlay crashed" / "The overlay stopped"

The app watches the overlay. If characters were on screen and the overlay disappears, a card appears in the corner:

- **The overlay stopped** (amber) — it exited by itself. That is what happens when you press the engine's stop
  key, or when the last character was dismissed.
- **The overlay crashed** (red) — it died on a signal. If your system keeps crash records (`systemd-coredump`), the app
  reads them, and says how many crashes there were in the last hour.
- **The overlay lost its connection** (red) — the desktop dropped the overlay, and the app ended what was left of it.
  See [below](#when-the-desktop-drops-the-overlay).

In both cases **Bring them back** starts the overlay and summons who was there. **Details** shows the last lines of
its log; **Copy log** puts them on the clipboard, which is what a bug report wants.
*Settings → App → Bring characters back by themselves* does it without the card, at most three times in three minutes:
a crash that repeats at once needs a person.

**How the app can tell.** It starts the overlay itself, so it waits for that process and
reads how it ended: a signal means a crash, plainly and on any system. (It also reads the
system's crash list where there is one — `systemd-coredump` is not installed everywhere, and
without it `coredumpctl` reports nothing even for a segmentation fault we watched happen.)

The known causes, all handled, and worth knowing if you write something similar:

- **A character with missing pictures.** Some packs reference animation frames that are not in the pack (the `Lestrade`
  character had no `shime1`–`shime3`). The engine does not check, and crashes when the character appears
  (*"Action iterator called while behavior is NULL"*). The app fills the gaps with the nearest picture the character
  does have, when it installs a character and again each time it starts, and names the culprit in the crash card when
  the overlay's log says who it was.
- **`shimejictl prototypes reload-all` crashes the overlay** (SIGABRT). The app never calls it; if you script the
  engine yourself, do not either.
- **Many `shimejictl summon` processes at once.** Early versions started them one after another, about a second
  each, and crashed the overlay when a dozen came in quickly (how much of that was the timing and how much the missing
  pictures above, we could not fully separate). The app now summons any number of characters over one
  connection and never runs the engine's commands in parallel: sixty characters take about two seconds.

**And one that is not fixed.** Beyond all of the above, `shimeji-overlayd` also segfaults on its
own from time to time while a crowd of characters is moving about — nothing to do with installing
or summoning them. Measured on a live system: with characters walking around, one run went down
after 145 seconds with 94 of them on screen and another after 5 seconds with 30, while other runs
of 40 characters kept going for ten minutes without trouble. It never happened under a debugger,
which points at a timing problem between the overlay's threads rather than at any one character.
So: it is a fault inside the engine, this app cannot prevent it, and what it does instead is
notice at once and offer to bring everyone back (or do it by itself — *Settings → App*).

If it keeps happening, the crash card's log and `coredumpctl list shimeji-overlayd` (where the
system has it) are what to attach to an issue.

### When the desktop drops the overlay

A compositor can disconnect a client it finds fault with. niri did, once, while a large crowd was being drawn on a machine
that was very busy (compiling, at the time): its journal said

```
Data too big for buffer (1048576 + 8 > 1048576).
error in client communication (pid …)
```

(`journalctl --user -b | grep "error in client communication"` finds it.) The overlay does not quit or reconnect when that
happens. It prints `Wayland connection closed` as fast as it can — over a hundred thousand lines a second, a whole
processor core and about 15 MB a second to the disk; two gigabytes in a hundred seconds — until it happens to fall over.
All that time the characters are gone but the process is alive, so nothing looks wrong.

The app looks at the end of the overlay's log every two seconds, whether or not its window is open. When that log is
nothing but this one line it ends the overlay, shrinks the log to the lines that came before plus one line and a count,
and the usual card (or the automatic recovery) follows. Independently of that, a log that passes 32 MB is emptied, so
that none of this can fill a disk. Only an overlay that writes to the app's own log is ever ended this way.

If it happens to you often, the compositor's journal line above is the useful part of a report. Fewer characters at once,
and not summoning a crowd while something heavy is running, are the workarounds.

## Errors that mention a "packet"

`Invalid header in packet (Expected at least 8 bytes, got 0)`, `Failed to start client` and similar lines are how the
engine's command-line tool reports "the overlay is not there any more". The app translates them to *"The overlay is not
responding. It has probably crashed."* If you see the raw text, you ran `shimejictl` yourself while the overlay was gone;
starting it again (summon anything) fixes it.

## The app's window is blank, black or glitchy

WebKitGTK, which draws the app's window, has known trouble with some graphics drivers, NVIDIA's most of all. Try, in this
order, starting the app from a terminal with:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 menagerie
WEBKIT_DISABLE_COMPOSITING_MODE=1 menagerie     # if the first was not enough
```

If one of them helps, put it in the launcher: copy `/usr/share/applications/menagerie.desktop` (or
`~/.local/share/applications/`) to `~/.local/share/applications/` and change the `Exec=` line to
`Exec=env WEBKIT_DISABLE_DMABUF_RENDERER=1 menagerie`. This does not affect the characters, which the engine
draws by itself.

## "Not a valid Shimeji-EE instance" when installing from a file

`shimejictl convert` only takes a Shimeji-EE folder — `img/` and `conf/` — and says this about anything else,
including wl_shimeji's own `.wlshm` files. The app now recognises those: a `.wlshm` file, or a zip full of them
(which is what **Export** in the Collection produces), skips the converter and is imported directly, so a collection
exported from this app can be installed back into it. If you still see the message, the archive is neither of those —
open it and check that there is an `img` folder inside.

## The app cannot find wl_shimeji, though it is installed

The app looks for `shimejictl` on its `PATH`, plus `~/.local/bin` and `/usr/local/bin` whatever the `PATH` says, so a normal
install is found even from a launcher whose `PATH` is short. If the engine is somewhere else (a `--prefix` of your own,
`/opt`), make the folder visible: start the app from a shell that has it, or add it to `~/.profile`. The card on start has
a *Check again* button, so there is no need to restart.

## Characters do not come back at login

- The *Startup* tab on niri writes `~/.config/niri/config.d/55-shimeji.kdl` and one `include` line. Check
  that the switch there is on and that the note under it says *On*.
- The *Startup* tab on Hyprland writes `~/.config/hypr/menagerie.conf` (and `menagerie-login.sh`) and one `source = …` line at the
  end of `hyprland.conf`; the copy of your config from before is `hyprland.conf.menagerie-backup`. If the note under the switch
  says *not checked*, Hyprland was not running when the file was written; it reads it at its next start or `hyprctl reload`.
  A shortcut with a key Hyprland spells differently (a media key, say) is left out and named there rather than guessed.
- On every other desktop, KDE Plasma included, the same switch writes `~/.config/autostart/menagerie-characters.desktop`, which KDE,
  GNOME, Xfce, Cinnamon, LXQt and most session managers read. A bare compositor started without a session manager
  (sway or river from a TTY, say) does not read that folder: there, copy the script the tab shows into your
  compositor's own `exec` lines.
- The commands in it call `shimeji-overlayd` and `shimejictl` by name. If you installed the engine under `~/.local/bin`,
  that folder has to be on the `PATH` of the session your compositor starts programs from. Installing to `/usr/local`
  (`./install.sh --engine-only` does) avoids the question.
- The startup delay (*Summon after*) gives the desktop time to load. If characters appear and are gone at once, raise it.

## Small things

- **"Downloads" opened my code editor.** The system's default program for folders was a code editor. The app asks your
  file manager directly (through D-Bus) and only falls back to `xdg-open` when that is not possible. If it still picks the
  wrong program: `xdg-mime default org.gnome.Nautilus.desktop inode/directory`, with your file manager's `.desktop` name.
- **"there is no trash available here".** Moving an archive to the Trash after installing needs `gio` (from GLib). Without
  it the archive is simply kept.
- **A character's picture is grey or missing.** The app fetches the picture again on its own a few times. If it stays,
  restart the app. The pictures are kept in `~/.local/share/menagerie/sprites/`.
- **"Character limit" does not stop me summoning.** It never did: wl_shimeji consults that number only when a
  character is about to copy itself. The app calls it *Breeding limit* now. There is no limit on summoning.
- **A setting seems to do nothing.** Every change is read back after it is written, and the control then shows what
  `wl_shimeji` really holds, not what was asked for. Three things behave that way on purpose:
  **Size** runs from ×0.5 to ×4 (the engine's own limits, which it states the other way round — to it, 2 means half-size)
  and each character takes the new size the next time it moves, so a crowd changes over a second or two rather than all at once; **See-through** needs the `wp_alpha_modifier_v1`
  protocol, which many compositors (niri among them) do not have, and the tab marks it as unavailable there; and
  **Window throw policy** is accepted and then ignored by the engine. Options that need a compositor plugin cannot work
  without one, and the tab says so.
- **Everything is see-through when the window is not focused.** Some compositors dim inactive windows by a rule of
  their own (niri's `opacity` window rule, for one). It is not the app.

## Moving the characters to another disk

**Settings → App → Characters → Move…**. They are copied to the folder you choose, every file is checked, and the old
path becomes a link to the new one — `wl_shimeji` has no setting for where they live, so the link is what makes it
follow. Only then does the old copy go to the trash, so nothing is lost even if the copy fails half-way. Summon someone
afterwards: an overlay that is already running keeps using what it read at startup.

To go back, use **Move…** again and pick the old place — a folder that already holds characters is used as it is, and
what is in it is kept.

## Resetting

The app's settings, favorites, presets and caches are in `~/.local/share/menagerie/` (or `$XDG_DATA_HOME/menagerie/`).
Close the app and delete `prefs.json` to reset the settings, or the whole folder to reset everything. **Your characters are
not in there**: they are in `~/.local/share/wl_shimeji/shimejis/`, and nothing in this section touches them.

## No tray icon

The tray icon needs a StatusNotifier host: KDE and GNOME (with an extension) have one, and on wlroots
compositors a bar such as Waybar or Quickshell provides it. Without one there is nowhere to draw it, and the
switch in *Settings → App* simply does nothing. Check with
`busctl --user list | grep StatusNotifierWatcher`.

The app also needs a small library of its own to draw the icon, `libayatana-appindicator` (`libayatana-appindicator3-1` on
Debian and Ubuntu, `libayatana-appindicator-gtk3` on Fedora). The `.deb` and `.rpm` bring it along; on Arch it is an optional
dependency of the package. Without it the app starts as usual, the switch stays off, and turning it on says what is missing.

## "Permission denied (os error 13)" when installing a character

Development builds before the first release kept their scratch files in `/tmp/menagerie`, a folder with one name for every user of the machine. If another
account had run the app first, that folder belonged to it and yours could not write there. Releases use a private folder of
their own (`$XDG_RUNTIME_DIR/menagerie`) and name the folder in the message. With an older build, ask the other account to remove
`/tmp/menagerie`, or remove it as root, and try again.

## Getting a log

**Settings → App → Overlay log** shows what wl_shimeji printed, with a Copy button — that is what to attach to a report.
The app's own output goes to the terminal it was started from: run `menagerie` there. The overlay's log file is
`~/.local/share/menagerie/overlay.log`.
For a crash: `coredumpctl list shimeji-overlayd` and `coredumpctl info shimeji-overlayd`.
