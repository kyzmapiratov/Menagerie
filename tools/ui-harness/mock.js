// A stand-in for the Tauri bridge (window.__TAURI__), so the app's real JavaScript runs
// unchanged in a plain browser engine. It answers every command the interface uses, from the
// data in window.__DATA__ (see data.py), and has knobs for making things go wrong.
//
// Knobs, all on window: __lat[cmd] (extra milliseconds), __latAll, __failNext[cmd] (the next N
// calls throw), __chaos {listEmpty, listFail, sceneFail} (odds), __overlay (is the overlay alive),
// __env / __protocols / __niri (what the first-run check finds), __crash(), __errors (page errors).
// Query flags: ?presets=1 ?shots=1 ?kde=1 ?empty=1 ?lat=MS ?overlay=off ?noindex=1 ?p.KEY=VALUE
(() => {
  const R = window.__DATA__;
  window.__errors = [];
  const q = new URLSearchParams(location.search);
  const hue = (s) => { let h = 0; for (const ch of String(s)) h = (h * 31 + ch.charCodeAt(0)) % 360; return h; };
  const svg = (c) => "data:image/svg+xml," + encodeURIComponent(
    `<svg xmlns='http://www.w3.org/2000/svg' width='64' height='72'><rect x='16' y='6' width='32' height='30' rx='10' fill='${c}'/><rect x='20' y='36' width='24' height='30' rx='6' fill='${c}' opacity='.7'/><circle cx='26' cy='20' r='3' fill='#111'/><circle cx='38' cy='20' r='3' fill='#111'/></svg>`);
  const art = (name) => svg(`hsl(${hue(name)} 62% 66%)`);

  let mine = JSON.parse(JSON.stringify(R.mine));
  let autostart = JSON.parse(JSON.stringify(R.autostart));
  let config = JSON.parse(JSON.stringify(R.config)).map((c) => ({ ...c, live: true }));
  // Pictures for the README show the ordinary defaults, not whatever a test left behind.
  if (q.get("shots")) {
    const setv = (k, v) => { const c = config.find((x) => x.key === k); if (c) c.value = v; };
    setv("MASCOT_SCALE", "-1"); setv("INTERPOLATION_FRAMERATE", "-1"); setv("OPACITY", "-1"); setv("DRAGGING", "true");
  }
  let presets = q.get("shots") ? JSON.parse(JSON.stringify(R.shots.presets)) : q.get("presets") ? JSON.parse(JSON.stringify(R.presetsDemo)) : [];
  let scene = q.get("empty") ? [] : JSON.parse(JSON.stringify(q.get("shots") ? R.shots.scene : R.scene));
  const prefs = { ...R.prefs };
  if (q.get("fresh")) for (const k of ["cacho-guide-seen", "cacho-banner-closed"]) delete prefs[k];
  if (q.get("cache")) prefs["mine-cache"] = JSON.stringify(R.mine);
  for (const [k, v] of q.entries()) if (k.startsWith("p.")) prefs[k.slice(2)] = v;
  window.__prefs = prefs;

  window.__lat = {};       // command -> extra latency in ms
  window.__latAll = Number(q.get("lat") || 0);
  window.__failNext = {};  // command -> how many next calls throw
  window.__chaos = { listEmpty: 0, listFail: 0, sceneFail: 0 };
  window.__downloads = [];
  window.__stats = { listFail: 0, listEmpty: 0, listOk: 0, sceneFail: 0 };
  window.__calls = [];
  window.__toasts = [];
  window.__indexReady = q.get("noindex") ? false : true;
  window.__overlay = q.get("overlay") === "off" ? false : true;   // is the overlay process alive
  window.__desktop = q.get("desktop") || "niri";
  window.__opened = [];
  window.__trashed = [];
  // a crash: the overlay is gone and so is everyone on screen
  window.__crash = () => { window.__overlay = false; scene = []; };

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const rnd = () => Math.random();
  // Catalog characters have real frame addresses (…/img/shime1.png, shime2.png, …) served by the test server, 12 frames each.
  const packedChars = (slug) => (R.packChars[slug] || []).map((c) => ({ slug: c.slug, name: c.name, sprite: `/__f/${encodeURIComponent(c.slug)}/img/shime1.png`, pack_title: c.pack_title }));

  const convertFileSrc = (p) => (String(p).startsWith("synthetic:") ? art(String(p).slice(10)) : "/sprite?p=" + encodeURIComponent(p));

  const H = {
    prefs_load: () => ({ ...prefs }),
    prefs_set: ({ key, value }) => { prefs[key] = value; return null; },
    list_installed: () => {
      if (rnd() < window.__chaos.listFail) { window.__stats.listFail++; throw "`shimejictl prototypes list` failed"; }
      if (rnd() < window.__chaos.listEmpty) { window.__stats.listEmpty++; return []; }
      window.__stats.listOk++;
      return JSON.parse(JSON.stringify(mine));
    },
    match_sprites: () => "ok",
    fetch_packs: () => R.packs.map((p) => {
      const local = mine.filter((m) => m.pack_title === p.title && m.sprite_path).slice(0, 3).map((m) => convertFileSrc(m.sprite_path));
      const previews = local.length ? local : [art(p.title), art(p.title + "b"), art(p.title + "c")];
      return { slug: p.slug, title: p.title, character_count: p.character_count, preview_sprites: previews };
    }),
    fetch_characters: ({ packSlug }) => packedChars(packSlug),
    catalog_index: async () => {
      if (!window.__indexReady) await new Promise((res) => (window.__releaseIndex = res));
      return R.index.map((c) => ({ ...c, sprite: art(c.name), frames: [] }));
    },
    cachomon_index: () => ({ stale: false, entries: R.cacho.entries.map((e) => ({ ...e, thumb: window.__REAL__ && e.thumb ? e.thumb : art(e.name) })) }),
    recent_downloads: () => window.__downloads,
    downloads_path: () => "/home/demo/Downloads",
    open_downloads: () => { window.__opened.push("downloads"); return null; },
    overlay_status: () => ({ running: window.__overlay, ready: window.__overlay, pid: window.__overlay ? 4242 : null }),
    cancel_summon: () => { const had = !!window.__summoning; window.__summoning = false; window.__cancelled = (window.__cancelled || 0) + 1; return had; },
    tray_enabled: () => window.__tray !== false,
    set_tray_enabled: ({ on }) => { window.__tray = on; return null; },
    set_downloads_dir: ({ path }) => { window.__watch = path; return path; },
    overlay_log_full: ({ lines }) => Array.from({length: Math.min(lines||200, 40)}, (_,i)=>`[16:58:${String(i).padStart(2,"0")}][WARN] <Mascot:Shimeji.Sprout:${i}> Applying offset: 1919, 970`).join("\n"),
    autostart_status: () => window.__auto || { active: false, file: "/home/demo/.config/autostart/menagerie-characters.desktop", command: "" },
    autostart_enable: async ({ options }) => { await sleep(250); window.__auto = { active: true, file: "/home/demo/.config/autostart/menagerie-characters.desktop", command: "sh -c \"...\"" }; window.__autoOptions = options; return window.__auto; },
    autostart_disable: async () => { await sleep(200); window.__auto = { active: false, file: "/home/demo/.config/autostart/menagerie-characters.desktop", command: "" }; return window.__auto; },
    check_characters_folder: ({ path }) => {
      if (String(path).includes("busy")) throw "That folder has other things in it. Please choose an empty one (or make a new one).";
      return String(path).includes("old-home");
    },
    move_characters_folder: async ({ path }) => {
      await sleep(400);
      window.__movedTo = path;
      return { characters: 63, bytes: 62914560, from: "/home/demo/.local/share/wl_shimeji/shimejis", to: path, old_trashed: true, note: "" };
    },
    environment_check: () => window.__env || { engine: "/usr/bin/shimejictl", session: "wayland", desktop: q.get("kde") ? "KDE" : q.get("hypr") ? "Hyprland" : "niri", family: "arch",
      protocols: window.__protocols || { layer_shell: true, subcompositor: true, alpha_modifier: false, viewporter: true, all: ["wl_compositor"] },
      niri: window.__niri !== undefined ? window.__niri : !q.get("kde") && !q.get("hypr"),
      hyprland: !!q.get("hypr"), compositor: q.get("kde") ? "kde" : q.get("hypr") ? "hyprland" : "niri" },
    start_overlay: async () => { await sleep(300); window.__overlay = true; return 4242; },
    overlay_crashes: () => window.__crashList === "unavailable" ? Promise.reject("there is no crash list on this system") : (window.__crashList || []),
    repair_characters: () => window.__repaired || [],
    overlay_culprit: () => window.__culprit || null,
    overlay_log: () => "[01:13:29][WARN] Unhandled opcode 3c for object type 5",
    on_screen: () => {
      if (!window.__overlay) return [];
      if (rnd() < window.__chaos.sceneFail) { window.__stats.sceneFail++; throw "the overlay did not answer"; }
      return JSON.parse(JSON.stringify(scene));
    },
    summon_batch: async ({ items }) => {
      window.__summoning = true;
      const total = items.reduce((a, [, c]) => a + (c || 1), 0);
      for (let i = 0; i < 20; i++) {
        if (!window.__summoning) { window.__summoning = false; throw "Summoning stopped."; }
        await sleep(60);
      }
      window.__summoning = false;
      scene = items.map(([n, c]) => [n, c || 1]);
      window.__overlay = true;
      return { spawned: total, missing: [], restart_needed: [], slow_reason: window.__slow || null };
    },
    summon_mascot: async ({ name, count }) => H.summon_batch({ items: [[name, count || 1]] }).then(() => null),
    run_preset: async ({ name }) => { const p = presets.find((x) => x.name === name); const r = await H.summon_batch({ items: p.members }); return "Summoned " + r.spawned; },
    dismiss_all: () => { scene = []; window.__overlay = false; return "ok"; },
    open_folder: ({ which }) => { window.__opened.push(which); return null; },
    storage_info: () => ({ characters_dir: "/home/demo/.local/share/wl_shimeji/shimejis", characters_count: mine.length, characters_bytes: 56900000, characters_link: null, app_dir: "/home/demo/.local/share/menagerie", app_bytes: 1700000, downloads_dir: "/home/demo/Downloads" }),
    trash_archive: ({ path }) => { window.__trashed.push(path); return null; },
    dismiss_character: ({ name }) => { scene = scene.filter(([n]) => n !== name); return 1; },
    summoned_tally: () => [],
    get_autostart: () => JSON.parse(JSON.stringify(autostart)),
    set_autostart: ({ list }) => { autostart = list; return null; },
    list_presets: () => presets,
    save_preset: ({ name, members }) => { presets.push({ name, members }); return null; },
    delete_preset: ({ name }) => { presets = presets.filter((p) => p.name !== name); return null; },
    list_profiles: () => [], save_profile_from_current: () => null, delete_profile: () => null, apply_profile: () => null,
    config_list: () => JSON.parse(JSON.stringify(config)),
    config_set: ({ key, value }) => {
      if (window.__failNext > 0) { window.__failNext--; throw "the overlay is not responding"; }
      let v = value;
      if (key === "MASCOT_SCALE" && parseFloat(v) > 2) v = "2.000000";
      if (key === "WINDOW_THROW_POLICY") v = "looping";
      window.__config = window.__config || {}; window.__config[key] = v;
      return { value: v, adjusted: String(v).trim() !== String(value).trim() };
    },
    plugin_status: () => ({ desktop: window.__desktop, plugins: [] }),
    niri_status: () => ({ active: !!window.__niriActive, include_line: 'include "config.d/55-shimeji.kdl"', backup: "/x/config.kdl.bak", file: "/x/55-shimeji.kdl", validation: "ok" }),
    startup_snippet: () => "#!/bin/sh\n# Menagerie: bring the characters back at login.\nshimeji-overlayd &\nsleep 3\nshimejictl prototypes list | sed -n 's/^[0-9]*: //p' | shuf -n 3 | while IFS= read -r n; do shimejictl summon \"$n\"; done\n",
    niri_render: () => 'spawn-at-startup "shimeji-overlayd"\nbinds {\n    Mod+Ctrl+D { spawn "x"; }\n}',
    niri_conflicts: () => [],
    niri_apply: () => ({ active: true, validation: "ok" }),
    niri_enable: () => { window.__niriActive = true; return { active: true, validation: "ok" }; },
    niri_disable: () => { window.__niriActive = false; return { active: false }; },
    // Hyprland: window.__hyprCalls lists what the page asked for; __hyprValidation / __hyprSkipped are set by a test.
    hyprland_status: () => ({ active: !!window.__hyprActive, include_line: "source = /x/hypr/menagerie.conf", backup: "/x/hypr/hyprland.conf.menagerie-backup", file: "/x/hypr/menagerie.conf", validation: window.__hyprActive ? (window.__hyprValidation || "ok") : "", skipped: window.__hyprActive ? (window.__hyprSkipped || []) : [] }),
    hyprland_render: () => "exec-once = shimeji-overlayd\nbind = SUPER CTRL, D, exec, shimejictl stop\n",
    hyprland_conflicts: ({ keys }) => (window.__hyprConflicts || []).filter((c) => keys.includes(c.keys)),
    hyprland_apply: async ({ options }) => { (window.__hyprCalls = window.__hyprCalls || []).push(["apply", options]); if (window.__hyprReject) throw window.__hyprReject; return { active: true, validation: window.__hyprValidation || "ok", skipped: window.__hyprSkipped || [], file: "/x/hypr/menagerie.conf" }; },
    hyprland_enable: async ({ options }) => { await sleep(200); (window.__hyprCalls = window.__hyprCalls || []).push(["enable", options]); window.__hyprActive = true; return { active: true, validation: window.__hyprValidation || "ok", skipped: window.__hyprSkipped || [], file: "/x/hypr/menagerie.conf", include_line: "source = /x/hypr/menagerie.conf", backup: "/x/hypr/hyprland.conf.menagerie-backup" }; },
    hyprland_disable: async () => { await sleep(150); (window.__hyprCalls = window.__hyprCalls || []).push(["disable"]); window.__hyprActive = false; return { active: false, validation: "", skipped: [] }; },
    character_frames: ({ name }) => [art(name), art(name + "x"), art(name)],
    cancel_install: () => { window.__exportCancelled = true; return null; },
    install_characters: ({ characters }) => {
      for (const c of characters) if (!mine.some((m) => m.name === c.name)) mine.push({ name: c.name.replace(/ /g, "_"), sprite_path: "", pack_title: c.pack_title, artist: "", favorite: false, installed_at: Math.floor(Date.now() / 1000), size_bytes: 500000 });
      return { installed: characters.map((c) => c.name), skipped: [], failed: [], cancelled: [] };
    },
    prepare_local_archive: () => window.__archiveNames || ["Some Newcomer"],
    install_local_selected: (a) => {
      window.__installArgs = a;
      // What was installed is in the collection afterwards, as it is for real.
      if (window.__archiveAdds) for (const n of a.selection) if (!mine.some((x) => x.name === n)) mine.push({ name: n, sprite_path: "", pack_title: "", artist: "", favorite: false, installed_at: Math.floor(Date.now() / 1000), size_bytes: 400000 });
      return window.__installSaid || "Installed: Some Newcomer";
    },
    cancel_local: () => null,
    remove_many: ({ names }) => { mine = mine.filter((m) => !names.includes(m.name)); return `Deleted ${names.length}`; },
    remove_mascot: ({ name }) => { mine = mine.filter((m) => m.name !== name); return "ok"; },
    toggle_favorite: ({ name }) => { const m = mine.find((x) => x.name === name); m.favorite = !m.favorite; return m.favorite; },
    export_collection: async (a) => {
      window.__lastExport = a;
      const label = a.names ? `Exporting ${a.names.length} character${a.names.length === 1 ? "" : "s"}` : "Exporting your collection";
      const say = (stage, fraction, message) => window.__emit("install-progress", { key: "export", name: label, index: 0, total: 1, stage, fraction, message });
      const n = a.names ? a.names.length : (window.__exportCount || 6);
      window.__exportCancelled = false;
      say("export", 0, "Starting…");
      for (let i = 0; i < n; i++) {
        if (window.__exportCancelled) { say("cancelled", 0, "Stopped"); throw "Export stopped."; }
        say("export", i / n, `Character ${i + 1} · ${i + 1} of ${n}`);
        await sleep(window.__exportStep ?? 350);
      }
      say("export", 0.99, "Writing the archive…");
      say("done", 1, `Exported ${n} characters to /tmp/x.zip (1.0 MB)`);
      return `Exported ${n} characters to /tmp/x.zip (1.0 MB)`;
    },
  };

  const listeners = {};
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        window.__calls.push(cmd);
        if (cmd === "catalog_index" && args && args.refresh) window.__indexRefreshes = (window.__indexRefreshes || 0) + 1;
        const wait = cmd.startsWith("prefs_") ? 0 : (window.__lat[cmd] ?? window.__latAll); // reading a small file is quick
        if (wait) await sleep(wait);
        if (window.__failNext[cmd] > 0) { window.__failNext[cmd]--; throw `${cmd} failed (test)`; }
        if (!(cmd in H)) { window.__errors.push("unmocked command: " + cmd); return null; }
        return H[cmd](args || {});
      },
      convertFileSrc,
    },
    dialog: { open: async (o) => { window.__dialogArgs = o; return window.__pickPath ?? null; }, save: async () => "/tmp/x.zip" },
    clipboardManager: { writeText: async () => {} },
    event: { listen: async (n, cb) => { (listeners[n] ||= []).push(cb); return () => {}; } },
    opener: { openUrl: async (u) => { window.__opened = u; } },
  };
  window.__emit = (n, payload) => (listeners[n] || []).forEach((cb) => cb({ payload }));
  window.__setMine = (fn) => { mine = fn(mine); };
  window.__setScene = (s) => { scene = s; };
  window.addEventListener("error", (e) => window.__errors.push(e.message + " @" + e.filename + ":" + e.lineno));
  window.addEventListener("unhandledrejection", (e) => window.__errors.push("rejection: " + (e.reason && (e.reason.message || e.reason))));
})();
