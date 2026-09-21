"""The data the harness feeds the app.

By default it is made up: forty-eight invented characters in a few invented universes, an
invented catalog and an invented list of engine settings. Nothing personal, nothing
copyrighted, the same on every machine, so a test that passes here passes there.

`--real` reads the developer's own collection instead (read-only), for taking pictures of the
app as it really looks.
"""
import json, os, re, subprocess, time

NAMES = """Mochi Pixel Sprout Nimbus Biscuit Pebble Marlow Juno Tofu Ember Clover Waffle Comet Pumpkin
Noodle Fable Rusty Sable Tinsel Otter Bramble Cinder Dot Echo Fudge Gizmo Hazel Iggy Jelly Kiwi Lumen
Mango Nori Opal Pip Quill Rumi Sunny Toast Umber Velvet Wisp Yuzu Zephyr Acorn Button Cocoa Dewdrop
Fizz Maple""".split()

UNIVERSES = ["Garden Friends", "Kitchen Crew", "Space Cadets", "Weather Sprites", "Tiny Monsters", "Forest Folk"]

# The engine's own settings, as `shimejictl config list` reports them.
CONFIG = [
    ("Multiplication", "BREEDING", "false"), ("Dragging", "DRAGGING", "true"),
    ("Window Interactions", "WINDOW_INTERACTIONS", "false"), ("Window Throwing", "WINDOW_THROWING", "true"),
    ("Window Throw Policy", "WINDOW_THROW_POLICY", "looping"), ("Cursor Position", "CURSOR_POSITION", "true"),
    ("Mascot Limit", "MASCOT_LIMIT", "512"), ("Allow Throwing Multihead", "ALLOW_THROWING_MULTIHEAD", "true"),
    ("Allow Dragging Multihead", "ALLOW_DRAGGING_MULTIHEAD", "true"), ("Unified Outputs", "UNIFIED_OUTPUTS", "true"),
    ("Dismiss Animations", "DISMISS_ANIMATIONS", "true"), ("Affordances", "AFFORDANCES", "true"),
    ("Interpolation Framerate", "INTERPOLATION_FRAMERATE", "-1"), ("Overlay layer", "WLR_SHELL_LAYER", "overlay"),
    ("Tablets Enabled", "TABLETS_ENABLED", "true"), ("Left Button", "POINTER_LEFT_BUTTON", "1"),
    ("Right Button", "POINTER_RIGHT_BUTTON", "2"), ("Middle Button", "POINTER_MIDDLE_BUTTON", "4"),
    ("On Tool Pen Button", "ON_TOOL_PEN", "1"), ("On Tool Eraser Button", "ON_TOOL_ERASER", "1"),
    ("On Tool Brush Button", "ON_TOOL_BRUSH", "1"), ("On Tool Pencil Button", "ON_TOOL_PENCIL", "1"),
    ("On Tool Airbrush Button", "ON_TOOL_AIRBRUSH", "1"), ("On Tool Finger Button", "ON_TOOL_FINGER", "1"),
    ("On Tool Lens Button", "ON_TOOL_LENS", "1"), ("On Tool Mouse Button", "ON_TOOL_MOUSE", "1"),
    ("On Tool Button 1", "ON_TOOL_BUTTON1", "2"), ("On Tool Button 2", "ON_TOOL_BUTTON2", "2"),
    ("On Tool Button 3", "ON_TOOL_BUTTON3", "2"), ("Opacity", "OPACITY", "-1"), ("Scaling", "MASCOT_SCALE", "-1"),
]


def synthetic():
    now = int(time.time())
    mine = []
    for i, name in enumerate(NAMES):
        mine.append({
            "name": name, "sprite_path": f"synthetic:{name}", "pack_title": UNIVERSES[i % len(UNIVERSES)],
            "artist": "", "favorite": i % 9 == 0,
            # the first eight are "recently added"; the rest are spread over the past months
            "installed_at": now - (i * 1800 if i < 8 else 86400 * (3 + i)), "size_bytes": 480_000 + (i * 37_000) % 900_000,
        })

    # A catalog: one enormous pack (the thing that used to lag on first selection), some middling, some small.
    counts = [2337, 240, 120, 96, 64, 48, 40, 36, 30, 24, 18, 12, 8, 6]
    titles = ["Open uploads", "Garden Friends", "Kitchen Crew", "Space Cadets", "Weather Sprites", "Tiny Monsters", "Forest Folk",
              "Sea Life", "Robot Club", "Night Shift", "Snack Bar", "Old Friends", "Winter Set", "Odd Ones"]
    packs, index, pack_chars = [], [], {}
    for title, n in zip(titles, counts):
        slug = re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-") + "-pack"
        chars = []
        for j in range(n):
            base = NAMES[(j * 7 + len(title)) % len(NAMES)]
            name = f"{base} {j + 1}" if n > len(NAMES) else f"{base} {chr(65 + j % 26)}{j // 26 or ''}"
            chars.append({"slug": f"{slug}-{j:04d}", "name": name, "pack_title": title})
            index.append({"slug": f"{slug}-{j:04d}", "name": name, "pack_slug": slug, "pack_title": title})
        pack_chars[slug] = chars
        packs.append({"slug": slug, "title": title, "character_count": str(n)})

    franchises = ["Cake Kingdom", "Deep Space", "Old Library", "Night Market", "Moon Garden", "Clockwork Town"]
    entries = []
    for i in range(42):
        entries.append({
            "id": str(400 + i), "name": f"{NAMES[(i * 5) % len(NAMES)]} {i}", "franchise": franchises[i % len(franchises)],
            "thumb": "", "artist": ["Aki", "Bo", "Cleo", "Dara"][i % 4], "availability": "public",
            "downloads": str(10 + i * 3), "complexity": ["Easy", "Medium", "Hard"][i % 3], "gender": "",
            "features": "['Hotspots']", "url": f"https://example.invalid/shimeji.php?id={400 + i}",
        })

    return {
        "mine": mine, "packs": packs, "packChars": pack_chars, "index": index, "cacho": {"entries": entries},
        "config": [{"label": l, "key": k, "value": v, "live": True} for l, k, v in CONFIG],
        # The two first-run banners are answered, so the pages under test are the pages themselves.
        "prefs": {"cacho-guide-seen": "1", "cacho-banner-closed": "1", "cacho-auto": "1", "cacho-trash": "1", "catalog-source": "xyz"},
        "autostart": [["Mochi", 1], ["Pixel", 2]],
        # Presets that stress the layout: a one-letter name, and one too long for its card.
        "presetsDemo": [
            {"name": "ц", "members": [["Mochi", 1], ["Pixel", 1], ["Sprout", 1], ["Nimbus", 1], ["Biscuit", 2]]},
            {"name": "Morning crew with a rather long name that will not fit", "members": [[n, 1] for n in NAMES[:24]]},
            {"name": "Work desk", "members": [["Mochi", 1], ["Pixel", 2], ["Sprout", 1]]},
            {"name": "Chaos", "members": [["Comet", 1], ["Fable", 1], ["Rusty", 1], ["Tinsel", 1], ["Otter", 2]]},
        ],
        "scene": [["Mochi", 1], ["Pixel", 2], ["Sprout", 1]],
        # What the README pictures use: tidy names and a believable crowd.
        "shots": {
            "presets": [
                {"name": "Work desk", "members": [["Mochi", 1], ["Pixel", 2], ["Sprout", 1], ["Nimbus", 1]]},
                {"name": "Morning crew", "members": [["Biscuit", 1], ["Pebble", 1], ["Marlow", 1], ["Juno", 2], ["Tofu", 1]]},
                {"name": "Everyone", "members": [[n, 1] for n in NAMES[:10]]},
            ],
            "scene": [["Mochi", 1], ["Pixel", 2], ["Sprout", 2], ["Nimbus", 1], ["Biscuit", 1], ["Pebble", 1], ["Marlow", 1], ["Juno", 1]],
        },
    }


def real():
    """The developer's own data, read-only. Same shape as `synthetic()`."""
    home = os.path.expanduser("~")
    data = next((p for p in (f"{home}/.local/share/menagerie",) if os.path.isdir(p)), None)
    if not data:
        raise SystemExit("--real needs the app's data folder (~/.local/share/menagerie); run the app once first")
    wl = f"{home}/.local/share/wl_shimeji"
    base = synthetic()
    lib = json.load(open(f"{data}/library.json"))["entries"]
    names = []
    for d in ("prototypes", "shimejis"):
        p = f"{wl}/{d}"
        for e in sorted(os.listdir(p)) if os.path.isdir(p) else []:
            if os.path.isdir(os.path.join(p, e)):
                n = e[len("Shimeji."):] if e.startswith("Shimeji.") else e
                if n and n not in names:
                    names.append(n)
    def folder(n):
        return next((c for c in (f"{wl}/shimejis/{n}", f"{wl}/shimejis/Shimeji.{n}", f"{wl}/prototypes/{n}") if os.path.isdir(c)), None)

    def size(path):
        return sum(os.path.getsize(os.path.join(r, f)) for r, _, fs in os.walk(path) for f in fs if os.path.isfile(os.path.join(r, f)))

    mine = []
    for n in names:
        e = lib.get(n) or next((v for k, v in lib.items() if n.endswith(k) or k.endswith(n)), None) or {}
        d = folder(n)
        mine.append({"name": n, "sprite_path": e.get("sprite_path", ""), "pack_title": e.get("pack_title", ""), "artist": e.get("artist", ""),
                     "favorite": bool(e.get("favorite")), "installed_at": int(os.path.getmtime(d)) if d else 0, "size_bytes": size(d) if d else 0})
    base["mine"] = mine
    # Pictures of the app use the developer's own characters, when they have the usual ones.
    have = {m["name"] for m in mine}
    pick = [n for n in ("Hornet", "BMO", "Sonic", "Finn", "Jake", "Marceline", "Pomni", "Thor", "Loki", "Bill_Cipher", "Zooble") if n in have]
    pick = pick if len(pick) >= 6 else [m["name"] for m in mine[:12]]
    base["shots"] = {
        "presets": [
            {"name": "Work desk", "members": [[n, 1] for n in pick[:4]]},
            {"name": "Morning crew", "members": [[n, 1] for n in pick[2:7]]},
            {"name": "Everyone", "members": [[n, 1] for n in pick[:10]]},
        ],
        "scene": [[n, 2 if i % 3 == 1 else 1] for i, n in enumerate(pick[:8])],
    }
    try:
        index = json.load(open(f"{data}/catalog-index.json"))
        base["index"] = [{"slug": c["slug"], "name": c["name"], "pack_slug": c["pack_slug"], "pack_title": c["pack_title"]} for c in index]
        order, chars = [], {}
        for c in index:
            if c["pack_slug"] not in chars:
                chars[c["pack_slug"]] = []; order.append((c["pack_slug"], c["pack_title"]))
            chars[c["pack_slug"]].append({"slug": c["slug"], "name": c["name"], "pack_title": c["pack_title"]})
        base["packChars"] = chars
        base["packs"] = [{"slug": s, "title": t, "character_count": str(len(chars[s]))} for s, t in order]
        base["cacho"] = json.load(open(f"{data}/cachomon-index.json"))
    except (OSError, ValueError):
        pass
    return base


def as_script(data, real_mode):
    return f"window.__DATA__ = {json.dumps(data)}; window.__REAL__ = {'true' if real_mode else 'false'};"
