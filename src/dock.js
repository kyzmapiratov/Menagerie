// Install progress panel (bottom right).
//
// The backend emits `install-progress` at every step of every character:
//   { key, name, index, total, stage, fraction, message }
// The panel keeps one row per character and updates that row in place.
//
// In place matters. An earlier version threw the whole list away and rebuilt it
// on every event, which restarted each spinner from zero and let the row height
// jump whenever a message changed length or the little progress bar appeared.
// Now every row is built once, the icon slot and the bar have a fixed size even
// when they are empty, and messages stay on one line.

import { $, el, events, invoke, pretty, spinner, withBusy } from "./util.js";

const rows = new Map(); // key -> { data, node, refs }
let hideTimer = null;
let collapsed = false;
let cancelRequested = false;
let hooks = { summon: async () => false, openCollection: () => {} };

const TERMINAL = new Set(["done", "error", "skipped", "cancelled"]);
const isFinished = () => rows.size > 0 && [...rows.values()].every((r) => TERMINAL.has(r.data.stage));
const hasError = () => [...rows.values()].some((r) => r.data.stage === "error");

function overall() {
  const list = [...rows.values()].map((r) => r.data);
  const declared = Math.max(...list.map((r) => r.total), list.length);
  const sum = list.reduce((a, r) => a + (TERMINAL.has(r.stage) ? 1 : r.stage === "queued" ? 0 : r.fraction), 0);
  return { frac: declared ? Math.min(1, sum / declared) : 0, declared };
}

function clearRows() {
  rows.clear();
  cancelRequested = false;
  $("dock-list").replaceChildren();
}

function hide() {
  $("dock").classList.add("hidden");
  clearRows();
}

/** A finished batch goes away by itself, unless something failed or the mouse is on it. */
function scheduleHide() {
  clearTimeout(hideTimer);
  if (isFinished() && !hasError()) hideTimer = setTimeout(hide, 12000);
}

function onProgress(p) {
  clearTimeout(hideTimer);

  // A new batch after a finished one starts from a clean slate.
  // (A terminal event for an unknown key does not open a batch: it is a late message.)
  if (isFinished() && !TERMINAL.has(p.stage) && (p.stage === "queued" || !rows.has(p.key))) clearRows();

  let row = rows.get(p.key);
  if (!row) {
    row = buildRow();
    rows.set(p.key, row);
    $("dock-list").appendChild(row.node);
  }
  row.data = p;
  syncRow(row);
  render();

  if (isFinished()) scheduleHide();
}

// ---------------------------------------------------------------------- rows

function buildRow() {
  const node = el("div", "dock-row");

  // The icon slot has the same size whatever it shows, and the spinner element
  // is created once, so its animation is never restarted.
  const icon = el("span", "dock-icon");
  const spin = spinner("spin");
  const glyph = el("span", "dock-glyph");
  icon.append(spin, glyph);

  const body = el("div", "dock-body");
  const name = el("div", "dock-name");
  const msg = el("div", "dock-msg");
  const bar = el("div", "bar mini");
  const fill = el("div", "bar-fill");
  bar.appendChild(fill);
  body.append(name, msg, bar);

  const act = el("div", "dock-act");
  node.append(icon, body, act);
  return { data: null, node, refs: { spin, glyph, name, msg, bar, fill, act, summon: null } };
}

const GLYPH = { done: "✓", skipped: "–", cancelled: "–", error: "✕", queued: "•" };

function shorten(text) {
  const first = String(text).split("\n")[0];
  return first.length > 110 ? first.slice(0, 110) + "…" : first;
}

function setText(node, text) {
  if (node.textContent !== text) node.textContent = text;
}

function syncRow(row) {
  const r = row.data;
  const { spin, glyph, name, msg, bar, fill, act } = row.refs;
  const active = !TERMINAL.has(r.stage) && r.stage !== "queued";

  row.node.dataset.stage = r.stage;
  spin.hidden = !active;
  glyph.hidden = active;
  setText(glyph, GLYPH[r.stage] || "");

  setText(name, r.name);
  setText(msg, r.stage === "error" ? shorten(r.message) : r.message);
  msg.title = r.stage === "error" ? r.message : "";

  // Reserved even when it is not shown, so the row never changes height.
  bar.style.visibility = active ? "visible" : "hidden";
  fill.style.width = `${Math.round(r.fraction * 100)}%`;

  // A finished catalog character can be summoned from here. (The local-archive
  // row is named after a list of characters, so it has no button.)
  if (r.stage === "done" && r.key !== "local" && r.key !== "export" && !row.refs.summon) {
    const b = el("button", "link-btn", "Summon");
    b.title = `Summon ${pretty(r.name)}`;
    b.addEventListener("click", () =>
      withBusy(b, async () => {
        const ok = await hooks.summon(r.name);
        if (!ok) return;
        setText(b, "Summoned");
        setTimeout(() => setText(b, "Summon"), 1500);
      })
    );
    act.appendChild(b);
    row.refs.summon = b;
  }
}

// -------------------------------------------------------------------- header

function render() {
  const dock = $("dock");
  dock.classList.remove("hidden");
  dock.classList.toggle("collapsed", collapsed);

  const all = [...rows.values()].map((r) => r.data);
  const { frac, declared } = overall();
  const finished = isFinished();
  const count = (stage) => all.filter((r) => r.stage === stage).length;
  const done = count("done"), failed = count("error"), skipped = count("skipped"), cancelled = count("cancelled");

  // An export is one row that counts up through the whole collection, and it is not an install: its own words.
  const exporting = all.length === 1 && all[0].key === "export";

  let title;
  if (exporting) {
    title = finished ? (done ? "Collection exported" : cancelled ? "Export cancelled" : "Export failed") : cancelRequested ? "Cancelling…" : "Exporting the collection";
  } else if (finished) {
    if (all.length === 1 && done === 1) title = `${pretty(all[0].name)} installed`;
    else {
      const parts = [];
      if (done) parts.push(`${done} installed`);
      if (skipped) parts.push(`${skipped} skipped`);
      if (cancelled) parts.push(`${cancelled} cancelled`);
      if (failed) parts.push(`${failed} failed`);
      title = parts.join(", ").replace(/^./, (c) => c.toUpperCase()) || "Done";
    }
  } else if (cancelRequested) {
    title = "Cancelling…";
  } else {
    title = declared > 1 ? `Installing · ${Math.min(done + failed + skipped + cancelled + 1, declared)} of ${declared}` : "Installing";
  }
  setText($("dock-title"), title);
  setText($("dock-pct"), finished ? "" : `${Math.round(frac * 100)}%`);

  const fill = $("dock-fill");
  fill.style.width = `${frac * 100}%`;
  fill.className = `bar-fill ${failed ? "bad" : finished ? "ok" : ""}`;

  $("dock-close").classList.toggle("hidden", !finished);

  // Cancel only makes sense while a catalog install is running; the local archive
  // conversion is a few seconds of local work and is not interruptible.
  const cancellable = !finished && all.some((r) => !TERMINAL.has(r.stage) && r.key !== "local");
  const cancel = $("dock-cancel");
  cancel.classList.toggle("hidden", !cancellable);
  cancel.title = exporting ? "Stop the export. No archive is written." : "Stop installing. What is already installed stays.";
  cancel.disabled = cancelRequested;

  $("dock-foot").classList.toggle("hidden", !(finished && done > 0 && !exporting));
}

export function initDock({ summon, openCollection } = {}) {
  if (summon) hooks.summon = summon;
  if (openCollection) hooks.openCollection = openCollection;

  events.listen("install-progress", (e) => onProgress(e.payload));

  $("dock-toggle").addEventListener("click", () => {
    collapsed = !collapsed;
    $("dock").classList.toggle("collapsed", collapsed);
    $("dock-toggle").textContent = collapsed ? "▴" : "▾";
  });
  $("dock-close").addEventListener("click", hide);

  $("dock-cancel").addEventListener("click", () => {
    cancelRequested = true;
    render();
    invoke("cancel_install").catch(() => {});
  });
  // Finished characters live in the Collection, so that is where this leads,
  // handing over who was just installed so they can be pointed out there.
  $("dock-collection").addEventListener("click", () => {
    const names = [...rows.values()].filter((r) => r.data.stage === "done" && r.data.key !== "local").map((r) => r.data.name);
    hooks.openCollection(names);
  });

  // Pointing at the panel keeps it: it should not vanish while you are about to click Summon.
  $("dock").addEventListener("mouseenter", () => clearTimeout(hideTimer));
  $("dock").addEventListener("mouseleave", scheduleHide);
}
