import {
  $, el, invoke, convertFileSrc, dlg, clip, note, busy, confirmDialog, promptDialog, toast, sprite, placeholder, store, pretty, plural, norm, fmtSize, debounce, openExternal,
  withBusy, latest, sleep, skeletons, fillSkeletons, healPictures, summonError,
} from "./util.js";
import { initDock } from "./dock.js";
import { initCachomon, showCachomon, syncCachomon } from "./cachomon.js";
import { initPalette, openPalette } from "./palette.js";
import { initStartup, refreshStartup } from "./startup.js";
import { attachHover, remoteFrames, localFrames } from "./anim.js";
import { Selection, selDot, paintSelection, showSelBar, hideSelBar } from "./select.js";
import { overlay } from "./overlay.js";
import { refreshAppSettings } from "./appsettings.js";
import { checkEnvironment } from "./environment.js";
import { initDragDrop } from "./dragdrop.js";

// ===========================================================================
// Navigation
// ===========================================================================

document.querySelectorAll(".nav").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".nav").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
    btn.classList.add("active");
    $(`view-${btn.dataset.view}`).classList.add("active");
    mineSel.clear();
    catSel.clear();
    document.querySelector("main").scrollTop = 0;
    healPictures($(`view-${btn.dataset.view}`));
    if (btn.dataset.view === "scene") {
      refreshPresets();
      loadScene();
    }
    if (btn.dataset.view === "settings") {
      loadConfig();
      refreshStartup();
    }
  });
});

/** Generic tab/segment switch: highlights the clicked one and calls onPick(value). */
function wireSeg(hostId, attr, onPick) {
  const btns = document.querySelectorAll(`#${hostId} .seg-btn`);
  btns.forEach((b) =>
    b.addEventListener("click", () => {
      btns.forEach((x) => x.classList.toggle("active", x === b));
      onPick(b.dataset[attr]);
    })
  );
}

// Catalog sources
wireSeg("source-tabs", "source", (src) => {
  const cacho = src === "cacho";
  catSel.clear();
  $("source-xyz").classList.toggle("hidden", cacho);
  $("source-cacho").classList.toggle("hidden", !cacho);
  store.set("catalog-source", src);
  if (cacho) showCachomon();
});

// Settings tabs
wireSeg("settings-tabs", "tab", (tab) => {
  document.querySelectorAll(".tab").forEach((t) => t.classList.toggle("active", t.id === `tab-${tab}`));
  store.set("settings-tab", tab);
  if (tab === "startup") refreshStartup();
  if (tab === "app") refreshAppSettings();
});

// ===========================================================================
// Catalog
// ===========================================================================

let packs = [];
let chars = [];
let index = null; // global index for searching everywhere
let currentPack = null;
let shownChars = []; // what the grid shows right now, in order (selection ranges follow it)
let protocols = null; // what the compositor offers, asked once (see loadConfig)

const catSel = new Selection({
  order: () => shownChars.map((c) => ({ key: c.slug, item: c })),
  onChange: syncCatSel,
});

/** What a list shows when it could not be loaded: one plain sentence and a way to try again. */
function retryBox(text, again) {
  const box = el("div", "empty-state");
  box.appendChild(el("div", "empty-title", text));
  const btn = el("button", "btn", "Try again");
  btn.addEventListener("click", () => withBusy(btn, again));
  box.appendChild(btn);
  return box;
}

async function loadPacks({ keep = false, refresh = false } = {}) {
  // `keep` means the reload button started this, and it is already spinning: a second spinner beside it said the
  // same thing twice. The line of text still says what is going on; the spinner is for when nothing else shows it.
  (keep ? note : busy)($("catalog-note"), "Loading the catalog…");
  // Refreshing a list that is already on screen leaves it there: the Refresh button
  // shows the waiting, and nothing flickers. Only an empty page gets placeholders.
  if (!keep || !$("packs").querySelector(".pack")) fillSkeletons($("packs"), "s-pack");
  $("packs").classList.remove("hidden");
  $("chars").classList.add("hidden");
  currentPack = null;
  $("crumb-sep").classList.add("hidden");
  $("crumb-pack").classList.add("hidden");
  $("pack-bar").classList.add("hidden");
  shownChars = [];
  catSel.clear();

  try {
    packs = await invoke("fetch_packs");
    note($("catalog-note"), `${packs.length} packs`);
    renderPacks(packs);
    if (refresh) {
      // The reload button renews everything the page is built from, as the cachomon one does: the list above, and
      // the index behind search and the counts on the cards. The button keeps waiting until both are done.
      note($("catalog-note"), "Updating the search index…");
      try {
        await refreshIndex();
        note($("catalog-note"), `${packs.length} packs`);
      } catch (e) {
        note($("catalog-note"), `${packs.length} packs. The search index could not be updated: ${e}`, "error");
      }
    } else {
      // The full index makes the progress on every pack exact; it is read from disk
      // when it was saved before, so this is quick and does not wait for a search.
      ensureIndex().catch(() => {});
    }
  } catch (e) {
    note($("catalog-note"), String(e), "error");
    $("packs").replaceChildren(retryBox("Could not load the catalog", () => loadPacks()));
  }
}

let shownPacks = []; // what the grid shows (the search may narrow it)

// The global index lists every character of every pack, which is what makes the
// progress on a pack card exact. It is loaded in the background (and by search).
let indexByPack = new Map(); // normalised pack title -> its characters
let indexLoading = null;

function setIndex(list) {
  index = list;
  indexByPack = new Map();
  for (const c of list) {
    const k = norm(c.pack_title);
    if (!indexByPack.has(k)) indexByPack.set(k, []);
    indexByPack.get(k).push(c);
  }
}

/** Asks the site for the whole index again (the saved copy is only trusted for half a day) and repaints what uses it. */
async function refreshIndex() {
  const list = await invoke("catalog_index", { refresh: true });
  setIndex(list);
  paintPacks();
  return list;
}

function ensureIndex() {
  if (index) return Promise.resolve(index);
  if (!indexLoading) {
    indexLoading = invoke("catalog_index", { refresh: false })
      .then((list) => {
        setIndex(list);
        paintPacks();
        return list;
      })
      .finally(() => (indexLoading = null));
  }
  return indexLoading;
}

// Packs this large (the community one has thousands) are not something you
// "complete", so they get a count and no progress bar.
const BIG_PACK = 300;

/**
 * How much of a pack you already have. With the global index this counts exact
 * characters; until it arrives it falls back on the group each installed
 * character was filed under, which is the pack's title.
 */
function packState(p) {
  const total = p.character_count || 0;
  const key = norm(p.title);
  let installed;
  if (index) {
    const have = haveNames();
    installed = (indexByPack.get(key) || []).filter((c) => have.has(norm(c.name))).length;
  } else {
    installed = mine.filter((m) => norm(m.pack_title) === key).length;
  }
  installed = Math.min(installed, total);
  return { installed, total, big: total > BIG_PACK, complete: total > 0 && installed >= total, started: installed > 0 };
}

function renderPacks(list) {
  const grid = $("packs");
  shownPacks = list;
  if (!list.length) {
    grid.replaceChildren(el("div", "empty-state", "Nothing found"));
    return;
  }
  const frag = document.createDocumentFragment();
  for (const p of list) {
    const card = document.createElement("div");
    card.className = "pack";
    card.dataset.slug = p.slug;
    card.addEventListener("click", () => openPack(p));

    const strip = document.createElement("div");
    strip.className = "pack-sprites";
    for (const s of p.preview_sprites.slice(0, 3)) strip.appendChild(sprite(s, p.title));
    card.appendChild(strip);

    card.appendChild(el("div", "pack-name", p.title));
    card.appendChild(el("div", "pack-count"));
    const meter = el("div", "pack-meter");
    meter.appendChild(document.createElement("i"));
    card.appendChild(meter);
    frag.appendChild(card);
  }
  grid.replaceChildren(frag);
  paintPacks();
}

/** Updates progress on the cards already on screen, without redrawing them. */
function paintPacks() {
  const bySlug = new Map(shownPacks.map((p) => [p.slug, p]));
  for (const card of $("packs").querySelectorAll(".pack[data-slug]")) {
    const p = bySlug.get(card.dataset.slug);
    if (!p) continue;
    const s = packState(p);
    card.classList.toggle("started", s.started && !s.big);
    card.classList.toggle("complete", s.complete);
    card.querySelector(".pack-meter i").style.setProperty("--p", `${s.total ? (s.installed / s.total) * 100 : 0}%`);
    const count = card.querySelector(".pack-count");
    const text = s.complete
      ? `✓ All ${s.total} installed`
      : s.started
        ? s.big
          ? `${s.installed} installed · ${plural(s.total, "character")}`
          : `${s.installed} of ${s.total} installed`
        : plural(s.total, "character");
    if (count.textContent !== text) count.textContent = text;
  }
}

const haveNames = () => new Set(mine.map((m) => norm(m.name)));

// Summons that overlap used to crash the overlay (verified), so they go one after
// another. A click that comes while one is running is not dropped (it used to be,
// with no sign that anything had been ignored): it waits its turn.
let summonQueue = Promise.resolve();
const inOrder = (task) => {
  const run = summonQueue.then(task, task);
  summonQueue = run.catch(() => {});
  return run;
};

function summonOne(name) {
  return inOrder(async () => {
    try {
      await invoke("summon_mascot", { name, count: 1 });
      refreshHealth();
      return true;
    } catch (e) {
      summonError(e);
      return false;
    }
  });
}

/** How much of the open pack you already have, and one button to get the rest. */
function renderPackBar() {
  const bar = $("pack-bar");
  bar.innerHTML = "";
  if (!currentPack || !chars.length || !mineLoaded) return bar.classList.add("hidden");

  const have = haveNames();
  const missing = chars.filter((c) => !have.has(norm(c.name)));
  const installed = chars.length - missing.length;
  bar.classList.remove("hidden");
  bar.classList.toggle("complete", !missing.length);

  const text = document.createElement("div");
  text.className = "banner-text";
  const line = document.createElement("div");
  line.innerHTML = missing.length
    ? `<b>${currentPack.title}</b> · ${installed} of ${chars.length} in your collection`
    : `<b>${currentPack.title}</b> · you have all ${plural(chars.length, "character")}`;
  const meter = el("div", "meter");
  const fill = document.createElement("i");
  fill.style.setProperty("--p", `${(installed / chars.length) * 100}%`);
  meter.appendChild(fill);
  text.append(line, meter);
  bar.appendChild(text);

  if (missing.length) {
    const btn = document.createElement("button");
    btn.className = "btn";
    btn.textContent = `Install the missing ${missing.length}`;
    btn.title = "Install every character of this pack you do not have yet";
    btn.addEventListener("click", () => withBusy(btn, () => installCharacters(missing)));
    bar.appendChild(btn);
  } else {
    // An empty right-hand side reads as "the button failed to load", so the
    // finished state says so itself.
    bar.appendChild(el("div", "banner-done", "✓ Complete"));
  }
}

async function openPack(p, { keep = false } = {}) {
  currentPack = p;
  $("packs").classList.add("hidden");
  $("chars").classList.remove("hidden");
  if (!keep || !$("chars").querySelector(".char")) {
    // A pack knows how many characters it has, so the placeholders are that many: the
    // page keeps the same shape when the real cards replace them.
    const known = p.character_count || 0;
    if (known) skeletons($("chars"), Math.min(known, 60), "s-char");
    else fillSkeletons($("chars"), "s-char");
  }
  $("crumb-sep").classList.remove("hidden");
  $("crumb-pack").classList.remove("hidden");
  $("crumb-pack").textContent = p.title;
  catSel.clear();
  (keep ? note : busy)($("catalog-note"), "Loading characters…");

  try {
    chars = await invoke("fetch_characters", { packSlug: p.slug });
    note($("catalog-note"), plural(chars.length, "character"));
    renderChars(chars);
    renderPackBar();
  } catch (e) {
    note($("catalog-note"), String(e), "error");
    $("chars").replaceChildren(retryBox("Could not load this pack", () => openPack(p)));
  }
}

// A pack can hold thousands of characters (the community one has 2,300). Drawing
// them all made the page heavy: the first selection re-styled every card at once,
// and it lagged. Cards are made a screenful or two at a time, and the next lot when
// you scroll near the end.
const CHUNK = 90;
let chunkWatch = null;

function charCard(c, have) {
  const card = document.createElement("div");
  card.className = "char";
  card.dataset.key = c.slug;
  if (have.has(norm(c.name))) {
    card.classList.add("have");
    const mark = document.createElement("span");
    mark.className = "char-have";
    mark.textContent = "✓";
    mark.title = "Already in your collection";
    card.appendChild(mark);
  }

  card.appendChild(selDot(catSel, c.slug, c));
  const mineName = have.has(norm(c.name)) ? mine.find((m) => norm(m.name) === norm(c.name))?.name : null;
  if (mineName) card.title = "In your collection: click to show it there";
  card.addEventListener("click", (e) => {
    // With something selected (or Ctrl/Shift held) a click selects. Otherwise a character
    // you have leads to its place in the Collection, and one you do not opens its preview.
    if (catSel.size || e.ctrlKey || e.metaKey || e.shiftKey) catSel.toggle(c.slug, c, { range: e.shiftKey });
    else if (mineName) showInCollection([mineName]);
    else openSheet(c);
  });

  const stage = document.createElement("div");
  stage.className = "stage";
  const img = sprite(c.sprite, c.name);
  stage.appendChild(img);
  card.appendChild(stage);
  attachHover(card, img, () => remoteFrames(c.sprite));

  const name = document.createElement("div");
  name.className = "char-name";
  name.textContent = c.name;
  name.title = c.name;
  card.appendChild(name);

  // In global search, show which pack the character belongs to
  if (!currentPack && c.pack_title) {
    const sub = document.createElement("div");
    sub.className = "char-sub";
    sub.textContent = c.pack_title;
    card.appendChild(sub);
  }
  return card;
}

function renderChars(list) {
  const grid = $("chars");
  const have = haveNames();
  shownChars = list;
  chunkWatch?.disconnect();
  chunkWatch = null;
  if (!list.length) {
    grid.replaceChildren(el("div", "empty-state", "Empty"));
    catSel.prune();
    return;
  }

  let drawn = 0;
  const more = () => {
    const out = document.createDocumentFragment();
    for (const c of list.slice(drawn, drawn + CHUNK)) out.appendChild(charCard(c, have));
    drawn += CHUNK;
    paintSelection(out, catSel); // cards that are selected already look it
    return out;
  };

  grid.replaceChildren(more());
  if (drawn < list.length) {
    const end = el("div", "chunk-end");
    grid.appendChild(end);
    chunkWatch = new IntersectionObserver(
      (entries) => {
        if (!entries.some((e) => e.isIntersecting)) return;
        end.before(more());
        if (drawn >= list.length) {
          chunkWatch.disconnect();
          end.remove();
        }
      },
      { root: document.querySelector("main"), rootMargin: "700px 0px" }
    );
    chunkWatch.observe(end);
  }
  catSel.prune();
  paintSelection(grid, catSel);
}

$("crumb-root").addEventListener("click", loadPacks);
$("reload-catalog").addEventListener("click", () =>
  withBusy($("reload-catalog"), () => (currentPack ? openPack(currentPack, { keep: true }) : loadPacks({ keep: true, refresh: true })))
);

// --- Global search ---

let searchTimer;
$("search").addEventListener("input", (e) => {
  const q = e.target.value.trim().toLowerCase();
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => runSearch(q), 220);
});

async function runSearch(q) {
  if (!q) {
    // Go back to where we were
    if (currentPack) renderChars(chars);
    else {
      $("packs").classList.remove("hidden");
      $("chars").classList.add("hidden");
      renderPacks(packs);
      note($("catalog-note"), `${packs.length} packs`);
    }
    return;
  }

  // Inside an open pack we search locally, which is instant.
  if (currentPack) {
    renderChars(chars.filter((c) => c.name.toLowerCase().includes(q)));
    return;
  }

  // At the top level we search the whole catalog.
  if (!index) {
    busy($("catalog-note"), "Building the search index…");
    try {
      await ensureIndex();
    } catch (e) {
      note($("catalog-note"), String(e), "error");
      return;
    }
  }

  const found = index.filter(
    (c) => c.name.toLowerCase().includes(q) || (c.pack_title || "").toLowerCase().includes(q)
  );

  $("packs").classList.add("hidden");
  $("chars").classList.remove("hidden");
  note($("catalog-note"), `${found.length} found`);
  renderChars(found.slice(0, 300));
}

// --- Selecting several characters to install ---

function visibleChars() {
  const q = $("search").value.trim().toLowerCase();
  return q ? chars.filter((c) => c.name.toLowerCase().includes(q)) : chars;
}

function lastSearchResult() {
  const q = $("search").value.trim().toLowerCase();
  if (!index || !q) return chars;
  return index
    .filter((c) => c.name.toLowerCase().includes(q) || (c.pack_title || "").toLowerCase().includes(q))
    .slice(0, 300);
}

function syncCatSel(sel) {
  paintSelection($("chars"), sel);
  showSelBar({
    count: sel.size,
    total: shownChars.length,
    actions: [{ label: `Install ${sel.size}`, kind: "btn", run: installSelected }],
    onAll: () => sel.selectAll(),
    onClear: () => sel.clear(),
  });
}

async function installSelected() {
  const list = catSel.items();
  if (!list.length) return;
  // The bar goes away at once; progress continues in the panel at the bottom right.
  catSel.clear();
  await installCharacters(list);
  renderChars(currentPack ? visibleChars() : lastSearchResult());
}

/**
 * Installs characters from the catalog. Ones that are already installed are
 * left alone: we ask whether to replace them. Progress shows in the bottom-right
 * panel, which is also where it can be cancelled.
 */
async function installCharacters(list) {
  let res;
  try {
    res = await invoke("install_characters", { characters: list, overwrite: false });
  } catch (e) {
    return toast(String(e), "error");
  }

  let installed = res.installed.length;
  const failed = [...res.failed];
  const cancelled = res.cancelled.length;
  let skipped = res.skipped;

  // After a Cancel, asking "replace them?" would start again what was just stopped.
  if (skipped.length && !cancelled) {
    const names = skipped.map(pretty).join(", ");
    const replace = await confirmDialog(`Already installed: ${names}.\nReplace them?`, "Replace", "Keep");
    if (replace) {
      try {
        const again = await invoke("install_characters", {
          characters: list.filter((c) => skipped.includes(c.name)),
          overwrite: true,
        });
        installed += again.installed.length;
        failed.push(...again.failed);
        skipped = [];
      } catch (e) {
        toast(String(e), "error");
      }
    }
  }

  if (failed.length) {
    toast(`Failed: ${failed.map(([n]) => n).join(", ")}`, "error");
  } else if (cancelled) {
    toast(installed ? `Cancelled. ${installed} installed before that.` : "Cancelled");
  } else if (installed) {
    toast(skipped.length ? `${installed} installed, ${skipped.length} skipped` : `${installed} installed`, "ok");
  } else if (skipped.length) {
    toast("Already installed");
  }
  await repairAfterInstall();
  await loadMine();
}

/**
 * A character that lacks pictures brings the overlay down when it appears (the site does
 * not have every picture for every character). Whatever was just installed is checked,
 * and any gap is filled with a copy of the nearest picture: files are only added.
 */
async function repairAfterInstall() {
  const fixed = await invoke("repair_characters").catch(() => []);
  if (fixed.length) {
    toast(`${fixed.map((f) => pretty(f.name)).join(", ")}: some pictures were missing (they would have crashed the overlay), so they were filled in`, "ok");
  }
}

// ===========================================================================
// Dialog: animation player + install
// ===========================================================================

let sheetChar = null;
let localMode = false;
let playTimer = null;

function stopPlayer() {
  clearInterval(playTimer);
  playTimer = null;
  $("player-play").textContent = "▶";
}

function openSheet(c) {
  sheetChar = c;
  localMode = false;
  stopPlayer();

  $("m-name").textContent = c.name;
  $("m-meta").textContent = c.pack_title;
  $("m-sprite").innerHTML = "";
  $("m-sprite").appendChild(sprite(c.sprite, c.name));
  $("m-list").innerHTML = "";
  $("m-all-wrap").classList.add("hidden");
  note($("m-note"), "");

  // Player. It plays the frames that really exist: they are probed first (a run of
  // 46 at most), so the slider has exactly as many steps as there are frames and its
  // filled part follows the handle. It used to assume 46, show broken pictures for the
  // missing ones, and start with the bar half full.
  $("player-wrap").classList.remove("hidden");
  const scrub = $("player-scrub");
  let frames = [];
  const paintFrame = (n) => {
    if (!frames.length) return;
    n = Math.max(1, Math.min(frames.length, Number(n) || 1));
    scrub.value = n;
    $("player-img").src = frames[n - 1];
    $("player-num").textContent = `${n} / ${frames.length}`;
    scrub.style.setProperty("--p", `${frames.length > 1 ? ((n - 1) / (frames.length - 1)) * 100 : 0}%`);
  };
  scrub.disabled = true;
  scrub.min = 1;
  scrub.max = 2;
  scrub.value = 1;
  scrub.style.setProperty("--p", "0%");
  $("player-img").src = c.sprite; // the first picture at once, while the rest are looked for
  $("player-num").textContent = "…";
  $("player-play").disabled = true;
  remoteFrames(c.sprite).then((list) => {
    if (sheetChar !== c) return; // another character was opened meanwhile
    frames = list.length ? list : [c.sprite];
    scrub.max = Math.max(2, frames.length);
    scrub.disabled = frames.length < 2;
    $("player-play").disabled = frames.length < 2;
    paintFrame(1);
  });
  scrub.oninput = () => paintFrame(scrub.value);

  $("player-play").onclick = () => {
    if (playTimer) return stopPlayer();
    $("player-play").textContent = "❚❚";
    playTimer = setInterval(() => {
      const next = Number(scrub.value) + 1;
      paintFrame(next > frames.length ? 1 : next);
    }, 160);
  };

  const btn = $("m-install");
  btn.disabled = false;
  btn.textContent = "Install";
  btn.onclick = installFromSheet;

  $("modal").classList.remove("hidden");
}

function closeSheet() {
  stopPlayer();
  $("modal").classList.add("hidden");
  if (localMode) invoke("cancel_local").catch(() => {});
  localMode = false;
  sheetChar = null;
}

$("m-close").addEventListener("click", closeSheet);
$("modal-bg").addEventListener("click", closeSheet);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("modal").classList.contains("hidden")) closeSheet();
});

async function installFromSheet() {
  const character = sheetChar;
  closeSheet();
  await installCharacters([character]);
}

// --- Local archive (may contain many characters) ---

function renderPicks(names) {
  const list = $("m-list");
  list.innerHTML = "";
  const have = haveNames();
  for (const n of names) {
    const isHave = have.has(norm(n));
    const label = document.createElement("label");
    label.className = "pick";
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.value = n;
    cb.checked = !isHave;
    cb.addEventListener("change", syncPicks);
    const span = document.createElement("span");
    span.textContent = pretty(n);
    label.append(cb, span);
    if (isHave) {
      const tag = document.createElement("span");
      tag.className = "muted small";
      tag.textContent = "in your collection";
      label.appendChild(tag);
    }
    list.appendChild(label);
  }
  $("m-all").checked = names.every((n) => !have.has(norm(n)));
  $("m-all-wrap").classList.toggle("hidden", names.length < 2);
  syncPicks();
}

const picked = () => [...$("m-list").querySelectorAll("input:checked")].map((i) => i.value);

function syncPicks() {
  const n = picked().length;
  const btn = $("m-install");
  btn.disabled = n === 0;
  btn.textContent = n === 0 ? "Install" : `Install (${n})`;
}

$("m-all").addEventListener("change", (e) => {
  $("m-list").querySelectorAll("input").forEach((i) => (i.checked = e.target.checked));
  syncPicks();
});

// ===========================================================================
// Collection
// ===========================================================================

// The list from last time is kept on disk and drawn at once. Reading the real one
// takes a moment (it asks shimejictl), and until it arrived every page that shows
// characters was empty: no cards, a blank "Summon" list, letters instead of
// pictures. That is what looked like characters disappearing.
function savedMine() {
  try {
    const list = JSON.parse(store.get("mine-cache", "[]"));
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}
let mine = savedMine();
// Until the first load finishes, an empty `mine` does not mean an empty
// collection. Anything that counts what is missing has to wait, or it would claim
// every character in an open pack still needs installing.
let mineLoaded = mine.length > 0;
let favOnly = false;

// Cards are built once and moved around, not rebuilt every time the list is
// redrawn. Rebuilding meant every sprite was fetched again, which showed as a
// flicker whenever a filter was switched on or off.
const cardCache = new Map(); // name -> { sig, node }
function mineCard(m) {
  const sig = [m.favorite, m.sprite_path, m.pack_title, m.size_bytes].join("|");
  let hit = cardCache.get(m.name);
  if (!hit || hit.sig !== sig) {
    hit = { sig, node: buildMineCard(m) };
    cardCache.set(m.name, hit);
  }
  // The caption (the universe) shows only when the list is not grouped by it: the
  // one thing that depends on the layout, so it is updated in place.
  const caption = mineGroup ? "" : m.pack_title || "";
  const sub = hit.node.querySelector(".char-sub");
  if (sub.textContent !== caption) sub.textContent = caption;
  return hit.node;
}

// The order the cards are on screen in: sorted, filtered, and grouped the same way as renderMine.
const mineSel = new Selection({
  order: () => screenOrder().map((m) => ({ key: m.name, item: m })),
  onChange: syncMineSel,
});

const mineLoad = latest();

/**
 * Reads the collection.
 *
 * - Only the newest request may apply its answer.
 * - An empty answer for a collection that had characters a moment ago is asked
 *   again before it is believed, and a failure never blanks what is on screen.
 * - A failed read keeps showing the last list rather than an empty page.
 */
async function loadMine({ quiet = false } = {}) {
  const ticket = mineLoad.next();
  if (!quiet && !mineLoaded) busy($("mine-note"), "Loading…");

  let fresh = null;
  let error = null;
  for (let attempt = 0; attempt < 3 && fresh === null; attempt++) {
    if (attempt) await sleep(350 * attempt);
    try {
      const list = await invoke("list_installed");
      if (!mineLoad.isCurrent(ticket)) return;
      if (list.length || !mine.length || attempt === 2) fresh = list;
    } catch (e) {
      if (!mineLoad.isCurrent(ticket)) return;
      error = e;
    }
  }

  if (fresh === null) {
    if (!mineLoaded) {
      note($("mine-note"), String(error), "error");
      $("mine").classList.remove("grouped");
      $("mine").replaceChildren(retryBox("Could not read your collection", () => loadMine()));
    } else if (!quiet) {
      note($("mine-note"), "Could not refresh · showing the last list", "error");
    }
    return;
  }
  applyMine(fresh, { quiet });
}

function applyMine(fresh, { quiet = false } = {}) {
  const same = JSON.stringify(fresh) === JSON.stringify(mine);
  const wasLoaded = mineLoaded;
  mine = fresh;
  mineLoaded = true;
  if (!same) store.set("mine-cache", JSON.stringify(mine));
  if (quiet && same && wasLoaded) return;

  const totalBytes = mine.reduce((a, m) => a + (m.size_bytes || 0), 0);
  note($("mine-note"), mine.length ? `${mine.length} · ${fmtSize(totalBytes)}` : "");
  const names = new Set(mine.map((m) => m.name));
  for (const k of [...cardCache.keys()]) if (!names.has(k)) cardCache.delete(k);
  renderMine();
  syncCachomon();
  refreshStartup({ status: false });
  paintPacks();
  renderPackBar();
  if (currentPack) renderChars(visibleChars());
  refreshPresets();
  maybeAutoMatch();
}

// Pictures and groups fill in by themselves: no buttons, once per character.
const matchTried = new Set();
let matching = false;
async function maybeAutoMatch() {
  if (matching) return;
  const need = mine.filter((m) => (!m.sprite_path || !m.pack_title) && !matchTried.has(m.name));
  if (!need.length) return;
  need.forEach((m) => matchTried.add(m.name));

  matching = true;
  try {
    await invoke("match_sprites");
    await loadMine({ quiet: true });
  } catch {
    // Quiet background job: a failure just leaves the card as it was.
  } finally {
    matching = false;
  }
}

function visibleMine() {
  const q = $("mine-search").value.trim().toLowerCase();
  return mine
    .filter((m) => !favOnly || m.favorite)
    .filter(
      (m) => !q || m.name.toLowerCase().includes(q) || (m.pack_title || "").toLowerCase().includes(q)
    );
}

// --- Order and groups ---

// "size" used to be a third choice; anyone who had it saved gets the default.
let mineSort = store.get("mine-sort", "new") === "name" ? "name" : "new"; // new | name
let mineGroup = store.get("mine-group", "1") === "1";
const collapsedGroups = new Set();
const NO_GROUP = "No group";

function sortMine(list) {
  const by =
    mineSort === "name"
      ? (a, b) => a.name.localeCompare(b.name)
      : (a, b) => (b.installed_at || 0) - (a.installed_at || 0) || a.name.localeCompare(b.name);
  return [...list].sort(by);
}

/** Groups by universe; ordered by their newest member (or alphabetically). */
function groupMine(list) {
  const map = new Map();
  for (const m of list) {
    const key = m.pack_title || NO_GROUP;
    if (!map.has(key)) map.set(key, []);
    map.get(key).push(m);
  }
  const newest = (items) => Math.max(...items.map((m) => m.installed_at || 0));
  const groups = [...map].map(([title, items]) => ({ title, items }));
  groups.sort((a, b) => {
    if (a.title === NO_GROUP) return 1;
    if (b.title === NO_GROUP) return -1;
    if (mineSort === "name") return a.title.localeCompare(b.title);
    return newest(b.items) - newest(a.items);
  });
  return groups;
}

async function summonMany(names, label) {
  // Helper prototypes (.Hornet_Needle and the like) are never summoned on their own.
  const list = names.filter((n) => !n.startsWith("."));
  if (!list.length) return false;
  if (!(await confirmCrowd(list.length))) return false;
  await inOrder(async () => {
    try {
      // One request for everyone: they arrive together, in a second or two, over a
      // single connection to the overlay (one call per character took a second each
      // and, with dozens, was more than the overlay could take).
      toast(spawnMessage(await invoke("summon_batch", { items: list.map((n) => [n, 1]) })), "ok");
    } catch (e) {
      summonError(e);
    } finally {
      refreshActive();
      refreshHealth();
    }
  });
  return true;
}

/**
 * Asks before a crowd, and says why.
 *
 * Measured on a live system: wl_shimeji's overlay segfaults on its own while many
 * characters are moving about — sooner the more of them there are. The app cannot prevent
 * that, so it says so once, before the crowd, instead of after.
 */
async function confirmCrowd(count, already = null) {
  const total = (already ?? onScreenCount()) + count;
  if (count <= 30 && total < 120) return true;
  const crowd = total > 120 ? `That would make about ${total} on screen. ` : "";
  return confirmDialog(
    `Summon ${plural(count, "character")}?\n\n${crowd}wl_shimeji tends to crash by itself with a large crowd moving about — the app will notice and offer to bring everyone back, but it cannot stop it happening.`,
    "Summon",
    "Cancel",
    false,
  );
}

/** How many are on screen right now, as far as the app knows. */
const onScreenCount = () => (sceneKnown ? scene.reduce((a, [, c]) => a + c, 0) : 0);

/** "Summoned 22", and a note about anyone who is no longer installed. */
function spawnMessage(result) {
  const gone = result.missing?.length ? ` (not installed any more: ${result.missing.map(pretty).join(", ")})` : "";
  // They came one at a time, which is slow and obvious; say why rather than leave it odd.
  const slow = result.slow_reason ? `. They came one by one: the quick way failed (${result.slow_reason})` : "";
  const wait = result.restart_needed?.length
    ? `. ${result.restart_needed.map(pretty).join(", ")} lacked pictures: fixed, but it shows up after the overlay restarts (Dismiss all, then summon again)`
    : "";
  return `Summoned ${result.spawned}${gone}${wait}${slow}`;
}

/** How the collection is laid out right now: a flat list, or groups with "Recently added" on top. */
function layoutMine() {
  const list = sortMine(visibleMine());
  if (!mineGroup) return { list, groups: null };

  // "Recently added": fresh installs do not drown in their groups (or, if they have
  // no group yet, at the bottom of "No group"). Only when sorted by Newest.
  if (mineSort === "new" && list.length > 8) {
    const dayAgo = Date.now() / 1000 - 24 * 3600;
    const recent = list.filter((m) => (m.installed_at || 0) > dayAgo).slice(0, 8);
    if (recent.length) {
      const rest = list.filter((m) => !recent.includes(m));
      return { list, groups: [{ title: "Recently added", items: recent }, ...groupMine(rest)] };
    }
  }
  return { list, groups: groupMine(list) };
}

/** The cards that are on screen, top to bottom. A collapsed group has none. */
function screenOrder() {
  const { list, groups } = layoutMine();
  if (!groups) return list;
  return groups.filter((g) => !collapsedGroups.has(g.title)).flatMap((g) => g.items);
}

function renderMine() {
  const grid = $("mine");

  // Nothing has arrived yet (and there is no saved copy): placeholders, not an empty page.
  if (!mineLoaded) {
    grid.classList.remove("grouped");
    fillSkeletons(grid, "s-char");
    return;
  }

  grid.classList.toggle("grouped", mineGroup);
  const { list, groups } = layoutMine();
  const out = document.createDocumentFragment();

  if (!list.length) {
    grid.classList.remove("grouped");
    const box = document.createElement("div");
    box.className = "empty-state";
    if (mine.length) {
      box.textContent = "Nothing found";
    } else {
      box.innerHTML = '<div class="empty-title">Your collection is empty</div>';
      const go = document.createElement("button");
      go.className = "btn";
      go.textContent = "Open the Catalog";
      go.addEventListener("click", () => document.querySelector('.nav[data-view="catalog"]').click());
      box.append(go);
    }
    out.appendChild(box);
    grid.replaceChildren(out);
    mineSel.prune();
    return;
  }

  if (!groups) {
    list.forEach((m) => out.appendChild(mineCard(m)));
    grid.replaceChildren(out);
    mineSel.prune();
    paintMine();
    return;
  }

  for (const g of groups) {
    const sec = document.createElement("section");
    sec.className = "group";
    sec.dataset.keys = JSON.stringify(g.items.map((m) => m.name));
    const isCollapsed = collapsedGroups.has(g.title);

    const head = document.createElement("header");
    head.className = "group-head";

    // The same circle as on a card, for the whole group.
    const dot = document.createElement("button");
    dot.className = "grp-dot";
    dot.type = "button";
    dot.title = "Select this group";
    dot.setAttribute("aria-label", `Select ${g.title}`);
    dot.addEventListener("click", () => mineSel.toggleMany(g.items.map((m) => ({ key: m.name, item: m }))));
    head.appendChild(dot);

    const toggle = document.createElement("button");
    toggle.className = "group-toggle";
    toggle.innerHTML = `<span class="chev">${isCollapsed ? "▸" : "▾"}</span>`;
    const title = document.createElement("span");
    title.className = "group-title";
    title.textContent = g.title;
    const cnt = document.createElement("span");
    cnt.className = "count";
    cnt.textContent = String(g.items.length);
    toggle.append(title, cnt);
    toggle.addEventListener("click", () => {
      isCollapsed ? collapsedGroups.delete(g.title) : collapsedGroups.add(g.title);
      renderMine();
    });
    head.appendChild(toggle);

    const sp = document.createElement("div");
    sp.className = "spacer";
    head.appendChild(sp);

    const call = document.createElement("button");
    call.className = "link-btn group-summon";
    call.textContent = "Summon all";
    call.addEventListener("click", () => withBusy(call, () => summonMany(g.items.map((m) => m.name), g.title)));
    head.appendChild(call);
    sec.appendChild(head);

    if (!isCollapsed) {
      const inner = document.createElement("div");
      inner.className = "char-grid";
      g.items.forEach((m) => inner.appendChild(mineCard(m)));
      sec.appendChild(inner);
    }
    out.appendChild(sec);
  }
  grid.replaceChildren(out);
  mineSel.prune();
  paintMine();
}

function buildMineCard(m) {
  const card = document.createElement("div");
  card.className = "char";
  card.dataset.key = m.name;

  card.appendChild(selDot(mineSel, m.name, m));
  card.addEventListener("click", (e) => {
    if (mineSel.size || e.ctrlKey || e.metaKey || e.shiftKey) mineSel.toggle(m.name, m, { range: e.shiftKey });
  });

  const star = document.createElement("button");
  star.className = `char-badge ${m.favorite ? "on" : ""}`;
  star.textContent = "★";
  star.title = m.favorite ? "Remove from favorites" : "Add to favorites";
  star.setAttribute("aria-label", star.title);
  star.addEventListener("click", async (e) => {
    e.stopPropagation();
    try {
      const fav = await invoke("toggle_favorite", { name: m.name });
      const current = mine.find((x) => x.name === m.name);
      if (current) current.favorite = fav;
      m.favorite = fav;
      cardCache.delete(m.name);
      renderMine();
    } catch (err) {
      toast(String(err), "error");
    }
  });
  card.appendChild(star);

  const stage = document.createElement("div");
  stage.className = "stage";
  if (m.sprite_path) {
    const img = sprite(convertFileSrc(m.sprite_path), m.name);
    stage.appendChild(img);
    attachHover(card, img, () => localFrames(m.name));
  } else {
    // The character may have been installed before this app existed, so it has no sprite yet.
    stage.appendChild(placeholder(m.name));
  }
  card.appendChild(stage);

  const name = document.createElement("div");
  name.className = "char-name";
  name.textContent = pretty(m.name);
  name.title = m.size_bytes ? `${m.name} · ${fmtSize(m.size_bytes)}` : m.name;
  card.appendChild(name);

  // The line under the name shows the caption (the universe, when the list is not
  // already grouped by it) and, when you point at the card, the buttons in its place.
  const foot = document.createElement("div");
  foot.className = "char-foot";
  const sub = document.createElement("div");
  sub.className = "char-sub";
  sub.textContent = mineGroup ? "" : m.pack_title || "";
  foot.appendChild(sub);

  const actions = document.createElement("div");
  actions.className = "char-actions";

  const call = document.createElement("button");
  call.className = "btn";
  call.textContent = "Summon";
  call.addEventListener("click", (e) => {
    e.stopPropagation();
    withBusy(call, async () => {
      if (await summonOne(m.name)) refreshActive();
    });
  });

  const del = document.createElement("button");
  del.className = "ghost icon";
  del.textContent = "✕";
  del.title = "Delete from disk";
  del.addEventListener("click", async (e) => {
    e.stopPropagation();
    const ok = await confirmDialog(`Delete “${pretty(m.name)}” from disk?`);
    if (!ok) return;
    await withBusy(del, async () => {
      try {
        await invoke("remove_mascot", { name: m.name });
        toast(`Deleted ${pretty(m.name)}`, "ok");
        await loadMine();
      } catch (err) {
        toast(String(err), "error");
      }
    });
  });

  actions.append(call, del);
  foot.appendChild(actions);
  card.appendChild(foot);
  return card;
}

/** After an install: shows the Collection with the new characters pointed out. */
function showInCollection(names) {
  // Whatever hid them (a filter, a collapsed group) must not.
  favOnly = false;
  $("filter-fav").classList.remove("on");
  $("mine-search").value = "";
  const keys = new Set(names.map(norm));
  for (const m of mine) if (keys.has(norm(m.name))) collapsedGroups.delete(m.pack_title || NO_GROUP);
  goto("mine");
  renderMine();
  setTimeout(() => {
    const cards = [...$("mine").querySelectorAll(".char[data-key]")].filter((c) => keys.has(norm(c.dataset.key)));
    if (!cards.length) return;
    cards[0].scrollIntoView({ block: "center", behavior: "smooth" });
    for (const c of cards) {
      c.classList.remove("flash");
      void c.offsetWidth; // restart the animation if it is already running
      c.classList.add("flash");
      setTimeout(() => c.classList.remove("flash"), 3200);
    }
  }, 280);
}

// --- Selecting cards ---

/** Marks selected cards and group circles in place, without redrawing the sprites. */
function paintMine() {
  const grid = $("mine");
  paintSelection(grid, mineSel);
  grid.querySelectorAll(".group[data-keys]").forEach((sec) => {
    const keys = JSON.parse(sec.dataset.keys);
    const n = keys.filter((k) => mineSel.has(k)).length;
    const dot = sec.querySelector(".grp-dot");
    dot.classList.toggle("on", n > 0 && n === keys.length);
    dot.classList.toggle("some", n > 0 && n < keys.length);
  });
}

function syncMineSel(sel) {
  paintMine();
  showSelBar({
    count: sel.size,
    total: screenOrder().length,
    actions: [
      {
        label: "Summon",
        kind: "btn",
        run: async () => {
          // The selection goes when the summon is done, not before: clearing it first hid the bar (and the
          // spinner on this button) at the moment of the click, so nothing showed that anything was happening.
          // Changing your mind at the crowd question keeps the selection, too.
          if (await summonMany(sel.keys(), "Selected")) sel.clear();
        },
      },
      // Chooses its own moment to wait: not while the save dialog is open.
      { label: "Export", kind: "ghost", plain: true, run: (btn) => exportCharacters(sel.keys(), btn) },
      { label: "Delete", kind: "danger", run: deleteSelected },
    ],
    onAll: () => sel.selectAll(),
    onClear: () => sel.clear(),
  });
}

async function deleteSelected() {
  const names = mineSel.keys();
  if (!names.length) return;
  const ok = await confirmDialog(`Delete ${plural(names.length, "character")} from disk?`);
  if (!ok) return;
  try {
    toast(await invoke("remove_many", { names }), "ok");
  } catch (e) {
    toast(String(e), "error");
  }
  mineSel.clear();
  loadMine();
}

$("filter-fav").addEventListener("click", () => {
  favOnly = !favOnly;
  $("filter-fav").classList.toggle("on", favOnly);
  renderMine();
});

$("mine-search").addEventListener("input", debounce(renderMine, 150));

// Sort order and grouping: the choice is remembered.
document.querySelectorAll("#mine-sort .seg-btn").forEach((b) =>
  b.classList.toggle("active", b.dataset.sort === mineSort)
);
wireSeg("mine-sort", "sort", (v) => {
  mineSort = v;
  store.set("mine-sort", v);
  renderMine();
});
$("mine-group").classList.toggle("on", mineGroup);
$("mine-group").addEventListener("click", () => {
  mineGroup = !mineGroup;
  store.set("mine-group", mineGroup ? "1" : "0");
  $("mine-group").classList.toggle("on", mineGroup);
  renderMine();
});

// There is no Refresh button: the collection is re-read when the window gets
// focus again, which is when it could have changed (say, a character added from
// the terminal). Nothing is redrawn if nothing is different.
window.addEventListener(
  "focus",
  debounce(() => {
    loadMine({ quiet: true });
    healPictures();
  }, 400)
);

const EXPORT_STOPPED = "Export stopped."; // what the backend says when the export was cancelled

/**
 * Saves characters as one .zip: the given ones (the selection), or the whole collection when `names` is null.
 * `btn` is the button that asked, and waits while the engine works (not while you choose where to save).
 */
async function exportCharacters(names, btn) {
  const some = Array.isArray(names) && names.length > 0;
  const path = await dlg.save({
    title: some ? "Export the selected characters" : "Export the collection",
    defaultPath: some ? "shimeji-selection.zip" : "shimeji-collection.zip",
    filters: [{ name: "Zip archive", extensions: ["zip"] }],
  });
  if (!path) return;

  await withBusy(btn, async () => {
    try {
      // Progress goes to the panel at the bottom right (the one installs use); this is the final word.
      const target = /\.zip$/i.test(path) ? path : `${path}.zip`;
      toast(await invoke("export_collection", { path: target, names: some ? names : null }), "ok");
    } catch (e) {
      // Pressing Cancel in the panel is not an error.
      if (String(e) !== EXPORT_STOPPED) toast(String(e), "error");
    }
  });
}

// With something selected the button saves that, and with nothing selected the whole collection.
$("export-all").addEventListener("click", () => exportCharacters(mineSel.size ? mineSel.keys() : null, $("export-all")));

// ===========================================================================
// Scene: who is on screen, presets
// ===========================================================================

// What is on screen right now: [[name, count]]. `sceneReal` is true when this
// was read from the overlay itself; if that is not possible we fall back to
// counting what this session summoned (and then a per-character ✕ cannot work).
let scene = [];
let sceneReal = false;
// Until the overlay has answered once we do not know who is there, and saying
// "Nobody on screen" then was wrong (and, next to presets, ugly).
let sceneKnown = false;
let sceneStale = false; // the last question got no answer, so this is how it was
let sceneFailed = false; // never got an answer, and there is nothing else to go on
const sceneLoad = latest();

async function loadScene() {
  const ticket = sceneLoad.next();
  if (!sceneKnown) {
    sceneFailed = false;
    busy($("active-note"), "Checking who is on screen…");
    renderScene();
  }

  // Ask up to three times: an overlay that is busy for a moment is not "nobody there".
  let list = null;
  for (let attempt = 0; attempt < 3 && list === null; attempt++) {
    if (attempt) await sleep(450 * attempt);
    try {
      list = await invoke("on_screen");
    } catch {
      if (!sceneLoad.isCurrent(ticket)) return;
    }
  }
  if (!sceneLoad.isCurrent(ticket)) return;

  if (list !== null) {
    scene = list;
    sceneReal = true;
    sceneKnown = true;
    sceneStale = false;
    sceneFailed = false;
    overlay.remember(list);
  } else if (sceneKnown) {
    sceneStale = true; // keep what we saw last time
  } else {
    // No answer at all yet. The tally of what this session summoned is only worth
    // showing if it has someone in it; an empty one would claim "nobody" falsely.
    let tally = [];
    try {
      tally = (await invoke("summoned_tally")).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    } catch {}
    if (!sceneLoad.isCurrent(ticket)) return;
    if (tally.length) {
      scene = tally;
      sceneReal = false;
      sceneKnown = true;
    } else {
      sceneFailed = true;
    }
  }
  renderScene();
}

// Callers fire this right after summoning; the overlay needs a moment to
// register the new character, and rapid calls collapse into one.
const refreshActive = debounce(loadScene, 350);

function renderScene() {
  const total = scene.reduce((a, [, c]) => a + c, 0);
  if (sceneKnown) {
    const text = total ? `${total} ${sceneReal ? "on screen" : "summoned this session"}` : "";
    note($("active-note"), sceneStale ? `${text}${text ? " · " : ""}could not check just now` : text);
  }
  $("save-preset").disabled = !total;

  if (!sceneKnown) {
    if (sceneFailed) {
      note($("active-note"), "Could not check who is on screen", "error");
      return $("active-list").replaceChildren(retryBox("The overlay did not answer", loadScene));
    }
    return skeletons($("active-list"), 3, "s-tile");
  }

  const frag = document.createDocumentFragment();
  if (!scene.length) {
    const empty = document.createElement("div");
    empty.className = "scene-empty";
    empty.innerHTML =
      '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2.5"/><path d="M8.5 20h7M12 16v4"/></svg>' +
      "<div>Nobody on screen</div>";
    // The overlay is off when nobody is there. If people were on screen before, one
    // click puts them back.
    const before = overlay.seen.reduce((a, [, c]) => a + c, 0);
    if (before) {
      const back = el("button", "ghost", `Bring back the last ${before}`);
      back.title = "Summon the characters that were on screen the last time";
      back.addEventListener("click", () => withBusy(back, async () => (await overlay.bringBack(), refreshActive())));
      empty.appendChild(back);
    }
    frag.appendChild(empty);
  }

  for (const [name, count] of scene) {
    const tile = document.createElement("div");
    tile.className = "tile";
    tile.tabIndex = 0;
    tile.setAttribute("role", "button");
    tile.title = `${pretty(name)}: click to summon another`;

    const meta = mine.find((m) => m.name === name);
    const stage = document.createElement("div");
    stage.className = "tile-stage";
    if (meta && meta.sprite_path) {
      const img = sprite(convertFileSrc(meta.sprite_path), name);
      stage.appendChild(img);
      attachHover(tile, img, () => localFrames(name));
    } else stage.appendChild(placeholder(name));
    tile.appendChild(stage);

    const label = document.createElement("div");
    label.className = "tile-name";
    label.textContent = pretty(name);
    tile.appendChild(label);

    const cnt = document.createElement("span");
    cnt.className = "tile-count";
    cnt.textContent = `×${count}`;
    tile.appendChild(cnt);

    const summon = () =>
      withBusy(
        tile,
        async () => {
          if (await summonOne(name)) refreshActive();
        },
        { card: true }
      );
    tile.addEventListener("click", summon);
    tile.addEventListener("keydown", (e) => {
      if (e.target === tile && (e.key === "Enter" || e.key === " ")) {
        e.preventDefault();
        summon();
      }
    });

    // ✕ removes this character from the screen (every copy of it).
    if (sceneReal) {
      const x = document.createElement("button");
      x.className = "tile-x";
      x.textContent = "✕";
      x.title = count > 1 ? `Dismiss ${pretty(name)} (all ${count})` : `Dismiss ${pretty(name)}`;
      x.setAttribute("aria-label", x.title);
      x.addEventListener("click", async (e) => {
        e.stopPropagation();
        // Gone from the list at once; the overlay plays a dismiss animation, so
        // the real state is read again a moment later.
        scene = scene.filter(([n]) => n !== name);
        if (!scene.length) overlay.quiet();
        renderScene();
        try {
          await invoke("dismiss_character", { name, all: true });
        } catch (err) {
          toast(String(err), "error");
        }
        setTimeout(loadScene, 1300);
      });
      tile.appendChild(x);
    }
    frag.appendChild(tile);
  }
  $("active-list").replaceChildren(frag);
}

$("random-summon").addEventListener("click", () =>
  withBusy($("random-summon"), async () => {
    // Helper prototypes (.Hornet_Needle and the like) are never summoned on their own.
    const pool = mine.filter((m) => !m.name.startsWith("."));
    if (!pool.length) return toast("Your collection is empty", "error");
    const pick = pool[Math.floor(Math.random() * pool.length)];
    if (await summonOne(pick.name)) {
      toast(`Summoned ${pretty(pick.name)}`, "ok");
      refreshActive();
    }
  })
);

// One button does both jobs: removes everyone from the screen and clears the list below.
$("dismiss-all").addEventListener("click", () =>
  withBusy($("dismiss-all"), async () => {
    try {
      // Pressed in the middle of summoning a crowd, this means "stop, I have changed my
      // mind" — so the summon is called off first instead of being waited for.
      const stopped = await invoke("cancel_summon").catch(() => false);
      overlay.quiet();
      scene = [];
      sceneKnown = true;
      renderScene();
      if (stopped) toast("Summoning stopped", "ok");
      await invoke("dismiss_all");
      toast("Everyone dismissed", "ok");
      setTimeout(loadScene, 1500);
      setTimeout(refreshHealth, 1500);
    } catch (e) {
      toast(String(e), "error");
    }
  })
);

// --- Presets ---

let presetSig = null; // what the preset cards were last drawn from

async function refreshPresets() {
  let presets = [];
  try {
    presets = await invoke("list_presets");
  } catch (e) {
    toast(String(e), "error");
    return;
  }

  // Drawn again only when something they show has changed, so coming back to the
  // Scene does not rebuild them (or push the list below them around).
  const sig = JSON.stringify([presets, presets.flatMap((p) => p.members.map(([n]) => mine.find((m) => m.name === n)?.sprite_path || ""))]);
  if (sig === presetSig) return;
  presetSig = sig;

  const box = $("presets");
  box.classList.toggle("hidden", !presets.length);
  // "On screen" only needs a title when something else sits above it.
  $("active-title").classList.toggle("hidden", !presets.length);

  const frag = document.createDocumentFragment();
  if (presets.length) frag.appendChild(el("div", "section-title", "Presets"));
  const grid = el("div", "preset-grid");

  for (const p of presets) {
    const total = p.members.reduce((a, [, c]) => a + c, 0);
    const card = el("div", "preset-card");
    card.tabIndex = 0;
    card.setAttribute("role", "button");
    card.title = "Summon: " + p.members.map(([n, c]) => (c > 1 ? `${pretty(n)} ×${c}` : pretty(n))).join(", ");

    // The faces of who is in it, overlapping, so a preset is recognised at a glance.
    const faces = el("div", "avatars");
    for (const [n] of p.members.slice(0, 3)) {
      const meta = mine.find((m) => m.name === n);
      const av = el("span", "av");
      av.appendChild(meta?.sprite_path ? sprite(convertFileSrc(meta.sprite_path), n) : placeholder(n));
      faces.appendChild(av);
    }
    if (p.members.length > 3) faces.appendChild(el("span", "av more", `+${p.members.length - 3}`));
    card.appendChild(faces);

    const text = el("div", "preset-text");
    text.appendChild(el("div", "preset-name", p.name));
    text.appendChild(el("div", "preset-sub", plural(total, "character")));
    card.appendChild(text);
    card.appendChild(el("span", "preset-go", "▶"));

    const run = async () => {
      if (!(await confirmCrowd(total))) return;
      return withBusy(
        card,
        () =>
          inOrder(async () => {
            try {
              toast(await invoke("run_preset", { name: p.name }), "ok");
              refreshActive();
              refreshHealth();
            } catch (e) {
              summonError(e);
            }
          }),
        { card: true },
      );
    };
    card.addEventListener("click", run);
    card.addEventListener("keydown", (e) => {
      if (e.target === card && (e.key === "Enter" || e.key === " ")) {
        e.preventDefault();
        run();
      }
    });

    const del = el("button", "preset-del", "✕");
    del.type = "button";
    del.title = "Delete this preset";
    del.addEventListener("click", async (e) => {
      e.stopPropagation();
      if (!(await confirmDialog(`Delete the preset “${p.name}”?`))) return;
      await invoke("delete_preset", { name: p.name }).catch((err) => toast(String(err), "error"));
      refreshPresets();
    });
    card.appendChild(del);
    grid.appendChild(card);
  }
  frag.appendChild(grid);
  box.replaceChildren(frag);
}

// Saves whoever is on screen right now as a preset.
$("save-preset").addEventListener("click", async () => {
  const members = scene;
  if (!members.length) return toast("Summon someone first", "error");

  const total = members.reduce((a, [, c]) => a + c, 0);
  const name = await promptDialog(`Name this preset (${plural(total, "character")})`, "e.g. Work desk", "Save");
  if (!name) return;
  await withBusy($("save-preset"), async () => {
    try {
      await invoke("save_preset", { name, members });
      toast(`Preset “${name}” saved`, "ok");
      await refreshPresets();
    } catch (e) {
      toast(String(e), "error");
    }
  });
});

// ===========================================================================
// wl_shimeji settings
// ===========================================================================

const PLUGIN_GROUP = "Window interaction (needs a plugin)";
const INPUT_GROUP = "Mouse and stylus";

// wl_shimeji gives a character three click actions and lets you choose which physical
// button does which: 1 = the one that picks it up and drags it, 2 = the one that opens
// what the character does on a right click, 4 = its middle-click action. The values are
// the engine's own (POINTER_PRIMARY/SECONDARY/THIRD_BUTTON in config.h).
const CLICK_ROLES = [
  ["1", "Drag"],
  ["2", "Right-click"],
  ["4", "Middle-click"],
];

/** "ON_TOOL_PEN" → "Pen", "POINTER_MIDDLE_BUTTON" → "Middle button". */
function inputName(key) {
  const words = key.toLowerCase().replace(/^on_tool_/, "").replace(/^pointer_/, "").split("_");
  const text = words.join(" ").replace(/button(\d)/, "button $1");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

const OPTIONS = {
  BREEDING: { group: "Characters", name: "Breeding", desc: "Characters can duplicate themselves", type: "bool" },
  DRAGGING: { group: "Characters", name: "Dragging", desc: "Pick characters up with the mouse", type: "bool" },
  DISMISS_ANIMATIONS: { group: "Characters", name: "Dismiss animation", desc: "Play an animation when a character leaves", type: "bool" },
  AFFORDANCES: { group: "Characters", name: "Character interactions", desc: "Characters can act on each other", type: "bool" },
  // Checked in wl_shimeji's source: the limit is read in one place, the breeding
  // action, and nothing looks at it when a character is summoned. Calling it a limit on
  // what is on screen would be a plain untruth — 839 summoned by hand is 839 on screen.
  MASCOT_LIMIT: {
    group: "Characters", name: "Breeding limit", desc: "Characters stop copying themselves once this many are on screen. Summoning is never limited.",
    type: "range", min: 1, max: 512, step: 1,
  },

  // The ranges are wl_shimeji's own: it quietly clamps a scale above 2 or below 0.25,
  // and an opacity outside 0..1. A slider that offers more than that only produces
  // values the engine throws away.
  //
  // MASCOT_SCALE is a divisor, not a multiplier: the engine draws a character at
  // `sprite / scale`, so its 2 is the *smallest* and its 0.25 the biggest. Nobody
  // reading "Size: x2" expects half-size characters, so the slider works in sizes —
  // 0.5x to 4x — and the reciprocal is what goes to the engine.
  MASCOT_SCALE: {
    group: "Appearance", name: "Size", desc: "Takes effect on each character the next time it moves",
    // The engine's -1 means "leave it at normal size": that is x1, not "x-1".
    // `log`: on a straight track 1x sat a seventh of the way in, next to the smallest size, though it is the size
    // nearly everybody wants; on a log track (each step is the same ratio) it is a third of the way in.
    type: "range", min: 0.5, max: 4, step: 0.1, float: true, unit: "x", invert: true, log: true,
    dflt: "-1", dfltAt: 1, dfltLabel: "x1", noReset: true,
  },
  OPACITY: {
    group: "Appearance", name: "See-through", desc: "How solid the characters look",
    // The engine's -1 means "leave it alone", which is the same as fully solid: it reads as
    // 100% and needs no link of its own to go back to.
    type: "range", min: 0, max: 1, step: 0.05, float: true, percent: true, dflt: "-1", dfltAt: 1, dfltLabel: "100%", noReset: true,
    needs: "alpha_modifier", needsName: "wp_alpha_modifier_v1",
  },
  INTERPOLATION_FRAMERATE: {
    group: "Appearance", name: "Motion smoothing", desc: "Extra frames between the drawn ones",
    type: "range", min: -1, max: 240, step: 1, words: { "-1": "Monitor", "0": "Off" }, suffix: " fps",
  },
  WLR_SHELL_LAYER: {
    group: "Appearance", name: "Layer", desc: "Which windows the characters are drawn above", type: "select",
    options: [["background", "Background"], ["bottom", "Bottom"], ["top", "Top"], ["overlay", "Overlay"]],
  },

  ALLOW_THROWING_MULTIHEAD: { group: "Multiple monitors", name: "Throw across monitors", desc: "Characters can be thrown onto the next screen", type: "bool" },
  ALLOW_DRAGGING_MULTIHEAD: { group: "Multiple monitors", name: "Drag across monitors", desc: "Characters can be dragged onto the next screen", type: "bool" },
  UNIFIED_OUTPUTS: { group: "Multiple monitors", name: "One shared desktop", desc: "Treat all monitors as a single space", type: "bool" },

  TABLETS_ENABLED: { group: "Devices", name: "Graphics tablet", desc: "Treat a stylus as a pointer", type: "bool", unused: true },

  // These four depend on a compositor plugin (see the notice under the group title).
  WINDOW_INTERACTIONS: { group: PLUGIN_GROUP, name: "Interact with windows", desc: "Characters can walk on and climb other windows", type: "bool", unused: true },
  WINDOW_THROWING: { group: PLUGIN_GROUP, name: "Throw windows", desc: "Characters can throw windows around", type: "bool" },
  WINDOW_THROW_POLICY: {
    group: PLUGIN_GROUP, name: "Window throw policy", desc: "What happens to a thrown window", type: "select", unused: true,
    options: [["looping", "looping"], ["bounce", "bounce"], ["stop", "stop"]],
  },
  CURSOR_POSITION: { group: PLUGIN_GROUP, name: "Global cursor position", desc: "Characters react to the pointer anywhere on screen", type: "bool", unused: true },
};

const ADVANCED = (k) => k.startsWith("POINTER_") || k.startsWith("ON_TOOL_");

/** A description for one of the pointer or stylus keys, built from its name. */
function inputMeta(key) {
  if (!ADVANCED(key)) return null;
  const tool = key.startsWith("ON_TOOL_");
  return {
    group: INPUT_GROUP,
    name: tool ? `Stylus: ${inputName(key).toLowerCase()}` : `Mouse: ${inputName(key).toLowerCase()}`,
    desc: tool ? "What this tool does to a character" : "What this button does to a character",
    type: "select",
    options: CLICK_ROLES,
  };
}
const isTrue = (v) => ["true", "1", "yes", "on"].includes(String(v).trim().toLowerCase());

function tidy(v) {
  const n = parseFloat(v);
  return Number.isNaN(n) ? v : String(parseFloat(n.toFixed(2)));
}

/** Explains what the window-interaction switches need in order to do anything. */
function pluginNotice(status) {
  const box = document.createElement("div");
  box.className = "panel-note";

  const text = document.createElement("div");
  text.className = "grow";
  if (status.plugins.length) {
    text.textContent = `Plugin found: ${status.plugins.join(", ")}.`;
    box.appendChild(text);
    return box;
  }

  const desktop = status.desktop || "your desktop";
  text.textContent = `These do nothing without a compositor plugin. The only official one is for KDE KWin; there is none for ${desktop} yet.`;
  box.appendChild(text);

  for (const [label, url] of [
    ["Plugin docs", "https://github.com/CluelessCatBurger/wl_shimeji#plugins"],
    ["KWin plugin (AUR)", "https://aur.archlinux.org/packages/wl_shimeji-plugin-kwinsupport"],
  ]) {
    const link = document.createElement("button");
    link.className = "link-btn";
    link.textContent = `${label} ↗`;
    link.addEventListener("click", () => openExternal(url));
    box.appendChild(link);
  }
  return box;
}

const cfgLoad = latest();

async function loadConfig(showAdvanced = false) {
  const ticket = cfgLoad.next();
  const box = document.createElement("div");
  box.className = "opts";
  note($("opts-note"), "");
  // The first time there is nothing on the page yet: placeholders instead of blank.
  if (!$("opts").children.length) skeletons($("opts"), 3, "s-panel");

  let opts;
  try {
    opts = await invoke("config_list");
  } catch (e) {
    if (!cfgLoad.isCurrent(ticket)) return;
    note($("opts-note"), String(e), "error");
    if ($("opts").querySelector(".skel")) $("opts").replaceChildren(retryBox("Could not read the settings", () => loadConfig(showAdvanced)));
    return;
  }
  if (!cfgLoad.isCurrent(ticket)) return;

  if (!opts.length) {
    note($("opts-note"), "Could not read the settings. Try again.", "error");
    if ($("opts").querySelector(".skel")) $("opts").replaceChildren(retryBox("Could not read the settings", () => loadConfig(showAdvanced)));
    return;
  }
  note($("opts-note"), "");

  // Overlay not running: shimejictl returns the values saved in the config file.
  // They can still be changed; the changes apply at the next start.
  const offline = opts.some((o) => o.live === false);
  const banner = $("opts-banner");
  banner.classList.toggle("hidden", !offline);
  banner.innerHTML = "";
  if (offline) {
    const t = document.createElement("div");
    t.className = "grow";
    t.textContent = "The overlay is stopped. Changes apply when it starts.";
    const re = document.createElement("button");
    re.className = "link-btn";
    re.textContent = "Check again";
    re.addEventListener("click", () => loadConfig(showAdvanced));
    banner.append(t, re);
  }

  let plugins = { desktop: "", plugins: [] };
  try {
    plugins = await invoke("plugin_status");
  } catch {}
  // Some settings need a Wayland protocol the compositor may simply not have. Asked
  // once and remembered: the answer cannot change while the session lasts.
  if (!protocols) protocols = (await invoke("environment_check").catch(() => null))?.protocols || {};
  if (!cfgLoad.isCurrent(ticket)) return;

  const known = Object.keys(OPTIONS);
  opts.sort((a, b) => {
    const ia = known.indexOf(a.key), ib = known.indexOf(b.key);
    return (ia < 0 ? 999 : ia) - (ib < 0 ? 999 : ib);
  });

  let hidden = 0;
  let lastGroup = null;
  let panel = null;

  for (const opt of opts) {
    const meta = OPTIONS[opt.key] || inputMeta(opt.key);
    // Checked against wl_shimeji's source: it stores these and reads them nowhere.
    if (meta && meta.unused) continue;
    if (!showAdvanced && ADVANCED(opt.key)) {
      hidden++;
      continue;
    }

    // One card per group: the settings are already sorted so groups are contiguous.
    const group = meta ? meta.group : "Other";
    if (group !== lastGroup) {
      panel = document.createElement("section");
      panel.className = "panel";
      const head = document.createElement("div");
      head.className = "panel-head";
      head.appendChild(el("div", "panel-title", group));
      panel.appendChild(head);
      if (group === INPUT_GROUP) {
        panel.appendChild(
          el(
            "div",
            "panel-note",
            "A character has three click actions. These say which button or stylus tool does which — a left-handed mouse, or a stylus whose eraser should pick characters up rather than open their menu. The stylus rows only matter if you have a graphics tablet.",
          ),
        );
      }
      if (group === PLUGIN_GROUP) {
        panel.appendChild(pluginNotice(plugins));
        // Without a plugin these do nothing, so they read as unavailable.
        if (!plugins.plugins.length) panel.classList.add("locked");
      }
      box.appendChild(panel);
      lastGroup = group;
    }

    const row = document.createElement("div");
    row.className = "opt";

    const text = document.createElement("div");
    text.className = "opt-text";
    text.appendChild(el("div", "opt-name", meta ? meta.name : opt.label || opt.key));
    // A setting that cannot do anything says so in place of its description and cannot be
    // touched. Two reasons for that: the compositor has no protocol for it, or wl_shimeji
    // stores the value and never reads it (checked in its source; four of them do that).
    const noProtocol = meta && meta.needs && protocols && protocols[meta.needs] === false;
    const unsupported = noProtocol || (meta && meta.unused);
    const why = noProtocol
      ? `Not available here: your compositor does not offer ${meta.needsName}`
      : `${meta ? meta.desc : ""} — but wl_shimeji keeps this value without ever using it, so it changes nothing`;
    text.appendChild(el("div", "opt-desc", unsupported ? why : meta ? meta.desc : opt.key));
    row.appendChild(text);
    if (unsupported) row.classList.add("unavailable");

    const ctl = document.createElement("div");
    ctl.className = "opt-ctl";
    row.appendChild(ctl);

    // Resolves to what the overlay holds afterwards ({value, adjusted}), or null when
    // it could not be saved at all, so a control can put itself back.
    const apply = async (v, quiet = false) => {
      try {
        const outcome = (await invoke("config_set", { key: opt.key, value: String(v) })) || { value: String(v), adjusted: false };
        if (!quiet && !outcome.adjusted) toast(`${meta ? meta.name : opt.key}: ${v}`, "ok");
        return outcome;
      } catch (e) {
        toast(String(e), "error");
        return null;
      }
    };

    const boolish = ["true", "false"].includes(String(opt.value).trim().toLowerCase());

    if ((meta && meta.type === "bool") || (!meta && boolish)) {
      const sw = document.createElement("button");
      sw.type = "button";
      sw.className = `sw ${isTrue(opt.value) ? "on" : ""}`;
      sw.setAttribute("role", "switch");
      sw.setAttribute("aria-checked", String(isTrue(opt.value)));
      sw.setAttribute("aria-label", meta ? meta.name : opt.key);
      const show = (on) => {
        sw.classList.toggle("on", on);
        sw.setAttribute("aria-checked", String(on));
      };
      sw.addEventListener("click", async () => {
        const on = !sw.classList.contains("on");
        show(on);
        // It goes back if saving failed.
        const outcome = await apply(on);
        if (!outcome) show(!on);
        else if (outcome.adjusted) show(isTrue(outcome.value));
      });
      ctl.appendChild(sw);
    } else if (meta && meta.type === "range") {
      const isDefault = (v) => meta.dflt !== undefined && (
        String(v).trim() === meta.dflt ||
        (!isNaN(parseFloat(v)) && !isNaN(parseFloat(meta.dflt)) && parseFloat(v) === parseFloat(meta.dflt))
      );
      // What the slider shows ← what the engine holds, and back again.
      const fromEngine = (v) => (meta.invert ? 1 / parseFloat(v) : parseFloat(v));
      const toEngine = (v) => (meta.invert ? 1 / parseFloat(v) : parseFloat(v));
      // "1.35" reads as "x1.35" or "70%"; the engine's "-1" means "whatever it ships with".
      const show = (v) => {
        if (isDefault(v)) return meta.dfltLabel || "Default";
        const word = meta.words && meta.words[String(parseFloat(v))];
        if (word) return word;
        const n = fromEngine(v);
        if (!isFinite(n)) return tidy(v);
        if (meta.percent) return `${Math.round(n * 100)}%`;
        // 2 reads as "x2", 1.5 as "x1.5": no trailing zeros to look like false precision.
        // A size is one decimal: it comes back from the engine as 1/x and 3.0303 is not a size.
        if (meta.unit === "x") return `x${parseFloat(n.toFixed(meta.invert ? 1 : 2))}`;
        return (meta.float ? parseFloat(n.toFixed(2)) : Math.round(n)) + (meta.suffix || "");
      };

      const val = el("span", "opt-val", show(opt.value));
      const sl = document.createElement("input");
      sl.type = "range";
      // The track's own numbers (`pos`) are the setting's, or their logarithm; `unpos` gives the setting back, to a tenth.
      const pos = (n) => (meta.log ? Math.log(n) : n);
      const unpos = (p) => (meta.log ? Math.round(Math.exp(parseFloat(p)) * 10) / 10 : parseFloat(p));
      sl.min = pos(meta.min);
      sl.max = pos(meta.max);
      sl.step = meta.log ? 0.005 : meta.step || 1;
      // A value the engine calls "default" has no place on the track, so the thumb
      // sits where that default actually is.
      const place = (v) => pos(isDefault(v) ? meta.dfltAt : Math.min(meta.max, Math.max(meta.min, fromEngine(v))));
      sl.value = place(opt.value);
      // The filled part of the track follows the thumb.
      const fill = () => sl.style.setProperty("--p", `${((sl.value - sl.min) / (sl.max - sl.min)) * 100}%`);
      fill();
      sl.setAttribute("aria-label", meta.name);

      // Only where a setting has a "leave it to wl_shimeji" value of its own, and only
      // when there is something to go back from.
      const reset = meta.dflt && !meta.noReset ? el("button", "link-btn tiny", "Default") : null;
      if (reset) reset.title = "Back to what wl_shimeji uses by default";
      // Hidden but still taking its room, so the slider does not move when it comes and goes.
      const showReset = (v) => reset && reset.classList.toggle("invisible", unsupported || isDefault(v));
      showReset(opt.value);

      // Whatever the engine ended up with is what the control shows: it clamps values
      // of its own accord, and a control that disagrees with it is a lie.
      const settle = (outcome, asked) => {
        if (!outcome) return;
        val.textContent = show(outcome.value);
        sl.value = place(outcome.value);
        fill();
        showReset(outcome.value);
        if (outcome.adjusted) {
          toast(`${meta.name}: wl_shimeji kept ${show(outcome.value)}`, "", { ms: 3200 });
        } else {
          toast(`${meta.name}: ${show(asked)}`, "ok");
        }
      };

      // While dragging there is no round trip, so the slider's own number is shown as it is.
      sl.addEventListener("input", () => {
        const n = unpos(sl.value);
        val.textContent = meta.percent ? `${Math.round(n * 100)}%` : meta.unit === "x" ? `x${parseFloat(n.toFixed(1))}` : show(toEngine(n));
        fill();
        showReset(String(toEngine(n)));
      });
      sl.addEventListener("change", async () => {
        const wanted = toEngine(unpos(sl.value));
        // Three decimals where the value is a reciprocal: 1/0.33 is 3.03, 1/0.333 is 3.
        const asked = meta.float ? wanted.toFixed(meta.invert ? 3 : 2) : String(Math.round(wanted));
        settle(await apply(asked), asked);
      });
      if (reset) reset.addEventListener("click", async () => settle(await apply(meta.dflt), meta.dflt));
      ctl.append(sl, val, ...(reset ? [reset] : []));
    } else if (meta && meta.type === "select" && offline && opt.key === "WINDOW_THROW_POLICY") {
      // Without the overlay the number in the file does not map to a name, so leave it alone.
      const v = document.createElement("span");
      v.className = "opt-val wide";
      v.textContent = "only editable while the overlay is running";
      ctl.appendChild(v);
    } else if (meta && meta.type === "select") {
      // A few short choices: segments instead of a drop-down, so everything is visible at once.
      const seg = document.createElement("div");
      seg.className = "seg";
      const current = String(opt.value).trim();
      for (const [value, label] of meta.options) {
        const b = document.createElement("button");
        b.className = `seg-btn ${value === current ? "active" : ""}`;
        b.textContent = label;
        b.addEventListener("click", async () => {
          if (b.classList.contains("active")) return;
          const before = seg.querySelector(".seg-btn.active");
          seg.querySelectorAll(".seg-btn").forEach((x) => x.classList.toggle("active", x === b));
          const outcome = await apply(value);
          if (!outcome) {
            seg.querySelectorAll(".seg-btn").forEach((x) => x.classList.toggle("active", x === before));
          } else if (outcome.adjusted) {
            const held = String(outcome.value).trim().toLowerCase();
            const back = [...seg.querySelectorAll(".seg-btn")].find((x, i) => meta.options[i][0].toLowerCase() === held) || before;
            seg.querySelectorAll(".seg-btn").forEach((x) => x.classList.toggle("active", x === back));
            toast(`${meta.name}: wl_shimeji kept ${back ? back.textContent : outcome.value}`, "", { ms: 3200 });
          }
        });
        seg.appendChild(b);
      }
      ctl.appendChild(seg);
    } else {
      const inp = document.createElement("input");
      inp.className = "field";
      inp.value = opt.value;
      inp.addEventListener("change", () => apply(inp.value));
      ctl.appendChild(inp);
    }

    panel.appendChild(row);
  }

  if (hidden) {
    const more = document.createElement("button");
    more.className = "ghost opts-more";
    more.textContent = `Mouse and stylus buttons (${hidden})`;
    more.addEventListener("click", () => loadConfig(true));
    box.appendChild(more);
  }
  $("opts").replaceChildren(...box.childNodes);
}

// ===========================================================================
// Install from a local .zip
// ===========================================================================

/**
 * Without an argument it asks for a file; cachomon.js passes the path of an
 * archive it found in the Downloads folder.
 */
/** Who is on screen this moment, by normalised name (the Scene page's last copy if the overlay cannot be asked). */
async function onScreenNow() {
  try {
    return new Set((await invoke("on_screen")).map(([n]) => norm(n)));
  } catch {
    return new Set(scene.map(([n]) => norm(n)));
  }
}

async function installLocalArchive(given, btn) {
  if (btn?.classList.contains("wait")) return; // already reading an archive
  const path =
    given ||
    (await dlg.open({
      multiple: false,
      // The chooser opens in Downloads, which is where the archive most likely is.
      defaultPath: await invoke("downloads_path").catch(() => undefined),
      filters: [{ name: "Shimeji", extensions: ["zip"] }],
    }));
  if (!path) return;

  try {
    // `btn` (the button that asked) spins while the archive is read.
    const names = await withBusy(btn, () => invoke("prepare_local_archive", { path }));
    localMode = true;
    stopPlayer();
    $("player-wrap").classList.add("hidden");
    $("m-name").textContent = path.split("/").pop();
    $("m-meta").textContent = "";
    $("m-sprite").innerHTML = "";

    const have = haveNames();
    const allHave = names.length > 0 && names.every((n) => have.has(norm(n)));
    note($("m-note"), allHave ? "All of these are already in your collection. Tick one to replace it." : "");
    renderPicks(names);

    $("m-install").onclick = async () => {
      let selection = picked();
      // Replacing a character that is on screen makes the overlay reload it under its own mascots, and the
      // overlay does not always survive that: the ones on screen are left as they are.
      // Asked of the overlay now, not read from the Scene page's copy, which is only as fresh as the last visit there.
      const onScreen = await onScreenNow();
      const live = selection.filter((n) => haveNames().has(norm(n)) && onScreen.has(norm(n)));
      let leftAlone = "";
      if (live.length) {
        selection = selection.filter((n) => !live.includes(n));
        leftAlone = `Left ${plural(live.length, "character")} as ${live.length === 1 ? "it is" : "they are"}: on screen now. Dismiss ${live.length === 1 ? "it" : "them"} first to replace.`;
      }
      if (!selection.length) {
        if (leftAlone) toast(leftAlone, "error");
        return;
      }
      // Progress continues in the bottom-right panel; do not cancel the conversion.
      localMode = false;
      closeSheet();
      try {
        let said = await invoke("install_local_selected", { selection });
        if (leftAlone) said += `\n${leftAlone}`;
        await repairAfterInstall();
        await loadMine();
        // One message, said once everything is done: a second toast replaces the first within a blink, and a
        // .zip that vanishes from Downloads with no word about it is a mystery to whoever has not read the setting.
        // More than one line in `said` means some of it did not go in, and the lines say which.
        const trashed = await afterArchiveInstalled(path, names);
        toast(trashed ? `${said}\n${trashed}` : said, /\n/.test(said) ? "error" : "ok");
      } catch (e) {
        toast(String(e), "error");
      }
    };
    $("modal").classList.remove("hidden");
  } catch (e) {
    toast(String(e), "error");
  }
}

/**
 * With "Move the archive to the Trash after installing" on, an archive from the
 * Downloads folder is trashed once everything in it is in the collection. (An
 * archive with characters you left out is kept: it is still wanted.) The backend
 * refuses anything that is not a .zip in Downloads, and it is the Trash, not a delete.
 */
async function afterArchiveInstalled(path, names) {
  if (!path || store.get("cacho-trash", "1") !== "1") return "";
  const have = haveNames();
  if (!names.every((n) => have.has(norm(n)))) return "";
  try {
    await invoke("trash_archive", { path });
    syncCachomon();
    return "The .zip was moved to the Trash, as set in the cachomon options. You can restore it from there.";
  } catch (e) {
    // Outside Downloads the file simply stays; that is not worth a message.
    return String(e).startsWith("Only .zip files") ? "" : `The .zip could not be moved to the Trash: ${e}`;
  }
}

// What the pill says, and what to do when the overlay disappears, is overlay.js's job.
// ===========================================================================

// What the pill says, and what to do when the overlay disappears, is overlay.js's job.
const refreshHealth = () => overlay.refresh();

// ===========================================================================
// Startup
// ===========================================================================

const goto = (view) => document.querySelector(`.nav[data-view="${view}"]`).click();

// "Summon" on a finished row of the install panel. The row carries the catalog
// name; the installed prototype may be named a little differently.
initDock({
  summon: async (name) => {
    const hit = mine.find((m) => norm(m.name) === norm(name));
    if (!(await summonOne(hit ? hit.name : name))) return false;
    refreshActive();
    return true;
  },
  openCollection: (names) => showInCollection(names),
});

initPalette({
  items: () => {
    const cmd = (title, kind, run, keywords = "") => ({ title, kind, run, keywords });
    const commands = [
      cmd("Dismiss all", "scene", () => $("dismiss-all").click(), "remove clear everyone"),
      cmd("Summon random", "scene", () => $("random-summon").click(), "random"),
      cmd("Go to Catalog", "go to", () => goto("catalog")),
      cmd("Go to Collection", "go to", () => goto("mine")),
      cmd("Go to Scene", "go to", () => goto("scene")),
      cmd("Go to Settings", "go to", () => goto("settings")),
      cmd("Go to cachomon.com catalog", "go to", () => {
        goto("catalog");
        document.querySelector('#source-tabs [data-source="cacho"]').click();
      }, "cachomon"),
      cmd("Go to shimejis.xyz catalog", "go to", () => {
        goto("catalog");
        document.querySelector('#source-tabs [data-source="xyz"]').click();
      }),
      cmd("Install from a .zip file…", "command", () => installLocalArchive(null), "install zip archive"),
    ];
    const chars = mine
      .filter((m) => !m.name.startsWith("."))
      .map((m) => ({
        title: pretty(m.name),
        sub: m.pack_title || "",
        kind: "summon",
        keywords: `${m.name} ${m.pack_title || ""} summon`,
        run: async () => {
          if (await summonOne(m.name)) {
            toast(pretty(m.name), "ok");
            refreshActive();
          }
        },
      }));
    return [...commands, ...chars];
  },
});

document.addEventListener("keydown", (e) => {
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || "");
  const modalOpen = !!document.querySelector(".modal:not(.hidden)");

  if (e.key === "Escape" && !modalOpen && !typing) {
    mineSel.clear();
    catSel.clear();
    return;
  }
  if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "a" && !typing && !modalOpen) {
    if ($("view-mine").classList.contains("active")) {
      e.preventDefault();
      mineSel.selectAll();
    } else if ($("view-catalog").classList.contains("active") && shownChars.length && !$("chars").classList.contains("hidden")) {
      e.preventDefault();
      catSel.selectAll();
    }
    return;
  }

  if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
  if (e.key.toLowerCase() === "k") {
    e.preventDefault();
    openPalette();
  } else if (["1", "2", "3", "4"].includes(e.key)) {
    e.preventDefault();
    goto(["catalog", "mine", "scene", "settings"][Number(e.key) - 1]);
  }
});

initCachomon({
  installArchive: installLocalArchive,
  showInCollection: (names) => showInCollection(names),
  mine: () => mine,
  // Stay out of the way while the local-archive dialog is open.
  canAutoInstall: () => !localMode,
  autoInstall: async (path, name) => {
    try {
      const names = await invoke("prepare_local_archive", { path });
      // Leave what is already installed alone; if the whole archive is installed, quietly stop.
      const have = haveNames();
      const fresh = names.filter((n) => !have.has(norm(n)));
      if (!fresh.length) return invoke("cancel_local");
      // The install panel shows it under way; the one message comes when it is over.
      const said = await invoke("install_local_selected", { selection: fresh });
      await repairAfterInstall();
      await loadMine();
      const trashed = await afterArchiveInstalled(path, names);
      toast(trashed ? `${said}\n${trashed}` : said, /\n/.test(said) ? "error" : "ok");
    } catch (e) {
      toast(`«${name}»: ${e}`, "error");
    }
  },
});
initStartup({ mine: () => mine, loaded: () => mineLoaded });

// Remember where we were last time.
if (store.get("catalog-source") === "cacho") {
  document.querySelector('#source-tabs [data-source="cacho"]').click();
}
const lastTab = store.get("settings-tab");
if (lastTab && lastTab !== "general") {
  document.querySelector(`#settings-tabs [data-tab="${lastTab}"]`)?.click();
}

refreshHealth();
checkEnvironment(refreshHealth); // engine missing, or a desktop that cannot run it: say so once
// An archive dropped on the window goes through the same flow as one picked by hand.
initDragDrop((path) => installLocalArchive(path, null));
// Every few seconds: is the overlay alive? And every twenty, who is on screen, so
// that if it disappears there is a list of who to bring back (people also arrive
// from the login file and from key bindings, which this app does not see).
let tick = 0;
setInterval(() => {
  if (document.hidden) return;
  refreshHealth();
  if (++tick % 4 === 0 && overlay.up) loadScene();
}, 5000);
window.addEventListener("focus", refreshHealth);

loadPacks();
renderMine(); // the saved list from last time, or placeholders until the real one arrives
loadMine();
refreshPresets(); // ready before you open the Scene, so nothing shifts when you do
loadConfig();
