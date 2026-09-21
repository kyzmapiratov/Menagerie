#!/usr/bin/env python3
"""Runs one suite against the REAL WebKitGTK 4.1 — the engine Tauri renders with — off screen.

    driver.py URL SUITE.json OUTDIR

No window is ever shown: a Gtk.OffscreenWindow holds the WebView. A suite is a JSON file:

    {"size": [1100, 760], "query": "?presets=1",
     "steps": [
        {"js": "return {n: document.querySelectorAll('.pack').length}", "label": "packs", "expect": {"n": {"min": 1}}},
        {"wait": 500},
        {"shot": "name.png"},
        {"size": [900, 700]}
     ]}

A `js` step (a string, or a list of lines) is the body of an async function; what it returns (anything JSON can hold) is checked against
`expect`, when there is one:
    "key": value                  equal
    "key": {"min": 3, "max": 9}   a range          "key": {"not": 0}       anything but
    "key": {"has": "text"}        contains          "key": {"re": "^x"}     matches
    "key": {"every": {...}}       every item of a list matches
Exit status: 0 all good, 1 something failed, 2 the engine could not be started.
"""
import json, os, re, sys, warnings

warnings.simplefilter("ignore")
try:
    import gi
    gi.require_version("Gtk", "3.0")
    gi.require_version("WebKit2", "4.1")
    from gi.repository import GLib, Gtk, WebKit2
except (ImportError, ValueError) as e:
    print(f"cannot start WebKitGTK 4.1 ({e}); need python-gobject and the WebKit2 4.1 typelib", file=sys.stderr)
    sys.exit(2)

url, suite_path, outdir = sys.argv[1], sys.argv[2], sys.argv[3]
suite = json.load(open(suite_path))
steps = suite["steps"]
W, H = suite.get("size", [1100, 760])
os.makedirs(outdir, exist_ok=True)

win = Gtk.OffscreenWindow()
win.set_default_size(W, H)
view = WebKit2.WebView.new_with_context(WebKit2.WebContext.new_ephemeral())
view.set_size_request(W, H)
view.get_settings().set_enable_write_console_messages_to_stdout(True)
win.add(view)
win.show_all()

pos = [0]
failures = []
name = os.path.splitext(os.path.basename(suite_path))[0]


def fail(label, why):
    failures.append(f"{label}: {why}")
    print(f"  FAIL  {label}: {why}", flush=True)


def matches(actual, want, where):
    """Yields a reason for every way `actual` differs from `want`."""
    if isinstance(want, dict) and set(want) & {"min", "max", "not", "has", "re", "every"}:
        for op, arg in want.items():
            if op == "min" and not (isinstance(actual, (int, float)) and actual >= arg): yield f"{where} is {actual!r}, wanted at least {arg}"
            elif op == "max" and not (isinstance(actual, (int, float)) and actual <= arg): yield f"{where} is {actual!r}, wanted at most {arg}"
            elif op == "not" and actual == arg: yield f"{where} is {actual!r}, wanted anything else"
            elif op == "has" and not (arg in actual if isinstance(actual, (str, list)) else False): yield f"{where} is {actual!r}, wanted it to contain {arg!r}"
            elif op == "re" and not (isinstance(actual, str) and re.search(arg, actual)): yield f"{where} is {actual!r}, wanted it to match {arg!r}"
            elif op == "every":
                if not isinstance(actual, list): yield f"{where} is {actual!r}, wanted a list"
                else:
                    for i, item in enumerate(actual): yield from matches(item, arg, f"{where}[{i}]")
        return
    if isinstance(want, dict):
        if not isinstance(actual, dict): yield f"{where} is {actual!r}, wanted an object"; return
        for k, v in want.items():
            yield from matches(actual.get(k), v, f"{where}.{k}" if where else k)
        return
    if actual != want:
        yield f"{where} is {actual!r}, wanted {want!r}"


def finish():
    ok = not failures
    print(f"{'PASS' if ok else 'FAIL'}  {name}" + ("" if ok else f"  ({len(failures)} problem{'s' if len(failures) != 1 else ''})"), flush=True)
    Gtk.main_quit()
    globals()["exit_code"] = 0 if ok else 1


def poll(label, expect, tries=[0]):
    def check():
        view.run_javascript("window.__r === undefined ? null : window.__r", None, on_poll)
        return False

    def on_poll(v, res):
        try:
            val = v.run_javascript_finish(res).get_js_value()
            text = None if val.is_null() or val.is_undefined() else val.to_string()
        except Exception as e:
            text = json.dumps({"error": f"poll failed: {e}"})
        if text is None:
            tries[0] += 1
            if tries[0] > 1500:
                tries[0] = 0
                fail(label, "the step never finished (timeout)")
                return step()
            GLib.timeout_add(40, check)
            return
        tries[0] = 0
        try:
            result = json.loads(text)
        except ValueError:
            result = text
        if isinstance(result, dict) and "error" in result and len(result) == 1:
            fail(label, f"the step threw: {str(result['error'])[:300]}")
        elif expect is not None:
            reasons = list(matches(result, expect, ""))
            if reasons:
                for r in reasons: fail(label, r)
            else:
                print(f"  ok    {label}", flush=True)
        else:
            print(f"  ..    {label}: {text[:200]}", flush=True)
        step()
    check()


def run_js(code, label, expect):
    wrapped = ("window.__r = undefined;(async()=>{try{window.__r=JSON.stringify(await (async()=>{" + code +
               "})());}catch(e){window.__r=JSON.stringify({error:String(e&&e.stack||e)});}})();0;")

    def after(v, r):
        try: v.run_javascript_finish(r)
        except Exception as e: fail(label, f"could not start: {e}")
        poll(label, expect)
    view.run_javascript(wrapped, None, after)


def shot(file):
    def cb(v, res):
        try:
            v.get_snapshot_finish(res).write_to_png(os.path.join(outdir, file))
            print(f"  shot  {os.path.join(outdir, file)}", flush=True)
        except Exception as e:
            fail(f"shot {file}", str(e))
        step()
    view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, cb)


def step():
    if pos[0] >= len(steps):
        return finish()
    s = steps[pos[0]]
    pos[0] += 1
    if "wait" in s: GLib.timeout_add(s["wait"], lambda: (step(), False)[1])
    elif "size" in s:
        w, h = s["size"]
        view.set_size_request(w, h); win.set_default_size(w, h); win.resize(w, h)
        GLib.timeout_add(250, lambda: (step(), False)[1])
    elif "shot" in s: shot(s["shot"])
    elif "js" in s:
        code = "\n".join(s["js"]) if isinstance(s["js"], list) else s["js"]   # a list of lines reads better in JSON
        run_js(code, s.get("label", f"step {pos[0]}"), s.get("expect"))
    else: step()


def on_load(v, ev):
    if ev == WebKit2.LoadEvent.FINISHED:
        GLib.timeout_add(int(os.environ.get("HARNESS_SETTLE", "900")), lambda: (step(), False)[1])


exit_code = 1
view.connect("load-changed", on_load)
view.connect("load-failed", lambda v, ev, uri, err: fail("load", f"{uri}: {err.message}") or False)
view.connect("web-process-terminated", lambda v, r: fail("web process", f"terminated ({r.value_nick})"))
GLib.timeout_add_seconds(int(os.environ.get("HARNESS_TIMEOUT", "180")), lambda: (fail("suite", "timed out"), finish(), False)[2])
view.load_uri(url)
Gtk.main()
sys.exit(exit_code)
