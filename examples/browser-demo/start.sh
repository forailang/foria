#!/usr/bin/env bash
set -e

PORT="${1:-3000}"
DIR="$(cd "$(dirname "$0")/dist/browser" && pwd)"

echo "serving browser-demo at http://localhost:$PORT"
python3 -c "
import http.server, functools

class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        super().end_headers()

s = http.server.HTTPServer(('', $PORT), functools.partial(Handler, directory='$DIR'))
s.serve_forever()
"
