// Dropping a file on the window.
//
// The usual way in is a file chooser, and a file chooser is a small tour of somebody
// else's folder tree. A downloaded archive is almost always already visible in a file
// manager or a browser's download list, so dragging it onto this window and letting go
// is both shorter and more obvious. Anything the app can install is accepted: a
// Shimeji-EE archive, a single character, or a whole collection exported from here.

import { $, el, events, toast } from "./util.js";

const TAKES = [".zip", ".wlshm"];

const accepted = (paths) => (paths || []).filter((p) => TAKES.some((ext) => p.toLowerCase().endsWith(ext)));

function sheet() {
  let box = $("drop-veil");
  if (!box) {
    box = el("div", "drop-veil hidden");
    box.id = "drop-veil";
    const card = el("div", "drop-card");
    card.append(
      el("div", "drop-mark", "＋"),
      el("div", "drop-title", "Drop to install"),
      el("div", "drop-sub", "A Shimeji archive, a single character, or a collection exported from here"),
    );
    box.appendChild(card);
    document.body.appendChild(box);
  }
  return box;
}

/**
 * `install` is called with the path of the dropped file. Nothing happens on a system
 * where the webview has no drag-and-drop events: the file chooser is still there.
 */
export async function initDragDrop(install) {
  const webview = window.__TAURI__?.webview?.getCurrentWebview?.();
  if (!webview?.onDragDropEvent) return;

  const veil = sheet();
  let showing = false;
  const hide = () => {
    showing = false;
    veil.classList.add("hidden");
  };

  try {
    await webview.onDragDropEvent(({ payload }) => {
      if (payload.type === "enter") {
        // Something is being dragged in that this app cannot open: say nothing, so the
        // person can still drop it on whatever is behind us.
        if (!accepted(payload.paths).length) return;
        showing = true;
        veil.classList.remove("hidden");
      } else if (payload.type === "leave") {
        hide();
      } else if (payload.type === "drop") {
        if (!showing) return;
        hide();
        const files = accepted(payload.paths);
        if (!files.length) return;
        if (files.length > 1) {
          toast("One archive at a time, please", "error");
          return;
        }
        install(files[0]);
      }
    });
  } catch {
    // No drag-and-drop here; the button in the toolbar still opens a chooser.
  }

  // A drop that never arrives (dragged away over another window) leaves the sheet up.
  window.addEventListener("blur", hide);
}

/** Lets the Collection's "export" flow explain the round trip. */
export const dropTargets = TAKES;
