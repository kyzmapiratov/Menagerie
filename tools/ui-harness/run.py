#!/usr/bin/env python3
"""Checks the front end in the real WebKitGTK engine, without opening a window.

    python3 tools/ui-harness/run.py                 every suite, on made-up data
    python3 tools/ui-harness/run.py settings chaos  only these
    python3 tools/ui-harness/run.py --list          what there is
    python3 tools/ui-harness/run.py screenshots     the README pictures, from the invented data
    python3 tools/ui-harness/run.py --real screenshots   the same from your own collection (do not commit those)

It starts a small server (server.py) for `src/` with a stand-in for the Tauri bridge (mock.js),
runs each suite in suites/ through driver.py, and stops the server. Pictures a suite takes go to
tools/ui-harness/out/. Needs python-gobject and the WebKit2 4.1 typelib (Arch: python-gobject,
webkit2gtk-4.1; Debian: python3-gi gir1.2-webkit2-4.1); on a machine with no display it needs
xvfb-run in front of it. Exit status: 0 all passed, 1 something failed, 2 the engine is missing.
"""
import glob, json, os, socket, subprocess, sys, time, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
SUITES = sorted(glob.glob(os.path.join(HERE, "suites", "*.json")))
NAMES = [os.path.splitext(os.path.basename(p))[0] for p in SUITES]


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def main():
    args = sys.argv[1:]
    if "--list" in args:
        print("\n".join(NAMES))
        return 0
    real = "--real" in args
    wanted = [a for a in args if not a.startswith("--")]
    unknown = [w for w in wanted if w not in NAMES]
    if unknown:
        print(f"no such suite: {', '.join(unknown)}  (try --list)", file=sys.stderr)
        return 2
    # The pictures are not a check, and a plain run leaves them out.
    chosen = wanted or [n for n in NAMES if n != "screenshots"]

    port = free_port()
    server = subprocess.Popen([sys.executable, os.path.join(HERE, "server.py"), str(port)] + (["--real"] if real else []),
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        for _ in range(100):
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{port}/__stats", timeout=0.3)
                break
            except OSError:
                if server.poll() is not None:
                    print(server.stderr.read().decode(), file=sys.stderr)
                    return 2
                time.sleep(0.1)

        env = dict(os.environ, GDK_BACKEND="x11", WEBKIT_DISABLE_COMPOSITING_MODE="1", WEBKIT_DISABLE_DMABUF_RENDERER="1")
        out = os.path.join(HERE, "out")
        failed, began = [], time.time()
        for name in chosen:
            suite = json.load(open(os.path.join(HERE, "suites", f"{name}.json")))
            url = f"http://127.0.0.1:{port}/{suite.get('query', '')}"
            print(f"\n{name}", flush=True)
            result = subprocess.run([sys.executable, os.path.join(HERE, "driver.py"), url, os.path.join(HERE, "suites", f"{name}.json"), out],
                                    env=env)
            if result.returncode == 2:
                return 2
            if result.returncode != 0:
                failed.append(name)
        print(f"\n{len(chosen) - len(failed)} of {len(chosen)} suites passed in {time.time() - began:.0f} s" + (f"; failed: {', '.join(failed)}" if failed else ""))
        return 1 if failed else 0
    finally:
        server.terminate()
        server.wait()


if __name__ == "__main__":
    sys.exit(main())
