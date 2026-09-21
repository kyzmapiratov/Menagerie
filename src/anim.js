// Hover animation: while the mouse is anywhere over a card, the character plays
// through its frames; when it leaves, the card goes back to the still picture.
//
// Kept cheap on purpose:
//   • nothing is loaded until you actually rest on a card (short delay), so
//     sweeping the mouse across a grid costs nothing;
//   • one animation at a time: starting one stops the previous;
//   • ~5 frames per second, one timer, and the timer stops on mouse-out;
//   • frame lists are cached in memory (the 24 most recent characters).

import { invoke } from "./util.js";

const DELAY_MS = 140; // rest on a card this long before anything starts
const FRAME_MS = 200; // ~5 frames per second: a relaxed walk, not a blur
const MAX_FRAMES = 46; // the classic Shimeji set
const CACHE_SIZE = 24;

const cache = new Map(); // key -> Promise<string[]>

function remember(key, make) {
  if (cache.has(key)) {
    const hit = cache.get(key);
    cache.delete(key); // move to the end: most recently used
    cache.set(key, hit);
    return hit;
  }
  const promise = make().catch(() => []);
  cache.set(key, promise);
  while (cache.size > CACHE_SIZE) cache.delete(cache.keys().next().value);
  return promise;
}

const loadImage = (url) =>
  new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = url;
  });

/**
 * Frames of a catalog character (shimejis.xyz): `.../img/shime1.png`,
 * `shime2.png`, ... The addresses are predictable, so we probe them in batches
 * and stop once a whole batch is missing.
 */
export function remoteFrames(spriteUrl) {
  const base = spriteUrl.replace(/shime\d+\.png$/, "");
  return remember(`r:${base}`, async () => {
    const urls = [];
    for (let start = 1; start <= MAX_FRAMES; start += 12) {
      const batch = [];
      for (let n = start; n < start + 12 && n <= MAX_FRAMES; n++) batch.push(`${base}shime${n}.png`);
      const loaded = await Promise.all(batch.map(loadImage));
      const ok = batch.filter((_, i) => loaded[i]);
      urls.push(...ok);
      if (!ok.length) break;
    }
    return urls;
  });
}

/** Frames of an installed character, read from its files on disk. */
export function localFrames(name) {
  return remember(`l:${name}`, () => invoke("character_frames", { name }));
}

let active = null; // the animation currently playing: { stop() }

/**
 * Plays `getFrames()` on `img` while the mouse is over `host`.
 * `getFrames` returns a promise of a list of image URLs.
 */
export function attachHover(host, img, getFrames) {
  if (!host || !img) return;
  const still = img.src;
  let starter = null;
  let timer = null;
  let hovering = false;
  let playing = false;

  const stop = () => {
    clearTimeout(starter);
    clearInterval(timer);
    starter = timer = null;
    // Only touch the picture if we actually changed it: a quick pass of the
    // mouse must leave the card completely alone.
    if (playing) img.src = still;
    playing = false;
    if (active && active.stop === stop) active = null;
  };

  host.addEventListener("mouseenter", () => {
    hovering = true;
    clearTimeout(starter);
    starter = setTimeout(async () => {
      const frames = await getFrames();
      // The mouse may have left (or the card been redrawn) while frames loaded.
      if (!hovering || !img.isConnected || frames.length < 2) return;

      if (active) active.stop();
      active = { stop };

      let i = 0;
      playing = true;
      timer = setInterval(() => {
        if (!img.isConnected) return stop();
        i = (i + 1) % frames.length;
        img.src = frames[i];
      }, FRAME_MS);
    }, DELAY_MS);
  });

  host.addEventListener("mouseleave", () => {
    hovering = false;
    stop();
  });
}
