#!/usr/bin/env python3
"""A throw-away web server for the front end: `src/` as it is, plus a stand-in for the Tauri bridge.

    server.py PORT [--real]

It serves the app's own files untouched, and adds three scripts to index.html: the data (data.py),
the bridge (mock.js) and a few helpers (hover.js). Nothing here writes anywhere or talks to the network.
"""
import http.server, os, random, re, socketserver, sys, urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.realpath(os.path.join(HERE, "..", "..", "src"))
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8766
REAL = "--real" in sys.argv

sys.path.insert(0, HERE)
import data as datamod

DATA = datamod.as_script(datamod.real() if REAL else datamod.synthetic(), REAL).encode()
HOME = os.path.expanduser("~")
# Real mode shows the developer's own pictures, from the app's own folder and nowhere else.
SPRITES = next((os.path.realpath(p) + "/" for p in (f"{HOME}/.local/share/menagerie/sprites",) if os.path.isdir(p)), "/nonexistent/")
STATE = {"sprite_fail": 0.0, "sprite_500": 0, "sprite_ok": 0}
INJECT = b'<script src="/__h/data.js"></script><script src="/__h/mock.js"></script><script src="/__h/hover.js"></script>'
TYPES = {"js": "text/javascript", "css": "text/css", "html": "text/html", "png": "image/png", "svg": "image/svg+xml"}


class Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _send(self, code, body=b"", ctype="text/plain"):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        url = urllib.parse.urlparse(self.path)
        qs = urllib.parse.parse_qs(url.query)

        if url.path in ("/", "/index.html"):
            html = open(os.path.join(SRC, "index.html"), "rb").read().replace(b"</head>", INJECT + b"</head>", 1)
            return self._send(200, html, "text/html; charset=utf-8")

        if url.path == "/__h/data.js":
            return self._send(200, DATA, "text/javascript")
        if url.path in ("/__h/mock.js", "/__h/hover.js"):
            return self._send(200, open(os.path.join(HERE, os.path.basename(url.path)), "rb").read(), "text/javascript")

        # Frames of a catalog character (…/img/shime1.png …): drawn on the spot, twelve of them, so the animation preview has something to play.
        if url.path.startswith("/__f/"):
            m = re.match(r"/__f/([^/]+)/img/shime(\d+)\.png", url.path)
            if not m or int(m.group(2)) > 12:
                return self._send(404)
            n, slug = int(m.group(2)), urllib.parse.unquote(m.group(1))
            hue = (sum(map(ord, slug)) * 31) % 360
            dx = [0, 2, 4, 2, 0, -2, -4, -2, 0, 2, 4, 2][n - 1]
            svg = (f"<svg xmlns='http://www.w3.org/2000/svg' width='128' height='128'><rect x='{40 + dx}' y='16' width='48' height='44' rx='14' "
                   f"fill='hsl({hue} 60% 66%)'/><rect x='{46 + dx}' y='60' width='36' height='46' rx='9' fill='hsl({hue} 60% 55%)'/>"
                   f"<circle cx='{54 + dx}' cy='36' r='4' fill='#111'/><circle cx='{74 + dx}' cy='36' r='4' fill='#111'/></svg>")
            return self._send(200, svg.encode(), "image/svg+xml")

        # Knobs a test can turn: make a share of the picture requests fail.
        if url.path == "/__stats":
            return self._send(200, repr(STATE).encode())
        if url.path == "/__chaos":
            STATE["sprite_fail"] = float(qs.get("sprite", ["0"])[0])
            return self._send(200, b"ok")

        if url.path == "/sprite":
            path = os.path.realpath(qs.get("p", [""])[0])
            if not path.startswith(SPRITES) or not os.path.isfile(path):
                return self._send(404)
            if random.random() < STATE["sprite_fail"]:
                STATE["sprite_500"] += 1
                return self._send(500)
            STATE["sprite_ok"] += 1
            return self._send(200, open(path, "rb").read(), "image/png")

        f = os.path.realpath(os.path.join(SRC, url.path.lstrip("/")))
        if not f.startswith(SRC) or not os.path.isfile(f):
            return self._send(404)
        return self._send(200, open(f, "rb").read(), TYPES.get(f.rsplit(".", 1)[-1], "application/octet-stream"))


socketserver.TCPServer.allow_reuse_address = True
socketserver.ThreadingTCPServer.daemon_threads = True
with socketserver.ThreadingTCPServer(("127.0.0.1", PORT), Handler) as server:
    print(f"ready on {PORT}", flush=True)
    server.serve_forever()
