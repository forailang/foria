#!/usr/bin/env bash
set -e

PORT="${1:-3000}"
DIR="$(cd "$(dirname "$0")/dist/browser" && pwd)"

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required to run this server" >&2
  exit 1
fi

if [ ! -d "$DIR" ]; then
  echo "error: $DIR does not exist" >&2
  echo "run: cargo run -p forai -- build examples/web-simple-wasm" >&2
  exit 1
fi

echo "serving web-simple-wasm at http://localhost:$PORT (COOP/COEP enabled)"
python3 -c "
import http.server, functools, sys

class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/favicon.ico':
            self.send_response(204)
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        super().do_GET()

    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        super().end_headers()

s = http.server.HTTPServer(('', $PORT), functools.partial(Handler, directory='$DIR'))
try:
    s.serve_forever()
except KeyboardInterrupt:
    s.server_close()
    sys.exit(0)
"
