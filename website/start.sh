#!/usr/bin/env bash
# Serve the website locally for testing.  Usage: ./start.sh [port]
cd "$(dirname "$0")" || exit 1
PORT="${1:-8791}"
echo "Radial Launcher site → http://localhost:$PORT  (Ctrl-C to stop)"
exec python3 -m http.server "$PORT"
