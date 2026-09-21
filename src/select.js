// Selecting cards: one behaviour for the Collection and for catalog packs.
//
// A card shows a small circle when you hover it; clicking the circle selects the
// card. Once anything is selected, clicking a card selects it as well, and Shift
// extends the selection over a range. A bar floats at the bottom with what you
// can do with the selection, and Esc clears it. There is no "Select" mode to
// enter or leave.

import { $, el, withBusy } from "./util.js";

export class Selection {
  #items = new Map(); // key -> item
  #anchor = null; // key of the last plain click: where a Shift range starts
  #order;
  #onChange;

  /**
   * @param order     () => [{ key, item }] in the order the cards are on screen
   * @param onChange  called after every change
   */
  constructor({ order, onChange }) {
    this.#order = order;
    this.#onChange = onChange;
  }

  get size() {
    return this.#items.size;
  }
  has(key) {
    return this.#items.has(key);
  }
  keys() {
    return [...this.#items.keys()];
  }
  items() {
    return [...this.#items.values()];
  }

  toggle(key, item, { range = false } = {}) {
    if (range && this.#anchor !== null && this.#anchor !== key) {
      const list = this.#order();
      const a = list.findIndex((x) => x.key === this.#anchor);
      const b = list.findIndex((x) => x.key === key);
      if (a >= 0 && b >= 0) {
        const [from, to] = a < b ? [a, b] : [b, a];
        for (const x of list.slice(from, to + 1)) this.#items.set(x.key, x.item);
        this.#change();
        return;
      }
    }
    if (this.#items.has(key)) this.#items.delete(key);
    else this.#items.set(key, item);
    this.#anchor = key;
    this.#change();
  }

  /** Adds all of them, or, if they are all selected already, removes them. */
  toggleMany(entries) {
    const all = entries.every((x) => this.#items.has(x.key));
    for (const x of entries) {
      if (all) this.#items.delete(x.key);
      else this.#items.set(x.key, x.item);
    }
    this.#change();
  }

  selectAll() {
    for (const x of this.#order()) this.#items.set(x.key, x.item);
    this.#change();
  }

  clear() {
    if (!this.#items.size) return;
    this.#items.clear();
    this.#anchor = null;
    this.#change();
  }

  /** Drops selected keys that are no longer on screen (after a search or a delete). */
  prune() {
    const live = new Set(this.#order().map((x) => x.key));
    let dropped = false;
    for (const k of [...this.#items.keys()]) {
      if (!live.has(k)) {
        this.#items.delete(k);
        dropped = true;
      }
    }
    if (dropped) this.#change();
  }

  #change() {
    this.#onChange(this);
  }
}

/** The circle in the corner of a card. */
export function selDot(sel, key, item) {
  const b = el("button", "sel-dot");
  b.type = "button";
  b.title = "Select";
  b.setAttribute("aria-label", "Select");
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    sel.toggle(key, item, { range: e.shiftKey });
  });
  return b;
}

/** Marks the selected cards in place, without redrawing (and re-fetching) the sprites. */
export function paintSelection(container, sel) {
  container.querySelectorAll("[data-key]").forEach((card) => {
    const on = sel.has(card.dataset.key);
    card.classList.toggle("picked", on);
    card.setAttribute("aria-selected", on ? "true" : "false");
  });
}

// --------------------------------------------------------------- floating bar

/**
 * Shows the bar for the current selection, or hides it when nothing is selected.
 *   actions: [{ label, kind: "btn" | "danger" | "ghost", run, plain? }]  (`plain`: run(button) does its own waiting)
 */
export function showSelBar({ count, total, actions, onAll, onClear }) {
  const bar = $("selbar");
  document.body.classList.toggle("selecting", count > 0);
  bar.classList.toggle("hidden", count === 0);
  if (!count) return;

  $("selbar-count").textContent = `${count} selected`;

  const all = $("selbar-all");
  all.textContent = count < total ? `Select all ${total}` : "";
  all.classList.toggle("hidden", count >= total);
  all.onclick = onAll;

  const box = $("selbar-actions");
  box.replaceChildren(
    ...actions.map((a) => {
      const b = el("button", a.kind === "danger" ? "btn danger" : a.kind === "ghost" ? "ghost" : "btn", a.label);
      b.addEventListener("click", async () => {
        // While one action runs the others wait: Delete pressed in the middle of a Summon would act on the same selection.
        bar.classList.add("working");
        try {
          if (a.plain) await a.run(b);
          else await withBusy(b, a.run);
        } finally {
          bar.classList.remove("working");
        }
      });
      return b;
    })
  );
  $("selbar-clear").onclick = onClear;
}

export function hideSelBar() {
  document.body.classList.remove("selecting");
  $("selbar").classList.add("hidden");
}
