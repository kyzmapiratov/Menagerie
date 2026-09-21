// The first-run check: is the engine installed, and can this desktop run it at all?
//
// The commonest way a fresh install "does nothing" is that wl_shimeji, the program that
// actually draws the characters, is not there, or that the desktop cannot run it (GNOME
// has no layer-shell, an X11 session has no Wayland). Every command would say so in the
// end, one failed click at a time. This says it once, at the start, in words, with the
// commands that fix it, and then stays out of the way.

import { $, el, invoke, toast, store, clip } from "./util.js";

const ENGINE = "https://github.com/CluelessCatBurger/wl_shimeji";

const BUILD = [
  `git clone --recursive ${ENGINE}.git`,
  'cd wl_shimeji && make -j"$(nproc)" && make install PREFIX="$HOME/.local"',
];

// Building the engine needs a C compiler, make, git, Python 3.10+ with Pillow (for its
// own converter) and the development files of wayland, wayland-protocols, libarchive
// and uthash. Only the package names differ between distributions.
const HINTS = {
  arch: ["yay -S wl_shimeji-git   # or paru, or any AUR helper"],
  debian: [
    "sudo apt install build-essential git python3 python3-pil libwayland-dev libwayland-bin wayland-protocols libarchive-dev libuthash-dev",
    ...BUILD,
  ],
  fedora: [
    "sudo dnf install gcc make git python3 python3-pillow wayland-devel wayland-protocols-devel libarchive-devel uthash-devel",
    ...BUILD,
  ],
  suse: [
    "sudo zypper install gcc make git python3 python3-Pillow wayland-devel wayland-protocols-devel libarchive-devel uthash-devel",
    ...BUILD,
  ],
  nix: [`# wl_shimeji ships a flake: add it as an input`, `# ${ENGINE}`],
  unknown: [
    "# needs: a C compiler, make, git, Python 3.10+ with Pillow, and the development",
    "# files of wayland, wayland-protocols, libarchive and uthash",
    ...BUILD,
  ],
};

/**
 * What is wrong, if anything: { kind, title, text, commands? }. A pure function of the
 * facts the backend reports, so it can be tried against any desktop.
 */
export function diagnose(info) {
  if (!info.engine) {
    return {
      kind: "engine",
      title: "wl_shimeji is not installed",
      text: "This app finds and installs characters. A separate program, wl_shimeji, draws them on your screen, and it was not found. Install it once, then press Check again.",
      commands: HINTS[info.family] || HINTS.unknown,
    };
  }
  if (info.session === "x11") {
    return {
      kind: "x11",
      title: "This is an X11 session",
      text: "wl_shimeji draws through Wayland, so no character can appear here. Log out and choose a Wayland session on the login screen.",
    };
  }
  const desktop = (info.desktop || "").toLowerCase();
  const name = info.desktop ? info.desktop.split(":").pop() : "This desktop";

  // The compositor itself was asked what it supports. That beats guessing from its
  // name: a desktop with an extension that adds layer-shell works, and a compositor
  // nobody has heard of gets the right answer too.
  const p = info.protocols;
  if (p && p.layer_shell === false) {
    return {
      kind: "no-layer-shell",
      title: `${name} cannot show the characters`,
      text: "It does not offer wlr-layer-shell, the protocol wl_shimeji draws on. Nothing this app does can work around that. KDE Plasma, niri, sway and other wlroots-based desktops do offer it.",
    };
  }
  if (p && p.subcompositor === false) {
    return {
      kind: "no-subcompositor",
      title: `${name} cannot show the characters`,
      text: "It does not provide wl_subcompositor, which wl_shimeji needs to draw each character.",
    };
  }
  // Only when the compositor could not be asked at all.
  if (!p && desktop.includes("gnome")) {
    return {
      kind: "gnome",
      title: "GNOME cannot show the characters",
      text: "Its compositor (Mutter) does not offer the layer-shell protocol that wl_shimeji draws with. KDE Plasma, niri, sway and other wlroots-based desktops do.",
    };
  }
  // Hyprland has the protocols and still differs in how it clips what is drawn, so this
  // one cannot be settled by asking: it goes by name.
  if (desktop.includes("hyprland") || info.compositor === "hyprland") {
    return {
      kind: "hyprland",
      title: "Hyprland has one known limit with wl_shimeji",
      text: "The engine's authors list Hyprland as unsupported because it clips subsurfaces differently: a character can look cut off at its edges. Launch at login and shortcuts are set up here for Hyprland the way they are for niri.",
    };
  }
  return null;
}

const WARNING =
  '<svg class="alert-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3.6 2.9 19.4h18.2L12 3.6Z"/><path d="M12 10v4.5"/><circle cx="12" cy="17.2" r="0.7"/></svg>';

const seen = () => new Set(String(store.get("env-seen", "")).split(",").filter(Boolean));

function hide() {
  $("env-alert").classList.add("hidden");
}

function show(problem, recheck) {
  const box = $("env-alert");
  box.classList.add("calm");

  const icon = el("span");
  icon.innerHTML = WARNING;
  const body = el("div", "alert-body");
  body.append(el("div", "alert-title", problem.title), el("div", "alert-sub", problem.text));
  const close = el("button", "icon-btn", "✕");
  close.title = "Dismiss";
  close.addEventListener("click", () => {
    // A missing engine is worth repeating at every start; a desktop that cannot run it
    // is not going to change, so that one is said once.
    if (problem.kind !== "engine") store.set("env-seen", [...seen(), problem.kind].join(","));
    hide();
  });
  const head = el("div", "alert-head");
  head.append(icon.firstChild, body, close);

  const parts = [head];
  if (problem.commands) {
    const text = problem.commands.join("\n");
    const pre = el("pre", "pre hidden", text);
    const actions = el("div", "alert-actions");
    const how = el("button", "btn", "Show how");
    how.addEventListener("click", () => {
      pre.classList.toggle("hidden");
      how.textContent = pre.classList.contains("hidden") ? "Show how" : "Hide";
    });
    const copy = el("button", "link-btn", "Copy commands");
    copy.addEventListener("click", async () => {
      try {
        if (clip?.writeText) await clip.writeText(text);
        else await navigator.clipboard.writeText(text);
        toast("Commands copied", "ok");
      } catch {
        toast("Could not copy", "error");
      }
    });
    const again = el("button", "link-btn", "Check again");
    again.addEventListener("click", recheck);
    actions.append(how, copy, again);
    parts.push(actions, pre);
  }
  box.replaceChildren(...parts);
  box.classList.remove("hidden");
}

/** Looks at the system once and shows what is wrong, if anything. `onFixed` runs when a recheck finds the engine. */
export async function checkEnvironment(onFixed = () => {}) {
  let info;
  try {
    info = await invoke("environment_check");
  } catch {
    return;
  }
  if (!info || typeof info !== "object") return;

  const recheck = async () => {
    const now = await invoke("environment_check").catch(() => null);
    if (now?.engine) {
      hide();
      toast("wl_shimeji found", "ok");
      onFixed();
    } else {
      toast("Still not found. It has to be on the PATH (~/.local/bin counts).", "error");
    }
  };

  const problem = diagnose(info);
  if (!problem || seen().has(problem.kind)) {
    hide();
    return;
  }
  show(problem, recheck);
}
