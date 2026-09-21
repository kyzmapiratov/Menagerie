// Watching the overlay.
//
// wl_shimeji's overlay is a program of its own, and it is not always there: it
// quits by itself when the screen is empty (after "Dismiss all", say), and now and
// then it crashes. From the app's side both look the same: the characters that were
// on screen are gone. This module notices, tells the two apart where the system
// allows it (it keeps a list of crashes), and offers to bring everyone back with
// one click, instead of leaving an empty screen and no explanation.

import { $, el, invoke, toast, store, plural, pretty, sleep, withBusy, clip, summonError } from "./util.js";

let up = null; // is the process alive (null: not known yet)
let lastAlive = Date.now(); // when it was last seen alive
let seen = readSaved(); // who was on screen the last time we looked: [[name, count]]
let quietUntil = 0; // before this moment a stop is deliberate
let alertShown = false;
const listeners = [];
const recoveries = []; // when the app brought characters back on its own

function readSaved() {
  try {
    const list = JSON.parse(store.get("last-scene", "[]"));
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}

const total = (list) => list.reduce((a, [, c]) => a + c, 0);



function paint(status) {
  const pill = $("health");
  pill.textContent = status.running ? "Overlay running" : "Overlay idle";
  pill.title = status.running
    ? "wl_shimeji is running"
    : "The overlay starts by itself when you summon someone, and quits when the screen is empty.";
  pill.classList.toggle("on", status.running);
}

/** Asks whether the overlay is alive, updates the pill, and reacts when it disappeared. */
async function refresh() {
  let status;
  try {
    status = await invoke("overlay_status");
  } catch {
    return null;
  }
  paint(status);
  const was = up;
  up = status.running;
  if (status.running) {
    lastAlive = Date.now();
    if (alertShown) hideAlert();
  }
  if (was !== up) listeners.forEach((cb) => cb(up));
  if (was === true && !status.running) await vanished();
  return status;
}

async function vanished() {
  // We emptied the screen ourselves, or nobody was there: nothing to report.
  if (Date.now() < quietUntil || !total(seen)) return;

  // A crash, or a quiet exit? The system's crash list knows, and needs a moment to catch up.
  let crashes = null;
  for (let i = 0; i < 2; i++) {
    try {
      crashes = await invoke("overlay_crashes", { sinceMs: lastAlive - 10000 });
    } catch {
      crashes = null; // no such list here: all we can do is guess
    }
    if (!Array.isArray(crashes)) {
      crashes = null;
      break;
    }
    if (crashes.length) break;
    await sleep(2500);
  }
  if (crashes && !crashes.length) return; // it left on its own (someone pressed the stop key, say)

  const crashed = !!(crashes && crashes.length);
  // The app itself ended it, because the desktop had dropped its connection and it went on
  // running with nothing to draw on.
  const cutOff = !!crashes?.some((c) => c.cut_off);

  // While it is down is the safe time to fix what broke it: a character that lacks
  // pictures (the overlay names it in its log) gets them filled in from its nearest one.
  let culprit = null;
  let fixed = [];
  if (crashed) {
    culprit = await invoke("overlay_culprit").catch(() => null);
    fixed = await invoke("repair_characters").catch(() => []);
  }
  const hour = await invoke("overlay_crashes", { sinceMs: Date.now() - 3600 * 1000 }).catch(() => []);

  if (store.get("overlay-recover", "0") === "1" && recoveryAllowed()) {
    recoveries.push(Date.now());
    const n = await bringBack();
    if (n) toast(`The overlay ${cutOff ? "lost its connection" : crashed ? "crashed" : "stopped"}; brought ${plural(n, "character")} back`);
    return;
  }
  showAlert({ crashed, cutOff, signal: crashes?.[0]?.signal, culprit, fixed, lastHour: Array.isArray(hour) ? hour.length : 0 });
}

/** At most three automatic recoveries in three minutes: a crash that repeats at once needs a person. */
function recoveryAllowed() {
  const cutoff = Date.now() - 3 * 60 * 1000;
  while (recoveries.length && recoveries[0] < cutoff) recoveries.shift();
  return recoveries.length < 3;
}

/** Starts the overlay if need be and summons who was on screen last time. Resolves to how many. */
async function bringBack() {
  if (!seen.length) {
    toast("There is nobody to bring back", "error");
    return 0;
  }
  try {
    const result = await invoke("summon_batch", { items: seen });
    hideAlert();
    const later = result.restart_needed?.length ? `; ${result.restart_needed.map(pretty).join(", ")} after the overlay restarts` : "";
    toast(`Brought back ${result.spawned}${later}`, "ok");
    refresh();
    return result.spawned;
  } catch (e) {
    summonError(e);
    return 0;
  }
}

// ------------------------------------------------------------------ the alert card

function hideAlert() {
  alertShown = false;
  $("overlay-alert").classList.add("hidden");
}

const WARNING =
  '<svg class="alert-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3.6 2.9 19.4h18.2L12 3.6Z"/><path d="M12 10v4.5"/><circle cx="12" cy="17.2" r="0.7"/></svg>';

/**
 * A crash is serious, so it looks it: red edge, a warning sign, what it was, what was
 * lost and who was to blame when the overlay's log says so, how often it has happened
 * this hour, and the one button that puts things right. A quiet stop is amber and calm.
 */
function showAlert({ crashed, cutOff = false, signal, culprit, fixed = [], lastHour = 0 }) {
  const box = $("overlay-alert");
  box.classList.toggle("calm", !crashed);
  const n = total(seen);

  const body = el("div", "alert-body");
  body.appendChild(el("div", "alert-title", cutOff ? "The overlay lost its connection" : crashed ? "The overlay crashed" : "The overlay stopped"));
  const sub = el("div", "alert-sub");
  sub.append(`${plural(n, "character")} ${n === 1 ? "was" : "were"} on screen. `);
  const named = culprit ? pretty(culprit) : null;
  if (named) {
    const wasFixed = fixed.some((f) => pretty(f.name) === named);
    sub.append(el("b", "", named), wasFixed ? " lacked some pictures, which is what brought it down. They are filled in now." : " was the last one it complained about.");
  } else if (cutOff) {
    sub.append("The desktop closed the overlay's connection, and it went on running with nothing to draw on, so the app ended it. Nothing was lost; they can all come back.");
  } else if (crashed) {
    sub.append(
      signal === 11
        ? "It hit a segmentation fault — a fault inside wl_shimeji itself, which happens now and then while a crowd of characters is moving about. Nothing was lost; they can all come back."
        : "It stopped without warning.",
    );
  }
  body.appendChild(sub);
  if (crashed && lastHour > 1) body.appendChild(el("div", "alert-count", `${lastHour} crashes in the last hour`));

  const close = el("button", "icon-btn", "✕");
  close.title = "Dismiss";
  close.addEventListener("click", hideAlert);
  const icon = el("span");
  icon.innerHTML = WARNING;
  const head = el("div", "alert-head");
  head.append(icon.firstChild, body, close);

  const actions = el("div", "alert-actions");
  const again = el("button", "btn", n === 1 ? "Bring it back" : "Bring them back");
  again.addEventListener("click", () => withBusy(again, bringBack));
  const more = el("button", "link-btn", "Details");
  const log = el("pre", "pre hidden");
  const readLog = async () => (await invoke("overlay_log", { lines: 12 }).catch(() => "")) || "The overlay printed nothing.";
  more.addEventListener("click", async () => {
    if (log.classList.contains("hidden")) log.textContent = await readLog();
    log.classList.toggle("hidden");
  });
  const copy = el("button", "link-btn", "Copy log");
  copy.addEventListener("click", async () => {
    try {
      const text = await readLog();
      if (clip?.writeText) await clip.writeText(text);
      else await navigator.clipboard.writeText(text);
      toast("Log copied", "ok");
    } catch {
      toast("Could not copy the log", "error");
    }
  });
  actions.append(again, more, copy);

  box.replaceChildren(head, actions, log);
  box.classList.remove("hidden");
  alertShown = true;
}

export const overlay = {
  get up() {
    return up;
  },
  get seen() {
    return seen;
  },
  refresh,
  bringBack,
  /** Call with who is on screen after every successful look. An empty screen is not remembered. */
  remember(list) {
    if (!list.length || JSON.stringify(list) === JSON.stringify(seen)) return;
    seen = list;
    store.set("last-scene", JSON.stringify(list));
  },
  /** The app is about to empty the screen on purpose: the overlay quitting is not news. */
  quiet(ms = 9000) {
    quietUntil = Date.now() + ms;
  },
  /** Called when the overlay starts or stops. */
  onChange(cb) {
    listeners.push(cb);
  },
};
