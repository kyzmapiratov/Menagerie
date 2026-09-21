// Startup tab: start Shimeji when you log in to Niri or Hyprland, and bind keys.
//
// One switch does the work. "Turn on" writes our own file and adds a single
// `include` line to config.kdl (backup kept, checked with `niri validate`, rolled
// back if Niri objects). While it is on, every change here is written straight
// away, and Niri reloads its config by itself, so there is no Save button.
//
// Niri does not complain when a key is bound twice: the later bind silently
// wins. Since ours would come last, a key that the user's own config already
// uses is left out and flagged, never overwritten.
//
// Niri and Hyprland each get their own file with key bindings in it (niri_* and hyprland_*
// commands, same shape). Every other desktop, KDE included, gets an XDG autostart entry and the
// commands to bind by hand in its own shortcut settings.

import { $, el, invoke, toast, store, sprite, placeholder, convertFileSrc, pretty, clip, debounce, confirmDialog, withBusy } from "./util.js";
import { attachHover, localFrames } from "./anim.js";

const KEY = "startup-options";

// Keys that are free in a stock Niri config. (Mod+Shift+S/H are taken by many
// setups, so they are not offered.)
const DEFAULTS = {
  overlay: true,
  delay: 3,
  random: 0, // how many characters, chosen at random from the collection, to add at login
  binds: [
    { on: true, keys: "Mod+Ctrl+D", action: "dismiss_all", arg: "" },
    { on: true, keys: "Mod+Ctrl+X", action: "stop", arg: "" },
    { on: false, keys: "Mod+Ctrl+H", action: "summon", arg: "" },
  ],
};

const LABEL = { dismiss_all: "Dismiss all characters", stop: "Stop the overlay", summon: "Summon" };

let desktop = ""; // XDG_CURRENT_DESKTOP, for wording only
let kde = false; // KDE Plasma: its shortcuts are set in System Settings
let wm = null; // "niri", "hyprland" or "" (any other desktop); null until asked, once

// Niri and Hyprland get their own file, with key bindings in it. Every other desktop gets the one thing
// they all understand: an entry in ~/.config/autostart. What must never happen is writing
// a niri config on a machine that is not running niri — and `XDG_CURRENT_DESKTOP` cannot
// answer that, since many compositors leave it empty. The backend checks properly.
const generic = () => wm === "";
const WM = () => (wm === "hyprland" ? "Hyprland" : "Niri");
const cmd = (name) => `${wm === "hyprland" ? "hyprland" : "niri"}_${name}`;
const configName = () => (wm === "hyprland" ? "hyprland.conf" : "config.kdl");
const desktopName = () => (desktop ? desktop.split(":").pop() : "your desktop");

let getMine = () => [];
let isLoaded = () => true; // has the collection been read yet?
let st = load();
let mascots = []; // [[name, count]]
let status = null; // last niri_status / hyprland_status
let auto = null; // last autostart_status, for every desktop that is not niri
let conflicts = []; // [{keys, file, line}]

function load() {
  try {
    const saved = JSON.parse(store.get(KEY, "null"));
    if (saved && Array.isArray(saved.binds)) return { ...structuredClone(DEFAULTS), ...saved };
  } catch {}
  return structuredClone(DEFAULTS);
}

const save = () => store.set(KEY, JSON.stringify(st));

const norm = (k) => {
  const p = k.split("+").map((x) => x.trim().toLowerCase()).filter(Boolean);
  const last = p.pop() ?? "";
  return [...p.sort(), last].join("+");
};

const conflictFor = (b) => conflicts.find((c) => norm(c.keys) === norm(b.keys));

/** What is actually written: keys that clash with the user's own binds are left out. */
function options() {
  return {
    overlay: st.overlay,
    delay: Number(st.delay) || 0,
    mascots,
    random: Math.max(0, Math.min(60, Number(st.random) || 0)),
    binds: st.binds
      .filter((b) => b.on && b.keys.trim() && !conflictFor(b) && (b.action !== "summon" || b.arg))
      .map(({ keys, action, arg }) => ({ keys, action, arg })),
  };
}

const preview = debounce(async () => {
  try {
    $("startup-out").textContent = await invoke(generic() ? "startup_snippet" : cmd("render"), { options: options() });
  } catch (e) {
    $("startup-out").textContent = String(e);
  }
}, 120);

const setNote = (text, kind = "") => {
  const n = $("startup-note");
  n.textContent = text;
  n.className = `status pre-wrap ${kind}`;
};

// While it is on, changes are written straight away — to whichever file this desktop uses.
const writeOut = debounce(async () => {
  if (generic()) {
    if (!auto?.active) return;
    try {
      auto = await invoke("autostart_enable", { options: options() });
      setNote("Saved. It takes effect at your next login.");
    } catch (e) {
      setNote(String(e), "error");
    }
    return;
  }
  if (!status?.active) return;
  try {
    status = await invoke(cmd("apply"), { options: options() });
    renderStatus();
    setNote(noteAfterWrite(`Saved. ${WM()} picks the change up by itself.`));
  } catch (e) {
    setNote(String(e), "error");
    toast(`${WM()} rejected the change, see the note under the shortcuts`, "error");
  }
}, 500);

function changed() {
  save();
  preview();
  writeOut();
}

// ------------------------------------------------------------------- status

/** What to say after a write: the text itself, plus what could not be written or checked. */
function noteAfterWrite(text) {
  const extra = [];
  if (status?.validation === "not checked") extra.push("Hyprland is not running, so the file could not be checked; it is read the next time Hyprland starts or reloads.");
  if (status?.skipped?.length) extra.push(`Not written:\n${status.skipped.join("\n")}`);
  return [text, ...extra].join("\n");
}

function renderStatus() {
  const box = $("startup-status");
  box.innerHTML = "";

  // Any desktop that is not niri: the same switch, backed by an autostart entry.
  if (generic()) {
    box.classList.remove("warn");
    box.classList.toggle("on", !!(auto && auto.active));
    if (auto && auto.active) {
      box.appendChild(el("div", "grow", `On. Your characters come back when you log in to ${desktopName()}.`));
      const off = el("button", "ghost", "Turn off");
      off.title = auto.file;
      off.addEventListener("click", () =>
        withBusy(off, async () => {
          try {
            auto = await invoke("autostart_disable");
            setNote("Removed.");
          } catch (e) {
            toast(String(e), "error");
          }
          renderStatus();
        }),
      );
      box.appendChild(off);
    } else {
      box.appendChild(el("div", "grow", `Off. Your characters do not come back when you log in to ${desktopName()}.`));
      const on = el("button", "btn", "Turn on");
      on.title = "Writes one file in ~/.config/autostart";
      on.addEventListener("click", () =>
        withBusy(on, async () => {
          try {
            auto = await invoke("autostart_enable", { options: options() });
            setNote("Saved. It takes effect at your next login.");
          } catch (e) {
            toast(String(e), "error");
          }
          renderStatus();
        }),
      );
      box.appendChild(on);
    }
    return;
  }
  // "not checked": Hyprland was not running to ask. That is no problem, just a fact worth a line below.
  const problem = !!(status && status.active && status.validation && status.validation !== "ok" && status.validation !== "not checked");
  box.classList.toggle("warn", problem);
  box.classList.toggle("on", !!(status && status.active) && !problem);
  if (!status) return;

  if (status.active) {
    const bad = status.validation && status.validation !== "ok" && status.validation !== "not checked";
    box.appendChild(el("div", "grow pre-wrap", bad ? `Active, but ${WM()} reports a problem:\n${status.validation}` : `On. Your characters come back when you log in to ${WM()}.`));
    const off = el("button", "ghost", "Turn off");
    off.title = `Removes the added line from ${configName()} and deletes the generated file`;
    off.addEventListener("click", () => withBusy(off, turnOff));
    box.appendChild(off);
  } else {
    box.appendChild(el("div", "grow", `Off. Your characters do not come back when you log in to ${WM()}.`));
    const on = el("button", "btn", "Turn on");
    on.title = `Adds one line to your ${WM()} config`;
    // The question comes first and is not "work": the spinner starts once you say yes.
    on.addEventListener("click", () => turnOn(on));
    box.appendChild(on);
  }
}

async function turnOn(btn) {
  const ok = await confirmDialog(
    wm === "hyprland"
      ? `This adds one line to your Hyprland config and saves a backup next to it:\n\n${status.include_line}\nbackup: ${status.backup}\n\nHyprland reloads and reports what it thinks. If it objects to our file, nothing is changed.`
      : `This adds one line to your Niri config and saves a backup next to it:\n\n${status.include_line}\nbackup: ${status.backup}\n\nNiri checks the result first. If it objects, nothing is changed.`,
    "Turn on",
    "Cancel",
    false
  );
  if (!ok) return;
  await withBusy(btn, async () => {
    try {
      status = await invoke(cmd("enable"), { options: options() });
      renderStatus();
      setNote(noteAfterWrite(`Written to ${status.file}`));
      toast("Launch at login is on", "ok");
    } catch (e) {
      setNote(String(e), "error");
      toast("Could not turn it on. Nothing was changed.", "error");
    }
  });
}

async function turnOff() {
  try {
    status = await invoke(cmd("disable"));
    renderStatus();
    setNote("");
    toast("Launch at login is off", "ok");
  } catch (e) {
    toast(String(e), "error");
  }
}

// ---------------------------------------------------------------- characters

const LIMIT = 20; // most copies of one character summoned at login

async function saveMascots({ redraw = true } = {}) {
  try {
    await invoke("set_autostart", { list: mascots });
  } catch (e) {
    toast(String(e), "error");
  }
  if (redraw) renderLineup();
  changed();
}

const summonable = () => getMine().filter((m) => !m.name.startsWith("."));

/** − 2 +   The number is updated in place, so nothing under the pointer jumps. */
function makeStepper({ value, min, max, label, onSet }) {
  const step = el("div", "stepper");
  const minus = el("button", "", "−");
  const shown = el("span", "stepper-val", String(value));
  const plus = el("button", "", "+");
  for (const b of [minus, plus]) b.type = "button";
  minus.setAttribute("aria-label", `Fewer ${label}`);
  plus.setAttribute("aria-label", `More ${label}`);
  const set = (v) => {
    v = Math.max(min, Math.min(max, v));
    shown.textContent = String(v);
    minus.disabled = v <= min;
    plus.disabled = v >= max;
    onSet(v);
  };
  minus.disabled = value <= min;
  plus.disabled = value >= max;
  minus.addEventListener("click", () => set(Number(shown.textContent) - 1));
  plus.addEventListener("click", () => set(Number(shown.textContent) + 1));
  step.append(minus, shown, plus);
  return step;
}

const DICE =
  '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="3.5" width="17" height="17" rx="4"/><circle cx="8.5" cy="8.5" r="1.2"/><circle cx="15.5" cy="8.5" r="1.2"/><circle cx="12" cy="12" r="1.2"/><circle cx="8.5" cy="15.5" r="1.2"/><circle cx="15.5" cy="15.5" r="1.2"/></svg>';

const RANDOM = "@random"; // the picker's name for "random characters"

/** Added from the picker like anyone else, and first in the list: a number of characters picked at random, differently at every login. */
function randomCard() {
  const n = Math.max(1, Math.min(LIMIT, Number(st.random) || 1));
  const card = el("div", "lineup-card random");
  card.title = "Someone different from your collection each time you log in";

  const x = el("button", "lineup-x", "✕");
  x.type = "button";
  x.title = "Remove random characters";
  x.setAttribute("aria-label", x.title);
  x.addEventListener("click", () => {
    st.random = 0;
    changed();
    renderLineup();
  });
  card.appendChild(x);

  const dice = el("div", "dice");
  dice.innerHTML = DICE;
  card.appendChild(dice);
  card.appendChild(el("div", "lineup-name", "Random"));
  card.appendChild(el("div", "lineup-sub", "from your collection"));
  card.appendChild(
    makeStepper({
      value: n,
      min: 1,
      max: LIMIT,
      label: "random characters",
      onSet: (v) => {
        st.random = v;
        changed();
      },
    })
  );
  return card;
}

/** Who appears at login: the random card, then one card each, with how many, and a tile that adds more. */
function renderLineup() {
  const mine = summonable();
  const chosen = new Set(mascots.map(([n]) => n));
  const frag = document.createDocumentFragment();
  if (Number(st.random) > 0) frag.appendChild(randomCard());

  for (const [name, count] of mascots) {
    const meta = mine.find((m) => m.name === name);
    const card = el("div", "lineup-card");

    const x = el("button", "lineup-x", "✕");
    x.type = "button";
    x.title = `Remove ${pretty(name)}`;
    x.setAttribute("aria-label", x.title);
    x.addEventListener("click", () => {
      mascots = mascots.filter(([nm]) => nm !== name);
      saveMascots();
    });
    card.appendChild(x);

    const stage = el("div", "lineup-stage");
    if (meta?.sprite_path) {
      const img = sprite(convertFileSrc(meta.sprite_path), name);
      stage.appendChild(img);
      attachHover(card, img, () => localFrames(name));
    } else {
      // Until the collection has been read there is no picture to show; that is
      // "still loading", not "this character has none".
      const ph = placeholder(name);
      if (!isLoaded()) ph.classList.add("loading");
      stage.appendChild(ph);
    }
    card.appendChild(stage);

    const label = el("div", "lineup-name", pretty(name));
    label.title = pretty(name);
    card.appendChild(label);

    card.appendChild(
      makeStepper({
        value: count,
        min: 1,
        max: LIMIT,
        label: pretty(name),
        onSet: (v) => {
          mascots = mascots.map(([nm, c]) => (nm === name ? [nm, v] : [nm, c]));
          saveMascots({ redraw: false });
        },
      })
    );

    frag.appendChild(card);
  }

  const left = mine.filter((m) => !chosen.has(m.name)).length + (Number(st.random) > 0 ? 0 : 1);
  const add = el("button", "lineup-add");
  add.type = "button";
  add.innerHTML = '<span class="lineup-plus">+</span><span>Add</span>';
  add.disabled = !left;
  add.title = !isLoaded() ? "Reading your collection…" : !mine.length ? "Nothing is installed yet" : left ? "Choose characters from your collection" : "Everyone is already here";
  add.addEventListener("click", openPicker);
  frag.appendChild(add);

  $("lineup").replaceChildren(frag);
}

// ---------------------------------------------------------------------- picker

// A sheet with the whole collection as cards, so you can see who you are adding,
// search by name, and take several at once.
const picking = new Set();

function openPicker() {
  picking.clear();
  $("pick-search").value = "";
  renderPicker();
  $("pick").classList.remove("hidden");
  $("pick-search").focus();
}

function closePicker() {
  $("pick").classList.add("hidden");
}

function renderPicker() {
  const chosen = new Set(mascots.map(([n]) => n));
  const q = ($("pick-search").value || "").trim().toLowerCase();
  const list = summonable()
    .filter((m) => !chosen.has(m.name))
    .filter((m) => !q || pretty(m.name).toLowerCase().includes(q) || (m.pack_title || "").toLowerCase().includes(q));

  const frag = document.createDocumentFragment();

  // First, so it is easy to find and never in the way: it is only here until it has been added.
  const showRandom = !(Number(st.random) > 0) && (!q || "random".includes(q));
  if (showRandom) {
    const tile = el("button", `pick-card random ${picking.has(RANDOM) ? "on" : ""}`);
    tile.type = "button";
    tile.title = "A different character from your collection every time you log in";
    const stage = el("div", "pick-stage");
    const dice = el("div", "dice-icon");
    dice.innerHTML = DICE;
    stage.appendChild(dice);
    tile.append(stage, el("span", "pick-tick", "✓"), el("div", "pick-name", "Random"));
    tile.addEventListener("click", () => {
      picking.has(RANDOM) ? picking.delete(RANDOM) : picking.add(RANDOM);
      tile.classList.toggle("on", picking.has(RANDOM));
      syncPicker();
    });
    frag.appendChild(tile);
  }
  if (!list.length && !showRandom) frag.appendChild(el("div", "empty-state", "No matches"));

  for (const m of list) {
    const card = el("button", `pick-card ${picking.has(m.name) ? "on" : ""}`);
    card.type = "button";
    card.dataset.name = m.name;

    const stage = el("div", "pick-stage");
    if (m.sprite_path) {
      const img = sprite(convertFileSrc(m.sprite_path), m.name);
      stage.appendChild(img);
      attachHover(card, img, () => localFrames(m.name));
    } else {
      stage.appendChild(placeholder(m.name));
    }
    card.append(stage, el("span", "pick-tick", "✓"), el("div", "pick-name", pretty(m.name)));
    card.title = pretty(m.name);
    card.addEventListener("click", () => {
      picking.has(m.name) ? picking.delete(m.name) : picking.add(m.name);
      card.classList.toggle("on", picking.has(m.name));
      syncPicker();
    });
    frag.appendChild(card);
  }
  $("pick-grid").replaceChildren(frag);
  syncPicker();
}

function syncPicker() {
  const n = picking.size;
  $("pick-add").disabled = n === 0;
  $("pick-add").textContent = n ? `Add ${n}` : "Add";
  $("pick-count").textContent = n ? `${n} selected` : "";
}

function addPicked() {
  if (!picking.size) return;
  const wantsRandom = picking.delete(RANDOM);
  // In the order they are shown, not the order they were clicked.
  const order = summonable().map((m) => m.name);
  const names = [...picking].sort((a, b) => order.indexOf(a) - order.indexOf(b));
  mascots = [...mascots, ...names.map((n) => [n, 1])];
  if (wantsRandom) st.random = 1;
  closePicker();
  saveMascots();
}

// ------------------------------------------------------------ keyboard shortcuts

const NAMED = {
  ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
  Enter: "Return", " ": "Space", PageUp: "Page_Up", PageDown: "Page_Down",
  Tab: "Tab", Home: "Home", End: "End", Insert: "Insert", Delete: "Delete",
};

/** Turns a key press into Niri's spelling ("Mod+Ctrl+D"), or null for a lone modifier. */
function comboFrom(e) {
  if (["Meta", "Control", "Alt", "Shift", "AltGraph", "OS"].includes(e.key)) return null;
  const mods = [];
  if (e.metaKey) mods.push("Mod");
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");

  let key;
  if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
  else if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
  else if (/^F\d{1,2}$/.test(e.key)) key = e.key;
  else key = NAMED[e.key];
  if (!key) return undefined; // a key Niri spells differently: not supported here
  return { mods, key };
}

/**
 * The shortcut field: the current keys, a ✕ that clears them, and while it waits
 * for a key press a hint that says how to back out. Clicking anywhere else backs
 * out too, so Esc is a shortcut and not the only way.
 */
function keyButton(b) {
  const wrap = el("div", "keybox");
  const hint = el("span", "keyhint hidden", "Esc to cancel");
  const btn = el("button", "keycap");
  btn.type = "button";
  const clear = el("button", "keyclear", "✕");
  clear.type = "button";
  clear.title = "Clear this shortcut";
  clear.setAttribute("aria-label", clear.title);
  wrap.append(hint, btn, clear);

  let listening = false;

  const paint = () => {
    btn.classList.toggle("listening", listening);
    btn.classList.toggle("unset", !listening && !b.keys);
    btn.textContent = listening ? "Press keys…" : b.keys || "Not set";
    hint.classList.toggle("hidden", !listening);
    clear.classList.toggle("invisible", listening || !b.keys);
  };

  const done = () => {
    listening = false;
    window.removeEventListener("keydown", onKey, true);
    paint();
  };

  const onKey = (e) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") return done();
    if (e.key === "Backspace") {
      b.keys = "";
      done();
      renderBinds();
      return changed();
    }
    const combo = comboFrom(e);
    if (combo === null) return; // just a modifier so far
    if (combo === undefined) return toast("That key cannot be used here", "error");
    if (!combo.mods.length) return toast("Hold Super, Ctrl, Alt or Shift as well", "error");
    b.keys = [...combo.mods, combo.key].join("+");
    done();
    refreshConflicts().then(() => {
      renderBinds();
      changed();
    });
  };

  btn.addEventListener("click", () => {
    if (listening) return done();
    listening = true;
    window.addEventListener("keydown", onKey, true);
    paint();
  });
  btn.addEventListener("blur", () => listening && done());

  clear.addEventListener("click", async () => {
    b.keys = "";
    await refreshConflicts();
    renderBinds();
    changed();
  });

  paint();
  return wrap;
}

function renderBinds() {
  const mine = getMine().filter((m) => !m.name.startsWith("."));
  const frag = document.createDocumentFragment();

  st.binds.forEach((b) => {
    const row = el("div", "bind");
    const what = el("label", "what");
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = b.on;
    cb.addEventListener("change", () => {
      b.on = cb.checked;
      renderBinds();
      changed();
    });
    what.append(cb, document.createTextNode(LABEL[b.action]));

    if (b.action === "summon") {
      const sel = document.createElement("select");
      sel.className = "field";
      sel.setAttribute("aria-label", "Character to summon");
      // While the collection is still being read `mine` is empty. The saved choice
      // must not be replaced by "nothing" (it was, and the next save kept it), so
      // until then the select shows the saved name and is left alone.
      if (isLoaded() && mine.length) {
        for (const m of mine) sel.appendChild(new Option(pretty(m.name), m.name));
        if (!mine.some((m) => m.name === b.arg)) b.arg = mine[0].name;
      } else {
        sel.appendChild(new Option(b.arg ? pretty(b.arg) : "…", b.arg));
        sel.disabled = true;
      }
      sel.value = b.arg;
      sel.addEventListener("change", () => {
        b.arg = sel.value;
        changed();
      });
      what.appendChild(sel);
    }

    row.append(what, keyButton(b));
    frag.appendChild(row);

    const c = b.on && conflictFor(b);
    if (c) {
      frag.appendChild(
        el("div", "bind-warn", `Not written: ${b.keys} is already used in your ${WM()} config (${c.file}, line ${c.line}). Pick another key.`)
      );
    }
  });
  $("startup-binds").replaceChildren(frag);
}

async function refreshConflicts() {
  const keys = st.binds.filter((b) => b.on && b.keys.trim()).map((b) => b.keys);
  try {
    conflicts = keys.length ? await invoke(cmd("conflicts"), { keys }) : [];
  } catch {
    conflicts = [];
  }
}

// ---------------------------------------------------------------------- API

export async function refreshStartup({ status: withStatus = true } = {}) {
  if (wm === null) {
    const info = await invoke("environment_check").catch(() => null);
    wm = info?.niri ? "niri" : info?.hyprland ? "hyprland" : "";
    desktop = info?.desktop || "";
    kde = info?.compositor === "kde";
  }
  const other = generic();
  // Key bindings are written into niri's own config; no other desktop has a format this
  // app could safely edit, so there they become commands to bind by hand.
  $("startup-keys").classList.toggle("hidden", other);
  $("startup-cmds").classList.toggle("hidden", !other);
  $("startup-details").open = other;
  if (other) {
    renderCommands();
    auto = await invoke("autostart_status").catch(() => null);
  }
  try {
    mascots = (await invoke("get_autostart")) || [];
  } catch {}
  $("startup-overlay").checked = st.overlay;
  $("startup-delay").value = st.delay;
  renderLineup();
  renderBinds();
  await refreshConflicts();
  renderBinds();
  preview();
  if (other) renderStatus();
  else if (withStatus) {
    try {
      status = await invoke(cmd("status"));
      renderStatus();
    } catch {}
  }
}

/** Key bindings are the compositor's business; here are the commands to bind. */
function renderCommands() {
  const rows = [
    ["Dismiss everyone", "timeout -s INT 0.7 shimejictl mascot dismiss --all"],
    ["Stop the overlay", "shimejictl stop"],
    ["Summon a character", "shimejictl summon 'Name'"],
  ];
  // Where to put them, for the desktops whose shortcuts we cannot write for you.
  $("startup-cmd-hint").textContent = kde
    ? "In System Settings, open Keyboard, then Shortcuts, then Add Command, and paste one of these. \"Dismiss all\" and \"Stop the overlay\" are also listed under Menagerie once you add it as an application there."
    : "Add these in your desktop's own keyboard shortcut settings.";
  const frag = document.createDocumentFragment();
  for (const [what, cmd] of rows) {
    const row = el("div", "opt");
    const text = el("div", "opt-text");
    text.append(el("div", "opt-name", what), el("code", "path", cmd));
    const cp = el("button", "ghost", "Copy");
    cp.addEventListener("click", () => copyText(cmd));
    const ctl = el("div", "opt-ctl");
    ctl.appendChild(cp);
    row.append(text, ctl);
    frag.appendChild(row);
  }
  $("startup-cmd-rows").replaceChildren(frag);
}

async function copyText(text) {
  try {
    if (clip?.writeText) await clip.writeText(text);
    else await navigator.clipboard.writeText(text);
    toast("Copied", "ok");
  } catch {
    toast("Could not copy to the clipboard", "error");
  }
}

export function initStartup({ mine, loaded }) {
  getMine = mine;
  if (loaded) isLoaded = loaded;

  $("startup-overlay").addEventListener("change", (e) => {
    st.overlay = e.target.checked;
    changed();
  });
  $("startup-delay").addEventListener("input", (e) => {
    st.delay = e.target.value;
    changed();
  });
  // − and + in place of the browser's own tiny arrows.
  const bump = (d) => {
    const inp = $("startup-delay");
    const v = Math.min(60, Math.max(0, Math.round(((Number(inp.value) || 0) + d) * 10) / 10));
    inp.value = v;
    inp.dispatchEvent(new Event("input"));
  };
  $("startup-delay-dec").addEventListener("click", () => bump(-0.5));
  $("startup-delay-inc").addEventListener("click", () => bump(0.5));
  $("pick-search").addEventListener("input", debounce(renderPicker, 100));
  $("pick-add").addEventListener("click", addPicked);
  $("pick-cancel").addEventListener("click", closePicker);
  $("pick-close").addEventListener("click", closePicker);
  $("pick-bg").addEventListener("click", closePicker);
  document.addEventListener("keydown", (e) => {
    if ($("pick").classList.contains("hidden")) return;
    if (e.key === "Escape") closePicker();
    else if (e.key === "Enter" && picking.size) {
      e.preventDefault();
      addPicked();
    }
  });
  $("startup-copy").addEventListener("click", () => copyText($("startup-out").textContent));
}
