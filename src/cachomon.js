// cachomon.com source: browse the author's catalog and pick up downloaded archives.
//
// The app never downloads anything from this site: its terms allow downloads
// only on cachomon.com itself. A card therefore opens the character page in the
// browser, and the app watches the Downloads folder: a fresh Shimeji .zip is
// found and (optionally) installed automatically. Archives whose characters are
// already installed are not listed.
//
// The list is deliberately simple: only free, downloadable characters, one
// search box, grouped by franchise, newest first.

import { $, el, invoke, note, busy, toast, openExternal, store, norm, plural, debounce, sprite, withBusy, setBusy, skeletons, fillSkeletons } from "./util.js";
import { attachHover, localFrames } from "./anim.js";

const OTHER = "Other characters";

let entries = []; // free characters only
let loaded = false;
let newIds = new Set(); // added on the site since your last visit (never on the first visit)
let hooks = { installArchive: null, autoInstall: null, canAutoInstall: () => true, mine: () => [], showInCollection: () => {} };
const collapsed = new Set();

// ------------------------------------------------------------------ catalog

function installedKeys() {
  return new Set(hooks.mine().map((m) => norm(m.name)));
}

function visible() {
  const q = $("cacho-search").value.trim().toLowerCase();
  const list = q
    ? entries.filter((e) => [e.name, e.franchise, e.artist].some((s) => (s || "").toLowerCase().includes(q)))
    : entries;
  return [...list].sort((a, b) => b.id - a.id);
}

/** Groups by franchise, newest group first. One-character franchises share "Other characters". */
function groupsOf(list) {
  const map = new Map();
  for (const e of list) {
    const key = e.franchise || OTHER;
    if (!map.has(key)) map.set(key, []);
    map.get(key).push(e);
  }

  const singles = [];
  const groups = [];
  for (const [title, items] of map) {
    if (items.length < 2 || title === OTHER) singles.push(...items);
    else groups.push({ title, items });
  }
  groups.sort((a, b) => Math.max(...b.items.map((e) => e.id)) - Math.max(...a.items.map((e) => e.id)));

  if (singles.length) groups.push({ title: OTHER, items: singles.sort((a, b) => b.id - a.id), muted: true });
  return groups;
}

// Cards are built once and reused, so redrawing the list (a search, a change in
// your collection) does not fetch every thumbnail again.
const cards = new Map(); // "id|installed|new" -> node

/**
 * One card, built like a Collection card: the artist's name under the character's,
 * and when you point at it a button in that place. What you already have is dimmed a
 * little and marked with a small tick and the word "Installed"; nothing about it
 * shouts, and there is no wall of blue buttons.
 */
function card(e, have) {
  const key = norm(e.name);
  const isMine = have.has(key);
  const isNew = !isMine && newIds.has(e.id);
  const cacheKey = `${e.id}|${isMine ? 1 : 0}|${isNew ? 1 : 0}`;
  const cached = cards.get(cacheKey);
  if (cached) return cached;

  const c = el("div", `char cacho-card ${isMine ? "have" : ""}`);
  const mineName = isMine ? hooks.mine().find((m) => norm(m.name) === key)?.name : null;
  // What you have opens in your Collection (where it lights up, as after an install);
  // the character's page on the site stays one button away. What you do not have
  // opens the page, where the download is.
  c.title = mineName ? "In your collection: click to show it there" : "Open this character on cachomon.com";
  c.addEventListener("click", () => (mineName ? hooks.showInCollection([mineName]) : openExternal(e.url)));

  if (isMine) c.appendChild(el("span", "cacho-check", "✓")).title = "In your collection";
  else if (isNew) c.appendChild(el("span", "cacho-new", "New"));

  // The site only gives a still thumbnail, but once a character is installed we
  // have its real frames, so it can animate on hover like everywhere else.
  const stage = el("div", "stage");
  const img = sprite(e.thumb, e.name);
  stage.appendChild(img);
  c.appendChild(stage);
  const installed = isMine && hooks.mine().find((m) => norm(m.name) === key);
  if (installed) attachHover(c, img, () => localFrames(installed.name));

  c.appendChild(el("div", "char-name", e.name)).title = e.name;

  const foot = el("div", "char-foot");
  foot.appendChild(isMine ? el("div", "char-sub state", "✓ Installed") : el("div", "char-sub", e.artist ? `by ${e.artist}` : ""));
  const actions = el("div", "char-actions");
  const btn = el("button", isMine ? "ghost" : "btn", isMine ? "Open page ↗" : "Download ↗");
  btn.title = "Opens the page in your browser. Save the .zip there and the app picks it up.";
  btn.addEventListener("click", (ev) => {
    ev.stopPropagation(); // for a character you have, the card itself goes to the Collection
    openExternal(e.url);
  });
  actions.appendChild(btn);
  foot.appendChild(actions);
  c.appendChild(foot);

  cards.set(cacheKey, c);
  return c;
}

function renderGrid() {
  const host = $("cacho-grid");
  const list = visible();
  const have = installedKeys();

  note($("cacho-note"), plural(list.length, "character"));

  if (!list.length) {
    host.replaceChildren(el("div", "empty-state", "No characters match your search"));
    return;
  }

  const out = document.createDocumentFragment();
  for (const g of groupsOf(list)) {
    const sec = el("section", `group ${g.muted ? "muted-group" : ""}`);
    const isCollapsed = collapsed.has(g.title);

    const head = el("header", "group-head");
    const toggle = el("button", "group-toggle");
    toggle.title = isCollapsed ? "Expand this group" : "Collapse this group";
    toggle.appendChild(el("span", "chev", isCollapsed ? "▸" : "▾"));
    toggle.appendChild(el("span", "group-title", g.title));
    toggle.appendChild(el("span", "count", String(g.items.length)));
    const mine = g.items.filter((e) => have.has(norm(e.name))).length;
    if (mine) toggle.appendChild(el("span", "count ok", `${mine} installed`));
    toggle.addEventListener("click", () => {
      isCollapsed ? collapsed.delete(g.title) : collapsed.add(g.title);
      renderGrid();
    });
    head.appendChild(toggle);
    sec.appendChild(head);

    if (!isCollapsed) {
      const grid = el("div", "cacho-grid-inner");
      g.items.forEach((e) => grid.appendChild(card(e, have)));
      sec.appendChild(grid);
    }
    out.appendChild(sec);
  }
  host.replaceChildren(out);
}

// ------------------------------------------------- downloads: fully automatic

const autoOn = () => store.get("cacho-auto", "1") === "1";

let files = []; // Shimeji-looking .zip files in the Downloads folder
let baseline = null; // what was already there at launch: never installed automatically
const autoTried = new Set();
let autoBusy = false;
const installing = new Set(); // archives being read right now (their button spins)
let stripSig = ""; // what the strip was last drawn from

function ago(secs) {
  if (secs < 90) return "just now";
  if (secs < 3600) return `${Math.round(secs / 60)} min ago`;
  if (secs < 86400) return `${Math.round(secs / 3600)} h ago`;
  return `${Math.round(secs / 86400)} d ago`;
}

/** Archives waiting to be installed. Ones that are done are not listed at all: they are not news. */
function renderDownloads() {
  const box = $("cacho-downloads");
  const rows = files.filter((f) => !f.installed);

  // Nothing to install: no strip taking up space.
  box.classList.toggle("hidden", !rows.length);
  if (!rows.length) {
    stripSig = "";
    return box.replaceChildren();
  }

  // The folder is checked every few seconds; the strip is only redrawn when what it
  // says has changed, so a button that is working keeps its bar and nothing flickers.
  const sig = JSON.stringify([rows.map((f) => [f.path, ago(f.age_secs), f.size_mb]), [...installing]]);
  if (sig === stripSig) return;
  stripSig = sig;

  const out = document.createDocumentFragment();
  out.appendChild(el("div", "strip-title", rows.length === 1 ? "Downloaded, not installed yet" : `${rows.length} downloads to install`));

  for (const f of rows) {
    const row = el("div", "dl-row");
    const info = el("div", "dl-info");
    info.appendChild(el("div", "dl-name", f.name)).title = f.path;
    const who = f.characters.map((n) => n.replace(/^\./, "")).join(", ");
    info.appendChild(el("div", "muted small clip", [who, `${f.size_mb} MB`, ago(f.age_secs)].filter(Boolean).join(" · ")));
    row.appendChild(info);

    const btn = el("button", "btn", "Install");
    btn.title = "Install the characters in this archive";
    if (installing.has(f.path)) {
      btn.classList.add("wait");
      setBusy(btn, true);
    }
    btn.addEventListener("click", async () => {
      if (installing.has(f.path) || !hooks.installArchive) return;
      installing.add(f.path);
      try {
        // installArchive puts the bar on this button while the archive is read.
        await hooks.installArchive(f.path, btn);
      } finally {
        installing.delete(f.path);
        renderDownloads();
      }
    });
    row.appendChild(btn);
    out.appendChild(row);
  }
  box.replaceChildren(out);
}

async function poll() {
  let list;
  try {
    list = await invoke("recent_downloads");
  } catch {
    return;
  }
  files = list.filter((f) => f.looks_like_shimeji);
  if (baseline === null) baseline = new Set(files.map((f) => f.path));

  renderDownloads();

  const fresh = files.filter((f) => !f.installed);
  document.querySelector('.nav[data-view="catalog"]').classList.toggle("badge", fresh.length > 0);

  // Auto-install only what appeared AFTER the app started, and only once the
  // archive is fully written (the file is a few seconds old).
  if (autoOn() && !autoBusy && hooks.canAutoInstall()) {
    const next = fresh.find((f) => !baseline.has(f.path) && !autoTried.has(f.path) && f.age_secs >= 3);
    if (next) {
      autoTried.add(next.path);
      autoBusy = true;
      try {
        await hooks.autoInstall(next.path, next.name);
      } finally {
        autoBusy = false;
      }
      poll();
    }
  }
}

// ---------------------------------------------------------------- lifecycle

async function load(refresh = false) {
  if (!loaded) {
    // The reload button passes `refresh` and is already spinning: the line of text then says it without a second spinner.
    (refresh ? note : busy)($("cacho-note"), "Loading…");
    const grid = el("div", "cacho-grid-inner");
    fillSkeletons(grid, "s-cacho");
    $("cacho-grid").replaceChildren(grid);
  }
  try {
    const idx = await invoke("cachomon_index", { refresh });
    // Beta and Patreon-only characters cannot be downloaded, so they are left out.
    entries = idx.entries.filter((e) => e.availability === "public");
    loaded = true;

    let seen = [];
    try {
      seen = JSON.parse(store.get("cacho-seen", "[]"));
    } catch {}
    const seenSet = new Set(seen);
    newIds = seenSet.size ? new Set(entries.filter((e) => !seenSet.has(e.id)).map((e) => e.id)) : new Set();
    // Remember the list only after you have actually looked at it.
    setTimeout(() => store.set("cacho-seen", JSON.stringify(entries.map((e) => e.id))), 6000);

    renderGrid();
    if (idx.stale) note($("cacho-note"), `${plural(entries.length, "character")} (offline: showing the saved list)`);
  } catch (e) {
    note($("cacho-note"), String(e), "error");
    if (!loaded) $("cacho-grid").replaceChildren();
  }
}

/** Call when the collection changes: refreshes the "Installed" marks. */
export function syncCachomon() {
  if (loaded) renderGrid();
  poll();
}

export function showCachomon() {
  if (!loaded) load();
  poll();
  renderGuide();
}

// ---------------------------------------------------------------- first visit

// The download itself happens in the browser, on the site, so the app cannot do
// it for you. Nothing here says so until you have tried to find the button, hence
// a note the first time, with the real folder the app is watching.
let dir = "";

async function renderGuide() {
  const box = $("cacho-guide");
  if (store.get("cacho-guide-seen", "0") === "1") return box.classList.add("hidden");
  if (!dir) {
    try {
      dir = await invoke("downloads_path");
    } catch {}
  }
  // "~/Downloads" is what people recognise, not "/home/name/Downloads".
  $("cacho-dir").textContent = dir ? dir.replace(/^\/home\/[^/]+/, "~") : "your Downloads folder";
  $("cacho-guide-tail").textContent = autoOn()
    ? "The app notices it and installs it for you."
    : "It then shows up here, ready to install.";
  box.classList.remove("hidden");
}

const openDownloads = () => invoke("open_downloads").catch((e) => toast(String(e), "error"));

export function initCachomon({ installArchive, autoInstall, canAutoInstall, mine, showInCollection }) {
  hooks = { installArchive, autoInstall, canAutoInstall, mine, showInCollection };

  $("cacho-search").addEventListener("input", debounce(renderGrid, 150));
  $("cacho-reload").addEventListener("click", () => withBusy($("cacho-reload"), () => load(true)));
  $("cacho-pick").addEventListener("click", () => installArchive(null, $("cacho-pick")));
  $("cacho-folder").addEventListener("click", openDownloads);
  $("cacho-guide-close").addEventListener("click", () => {
    store.set("cacho-guide-seen", "1");
    $("cacho-guide").classList.add("hidden");
  });

  // What happens after a download lives in a small popover on the toolbar, where it
  // can always be found (it used to be a checkbox inside the list of downloads, which
  // is not on screen when there are none). Both are on unless turned off.
  $("cacho-auto").checked = autoOn();
  $("cacho-auto").addEventListener("change", (e) => {
    store.set("cacho-auto", e.target.checked ? "1" : "0");
    renderGuide();
    poll();
  });
  $("cacho-trash").checked = store.get("cacho-trash", "1") === "1";
  $("cacho-trash").addEventListener("change", (e) => store.set("cacho-trash", e.target.checked ? "1" : "0"));

  const pop = $("cacho-pop");
  const trigger = $("cacho-after");
  const setOpen = (open) => {
    pop.classList.toggle("hidden", !open);
    trigger.setAttribute("aria-expanded", String(open));
  };
  trigger.addEventListener("click", (e) => {
    e.stopPropagation();
    setOpen(pop.classList.contains("hidden"));
  });
  document.addEventListener("click", (e) => {
    if (!pop.classList.contains("hidden") && !pop.contains(e.target)) setOpen(false);
  });
  document.addEventListener("keydown", (e) => e.key === "Escape" && setOpen(false));

  // Watch the Downloads folder all the time, not only while this tab is open.
  poll();
  setInterval(() => {
    if (!document.hidden) poll();
  }, 5000);
  window.addEventListener("focus", poll);
}
