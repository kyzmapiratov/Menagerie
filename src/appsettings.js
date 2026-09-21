// The "App" tab: where everything is kept, and the app's own habits.
//
// (The overlay's settings are on the Overlay tab, and login on Startup.)

import { $, el, invoke, store, toast, fmtSize, clip, withBusy, plural, dlg, confirmDialog } from "./util.js";

/** "/home/me/x" reads as "~/x". */
const short = (p) => p.replace(/^\/home\/[^/]+/, "~");

function panel(title, ...rows) {
  const box = el("section", "panel");
  const head = el("div", "panel-head");
  head.appendChild(el("div", "panel-title", title));
  box.append(head, ...rows);
  return box;
}

/**
 * A switch. It remembers itself in the preference file, unless `initial`/`onChange` are
 * given — then the state lives on the Rust side (the tray icon does, because the icon
 * itself is what holds it).
 */
function switchRow({ name, desc, key, initial = null, onChange = null }) {
  const row = el("div", "opt");
  const text = el("div", "opt-text");
  text.append(el("div", "opt-name", name), el("div", "opt-desc", desc));
  const on = initial === null ? store.get(key, "0") === "1" : !!initial;
  const sw = el("button", `sw ${on ? "on" : ""}`);
  sw.type = "button";
  sw.setAttribute("role", "switch");
  sw.setAttribute("aria-checked", String(on));
  sw.setAttribute("aria-label", name);
  sw.addEventListener("click", async () => {
    const now = !sw.classList.contains("on");
    sw.classList.toggle("on", now);
    sw.setAttribute("aria-checked", String(now));
    if (onChange) {
      try {
        await onChange(now);
      } catch (e) {
        // It could not be done, so the switch goes back rather than lying.
        sw.classList.toggle("on", !now);
        sw.setAttribute("aria-checked", String(!now));
        toast(String(e), "error");
      }
      return;
    }
    store.set(key, now ? "1" : "0");
  });
  const ctl = el("div", "opt-ctl");
  ctl.appendChild(sw);
  row.append(text, ctl);
  return row;
}

async function copy(text) {
  try {
    if (clip?.writeText) await clip.writeText(text);
    else await navigator.clipboard.writeText(text);
    toast("Path copied", "ok");
  } catch {
    toast("Could not copy to the clipboard", "error");
  }
}

function pathRow({ name, path, detail, which, extra }) {
  const row = el("div", "opt");
  const text = el("div", "opt-text");
  const where = el("code", "path", short(path));
  where.title = path;
  text.append(el("div", "opt-name", name), where, el("div", "opt-desc", detail));

  const open = el("button", "ghost", "Open");
  open.title = "Show this folder in the file manager";
  open.addEventListener("click", () =>
    withBusy(open, async () => {
      try {
        await invoke("open_folder", { which });
      } catch (e) {
        toast(String(e), "error");
      }
    })
  );
  const cp = el("button", "ghost", "Copy path");
  cp.addEventListener("click", () => copy(path));
  const ctl = el("div", "opt-ctl");
  ctl.append(...(extra ? [extra] : []), open, cp);
  row.append(text, ctl);
  return row;
}

/**
 * Moving the characters somewhere else: a disk with room on it, usually.
 *
 * wl_shimeji has no setting for this, so the folder becomes a link. Everything is
 * copied and checked before anything is removed, and the old copy goes to the trash,
 * so the worst case is a folder to delete by hand — never a lost collection.
 */
function moveButton(info, onDone) {
  const btn = el("button", "ghost", "Move…");
  btn.title = "Keep the characters in another folder";
  btn.addEventListener("click", () =>
    withBusy(btn, async () => {
      const chosen = await dlg
        .open({ directory: true, multiple: false, title: "Where should the characters be kept?" })
        .catch(() => null);
      if (!chosen) return;

      let adopting;
      try {
        adopting = await invoke("check_characters_folder", { path: chosen });
      } catch (e) {
        toast(String(e), "error");
        return;
      }

      const size = fmtSize(info.characters_bytes);
      const ok = await confirmDialog(
        adopting
          ? `${short(chosen)} already holds characters.\n\nYours (${size}) will be copied in beside them, and the folder here becomes a link to it. Nothing there is overwritten.`
          : `${plural(info.characters_count, "character")} (${size}) will be copied to ${short(chosen)}, and the folder here becomes a link to it.\n\nEverything is checked before the old copy goes to the trash. Characters on screen stay where they are.`,
        "Move",
        "Cancel",
        false,
      );
      if (!ok) return;

      try {
        const moved = await invoke("move_characters_folder", { path: chosen });
        const tail = moved.old_trashed ? " The old copy is in the trash." : ` ${moved.note}`;
        toast(`${plural(moved.characters, "character")} now live in ${short(moved.to)}.${tail}`, "ok", { ms: 7000 });
        onDone();
      } catch (e) {
        toast(String(e), "error", { ms: 7000 });
      }
    }),
  );
  return btn;
}

/**
 * Choosing where downloaded archives land. A browser can be told to save anywhere, and
 * the watcher is only useful if it is looking at the right folder.
 */
function watchButton(info, onDone) {
  const btn = el("button", "ghost", "Change…");
  btn.title = "Watch a different folder for downloaded archives";
  btn.addEventListener("click", () =>
    withBusy(btn, async () => {
      const chosen = await dlg
        .open({ directory: true, multiple: false, title: "Which folder do your downloads land in?" })
        .catch(() => null);
      if (!chosen) return;
      try {
        const now = await invoke("set_downloads_dir", { path: chosen });
        toast(`Watching ${short(now)}`, "ok");
        onDone();
      } catch (e) {
        toast(String(e), "error");
      }
    }),
  );
  return btn;
}

/**
 * The overlay's own output, in the app.
 *
 * When characters misbehave or the overlay goes down, this is the evidence — and asking
 * somebody to find a log file in a hidden folder is asking them not to bother.
 */
function logPanel() {
  const box = panel("What the overlay printed");
  const pre = el("pre", "pre log-view", "");
  const status = el("div", "opt-note", "");

  const read = async (n = 200) => {
    try {
      const text = await invoke("overlay_log_full", { lines: n });
      pre.textContent = text || "Nothing yet. The overlay writes here while it runs.";
      pre.scrollTop = pre.scrollHeight;
      status.textContent = `Last ${plural(text ? text.split("\n").length : 0, "line")} · kept in the app's data folder`;
    } catch (e) {
      pre.textContent = String(e);
    }
  };

  const row = el("div", "opt");
  const text = el("div", "opt-text");
  text.append(el("div", "opt-name", "Overlay log"), el("div", "opt-desc", "What wl_shimeji printed, newest at the bottom. Worth attaching to a bug report."));
  const refresh = el("button", "ghost", "Refresh");
  refresh.addEventListener("click", () => withBusy(refresh, () => read()));
  const more = el("button", "ghost", "Show more");
  more.addEventListener("click", () => withBusy(more, () => read(2000)));
  const copy = el("button", "ghost", "Copy");
  copy.addEventListener("click", () => copy2(pre.textContent));
  const ctl = el("div", "opt-ctl");
  ctl.append(refresh, more, copy);
  row.append(text, ctl);

  box.append(row, pre, status);
  read();
  return box;
}

const copy2 = async (text) => {
  try {
    if (clip?.writeText) await clip.writeText(text);
    else await navigator.clipboard.writeText(text);
    toast("Log copied", "ok");
  } catch {
    toast("Could not copy to the clipboard", "error");
  }
};

export async function refreshAppSettings() {
  const info = await invoke("storage_info").catch(() => null);

  const where = panel("Where things are kept");
  if (info && info.characters_dir) {
    const linked = info.characters_link ? ` · a link to ${short(info.characters_link)}` : "";
    where.append(
      pathRow({
        name: "Characters",
        path: info.characters_dir,
        detail: `${plural(info.characters_count, "character")} · ${fmtSize(info.characters_bytes)}${linked}`,
        which: "characters",
        extra: moveButton(info, refreshAppSettings),
      }),
      pathRow({
        name: "App data",
        path: info.app_dir,
        detail: `${fmtSize(info.app_bytes)} · pictures, catalog lists and this app's settings`,
        which: "app",
      }),
      pathRow({
        name: "Downloads",
        path: info.downloads_dir,
        detail: "Where the app looks for .zip files you download",
        which: "downloads",
        extra: watchButton(info, refreshAppSettings),
      }),
      el(
        "div",
        "opt-note",
        "wl_shimeji always looks for the characters in the same place, so “Move…” copies them where you choose and leaves a link behind. Summon someone after moving to have the overlay pick up the new place.",
      )
    );
  } else {
    where.appendChild(el("div", "opt-note", "Could not read the folders."));
  }

  // The tray icon, if this desktop has somewhere to put one.
  const tray = panel(
    "Out of the way",
    switchRow({
      name: "Tray icon",
      desc: "An icon in the system tray (the row of small icons in your panel or bar). Click it to summon someone or clear the screen without opening this window. Needs a tray: KDE, GNOME with an extension, or a bar such as Waybar or Quickshell.",
      key: "tray",
      initial: await invoke("tray_enabled").catch(() => true),
      onChange: (on) => invoke("set_tray_enabled", { on }),
    }),
  );

  const recovery = panel(
    "If the overlay stops",
    switchRow({
      name: "Bring characters back by themselves",
      desc: "The overlay sometimes crashes. With this on, the app restarts it and summons who was on screen (at most three times in three minutes). Off: a card appears with a button to do it.",
      key: "overlay-recover",
    })
  );

  $("app-panels").replaceChildren(where, tray, recovery, logPanel());
}
