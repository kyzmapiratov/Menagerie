// Command palette (Ctrl+K): type a few letters and run an action without hunting
// through tabs. Characters are listed by name (Enter summons them); there are
// also screen switches, "Dismiss all", "Summon random" and so on.

import { $, el } from "./util.js";

let getItems = () => [];
let shown = [];
let idx = 0;

const fold = (s) => (s || "").toLowerCase();

function match(items, query) {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (!words.length) return items.slice(0, 14);

  return items
    .map((it) => {
      const hay = fold(`${it.title} ${it.keywords || ""}`);
      if (!words.every((w) => hay.includes(w))) return null;
      // An earlier match ranks higher; a match at a word start ranks higher still.
      const pos = hay.indexOf(words[0]);
      const boundary = pos === 0 || /[\s._-]/.test(hay[pos - 1]) ? 0 : 1;
      return { it, score: boundary * 100 + pos };
    })
    .filter(Boolean)
    .sort((a, b) => a.score - b.score)
    .slice(0, 40)
    .map((x) => x.it);
}

function render() {
  const list = $("palette-list");
  list.innerHTML = "";
  if (!shown.length) {
    list.appendChild(el("div", "palette-empty", "Nothing found"));
    return;
  }
  shown.forEach((it, i) => {
    const row = el("div", `palette-row ${i === idx ? "active" : ""}`);
    const text = el("div", "palette-text");
    text.appendChild(el("div", "palette-title", it.title));
    if (it.sub) text.appendChild(el("div", "palette-sub", it.sub));
    row.append(text, el("span", "palette-kind", it.kind || ""));
    row.addEventListener("mouseenter", () => {
      idx = i;
      list.querySelectorAll(".palette-row").forEach((r, j) => r.classList.toggle("active", j === i));
    });
    row.addEventListener("click", () => run(it));
    list.appendChild(row);
  });
  list.querySelector(".active")?.scrollIntoView({ block: "nearest" });
}

function run(it) {
  closePalette();
  it.run();
}

function refilter() {
  shown = match(getItems(), $("palette-input").value);
  idx = 0;
  render();
}

export function closePalette() {
  $("palette").classList.add("hidden");
}

export function openPalette() {
  $("palette").classList.remove("hidden");
  $("palette-input").value = "";
  refilter();
  $("palette-input").focus();
}

export function initPalette({ items }) {
  getItems = items;
  const input = $("palette-input");
  input.addEventListener("input", refilter);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") return closePalette();
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!shown.length) return;
      idx = (idx + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length;
      render();
    }
    if (e.key === "Enter" && shown[idx]) run(shown[idx]);
  });
  $("palette-bg").addEventListener("click", closePalette);
}
