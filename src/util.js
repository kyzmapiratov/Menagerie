// Shared UI helpers.

export const { invoke, convertFileSrc } = window.__TAURI__.core;
export const dlg = window.__TAURI__.dialog;
export const clip = window.__TAURI__.clipboardManager;
export const events = window.__TAURI__.event;

export const $ = (id) => document.getElementById(id);

// A focus ring is for people who use the keyboard. WebKitGTK also draws one on a
// button after a mouse click (the tab you just pressed, the "Details" link), which
// looked like a stray box. So the ring only shows once a navigation key has been
// used, and goes away again at the next click or tap.
{
  const root = document.documentElement;
  const NAV = new Set(["Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"]);
  addEventListener("keydown", (e) => NAV.has(e.key) && root.classList.add("kbd"), true);
  addEventListener("pointerdown", () => root.classList.remove("kbd"), true);
}

// These used to replace the whole class list, which threw away `status`: the line
// lost its colour, its size and the height it keeps reserved, so the page below
// it moved every time the text came or went. Now only `error` is toggled.
export function note(el, text, kind = "") {
  el.textContent = text;
  el.classList.add("status");
  el.classList.toggle("error", kind === "error");
}

// The one waiting mark: an arc that turns on a faint rail and, as it turns, grows and shortens. It is a real
// SVG shape because the length of a stroke can only be animated on one (see "the waiting mark" in styles.css).
const SPINNER =
  '<svg class="sp" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true">' +
  '<circle cx="12" cy="12" r="9.5" opacity=".2"/><circle class="hd" cx="12" cy="12" r="9.5" pathLength="100"/></svg>';

/** The spinner in a span: `spin` for one that sits in a line of text, `wm` for one laid over an element. */
export function spinner(cls = "spin") {
  const s = document.createElement("span");
  s.className = cls;
  s.innerHTML = SPINNER;
  return s;
}

export function busy(el, text) {
  el.innerHTML = `<span class="spin">${SPINNER}</span>${text}`;
  el.classList.add("status");
  el.classList.remove("error");
}

/**
 * Our own confirmation dialog.
 * The native window.confirm() is not shown in Tauri's webview, so destructive
 * actions used to run silently. Hence the custom dialog.
 */
export function confirmDialog(text, okLabel = "Delete", cancelLabel = "Cancel", danger = true) {
  return new Promise((resolve) => {
    const box = $("confirm");
    $("confirm-text").textContent = text;
    $("confirm-yes").textContent = okLabel;
    $("confirm-no").textContent = cancelLabel;
    $("confirm-yes").classList.toggle("danger", danger);
    box.classList.remove("hidden");

    const done = (val) => {
      box.classList.add("hidden");
      $("confirm-yes").onclick = null;
      $("confirm-no").onclick = null;
      $("confirm-bg").onclick = null;
      resolve(val);
    };

    $("confirm-yes").onclick = () => done(true);
    $("confirm-no").onclick = () => done(false);
    $("confirm-bg").onclick = () => done(false);
  });
}

/** Dialog with a text field. Resolves to the string, or null if cancelled. */
export function promptDialog(text, placeholder = "", okLabel = "Save", initial = "") {
  return new Promise((resolve) => {
    const box = $("prompt");
    const input = $("prompt-input");
    $("prompt-text").textContent = text;
    $("prompt-yes").textContent = okLabel;
    input.placeholder = placeholder;
    input.value = initial;
    box.classList.remove("hidden");
    input.focus();

    const done = (val) => {
      box.classList.add("hidden");
      $("prompt-yes").onclick = $("prompt-no").onclick = $("prompt-bg").onclick = null;
      input.onkeydown = null;
      resolve(val);
    };
    $("prompt-yes").onclick = () => done(input.value.trim());
    $("prompt-no").onclick = () => done(null);
    $("prompt-bg").onclick = () => done(null);
    input.onkeydown = (e) => {
      if (e.key === "Enter") done(input.value.trim());
      if (e.key === "Escape") done(null);
    };
  });
}

let toastTimer;
/**
 * A short message at the bottom. Errors stay longer, and a message can carry one
 * action ("Try again"), so a failure is not a dead end.
 */
/** What the backend says when a summon was called off on purpose. */
export const SUMMON_STOPPED = "Summoning stopped.";

/** Shows a failed summon, unless it failed because somebody stopped it. */
export function summonError(e) {
  const text = String(e);
  if (text !== SUMMON_STOPPED) toast(text, "error");
}

export function toast(text, kind = "", { action = null, ms = null } = {}) {
  const box = $("toast");
  const shown = text.length > 240 ? text.slice(0, 240) + "…" : text;
  box.className = `toast ${kind}`;
  box.replaceChildren(el("span", "toast-text", shown));
  if (action) {
    const b = el("button", "link-btn", action.label);
    b.addEventListener("click", () => {
      box.classList.add("hidden");
      action.run();
    });
    box.appendChild(b);
  }
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => box.classList.add("hidden"), ms ?? (kind === "error" ? 7000 : 4500));
}

/**
 * A character picture. A load can fail for a passing reason (the file is being
 * rewritten, the network blinked), and removing the image on the first failure
 * left a card with a hole in it that stayed until the next redraw. So it tries
 * again twice; if the picture really is not there it shows the character's
 * initial in its place, keeps the image element (hidden) and tries once more
 * whenever `healPictures()` runs. The letter goes away by itself when it works.
 */
export function sprite(url, alt = "") {
  const img = document.createElement("img");
  img.alt = alt;
  img.loading = "lazy";

  let tries = 0; // automatic retries so far
  let nonce = 0; // makes every request address different: asking for the same one again is answered from memory, with the same error
  let stand = null; // the letter shown while the picture is missing

  const reload = () => {
    // A data: address cannot take a query string; anything else gets one so the retry is a fresh request.
    img.src = url.startsWith("data:") ? url : `${url}${url.includes("?") ? "&" : "?"}retry=${++nonce}`;
  };
  img.onload = () => {
    if (!stand) return;
    stand.remove();
    stand = null;
    img.style.display = "";
  };
  img.onerror = () => {
    if (tries < 2) return void setTimeout(reload, 350 * ++tries);
    if (stand) return;
    stand = placeholder(alt);
    img.style.display = "none";
    img.after(stand);
  };
  img.heal = () => {
    if (!stand) return;
    tries = 1; // one more go, not the full round of retries
    // A hidden image with loading="lazy" is never fetched (it has no box to scroll
    // into view), so the second attempt has to ask for the picture straight away.
    img.loading = "eager";
    reload();
  };
  img.src = url;
  return img;
}

/** Gives every picture that is showing a letter (because it failed) another try. */
export function healPictures(root = document) {
  root.querySelectorAll("img").forEach((img) => img.heal?.());
}

/** The round stand-in for a picture: the first letter of the name. */
export function placeholder(name = "") {
  return el("div", "ph", pretty(name).charAt(0).toUpperCase() || "?");
}

/**
 * Runs `task` while `btn` shows that it is working: the spinner takes the place of a button's label
 * (which fades out, so the button keeps its size and nothing around it moves), lies over a card, sits
 * before a link. Clicks are ignored from the first moment, but the spinner itself waits a beat: an
 * action that finishes at once should not flash one.
 * `card: true` is for tiles, cards and presets: a tile dims a little under the spinner, a preset's
 * play button turns into it and its name stays as it is.
 */
export async function withBusy(btn, task, { card = false } = {}) {
  if (!btn) return task();
  if (btn.classList.contains("wait")) return;
  btn.classList.add("wait");
  btn.setAttribute("aria-busy", "true");
  const timer = setTimeout(() => setBusy(btn, true, { card }), 150);
  try {
    return await task();
  } finally {
    clearTimeout(timer);
    setBusy(btn, false, { card });
    btn.classList.remove("wait");
    btn.removeAttribute("aria-busy");
  }
}

/**
 * Puts the spinner on an element, or takes it off. A preset carries it on its round play button, anything
 * else on itself. The styling is all in the `busy` / `busy-card` class, so this only keeps the two in step.
 */
export function setBusy(el, on, { card = false } = {}) {
  const host = el.classList.contains("preset-card") ? el.querySelector(".preset-go") || el : el;
  host.querySelectorAll(":scope > .wm").forEach((m) => m.remove());
  el.classList.toggle(card ? "busy-card" : "busy", on);
  if (on) host.appendChild(spinner("wm"));
}

/**
 * Lets only the newest of several overlapping loads apply its answer. Without
 * it, an older request that finishes later overwrites a newer one, which made
 * lists jump back to how they were, or empty themselves.
 */
export function latest() {
  let n = 0;
  return { next: () => ++n, isCurrent: (t) => t === n };
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Fills `host` with `n` shimmering placeholders (see .skel in the stylesheet). */
export function skeletons(host, n, size) {
  const frag = document.createDocumentFragment();
  for (let i = 0; i < n; i++) frag.appendChild(el("div", `skel ${size}`));
  host.replaceChildren(frag);
}

/**
 * The same, but as many as it takes to reach the bottom of the window.
 *
 * A fixed number leaves a stripe of placeholders and a page of emptiness under it, which
 * reads as "that is all there is" rather than "this is loading". The count is measured
 * from the real layout — one placeholder is put in, its size and the grid's columns are
 * read back — so it follows the window and the stylesheet without repeating either.
 */
export function fillSkeletons(host, size, { max = 60 } = {}) {
  host.replaceChildren(el("div", `skel ${size}`));
  const first = host.firstElementChild;
  const box = first.getBoundingClientRect();
  const styles = getComputedStyle(host);
  const columns = (styles.gridTemplateColumns || "").split(" ").filter(Boolean).length || 1;
  const gap = parseFloat(styles.rowGap) || 10;
  const room = Math.max(0, window.innerHeight - box.top);
  const rows = Math.max(1, Math.ceil(room / Math.max(1, box.height + gap)));
  skeletons(host, Math.min(max, columns * rows), size);
}

/** Opens a link in the system browser (allowed domains live in capabilities/default.json). */
export async function openExternal(url) {
  try {
    await window.__TAURI__.opener.openUrl(url);
  } catch (e) {
    toast(`Could not open the link: ${e}`, "error");
  }
}

/** Small helper for building elements: el("div", "class", "text"). */
export function el(tag, cls = "", text = "") {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text) node.textContent = text;
  return node;
}

/** localStorage may be unavailable; then we simply run without remembering anything. */
// Preferences live in a file on disk (see prefs.rs), not in localStorage: that is
// tied to the webview's own cache and is not shared between `tauri dev` and a
// release build. They are read once at start-up (top-level await, so every
// module that imports this one sees them already loaded); `set` updates the
// in-memory copy at once and saves in the background.
async function loadPrefs() {
  try {
    const saved = await invoke("prefs_load");
    if (saved && typeof saved === "object") {
      // One-time move of anything an older version kept in localStorage.
      if (!Object.keys(saved).length) {
        try {
          for (let i = 0; i < localStorage.length; i++) {
            const k = localStorage.key(i);
            const v = localStorage.getItem(k);
            if (k && v !== null) {
              saved[k] = v;
              invoke("prefs_set", { key: k, value: v }).catch(() => {});
            }
          }
        } catch {}
      }
      return saved;
    }
  } catch {}
  return {};
}

const prefs = await loadPrefs();

export const store = {
  get(key, fallback = null) {
    return Object.prototype.hasOwnProperty.call(prefs, key) ? prefs[key] : fallback;
  },
  set(key, value) {
    const v = String(value);
    prefs[key] = v;
    invoke("prefs_set", { key, value: v }).catch(() => {});
  },
};

/** Display name: no leading dot or underscores (Bill_Cipher → Bill Cipher). */
export function pretty(name) {
  return String(name).replace(/^\./, "").replace(/_/g, " ").replace(/\s+/g, " ").trim();
}

/** "1 character", "7 characters". Packs with a single character are common enough
 *  that the plain `${n} characters` form showed up as "1 characters" all over. */
export const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Name for comparing across sources: ignores case, spaces and punctuation. */
export const norm = (s) => (s || "").toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");

/** 4.2 MB / 820 KB */
export function fmtSize(bytes) {
  if (!bytes) return "";
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  const mb = bytes / 1048576;
  return `${mb >= 100 ? Math.round(mb) : mb.toFixed(1)} MB`;
}

/** Delayed call: keeps a list from redrawing on every keystroke of a search. */
export function debounce(fn, ms = 150) {
  let t;
  return (...a) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...a), ms);
  };
}
