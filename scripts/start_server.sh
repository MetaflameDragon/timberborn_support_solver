#!/usr/bin/env bash
# Taken from the egui demo
# https://github.com/emilk/egui/blob/42c2fc58c90af92f19cc6bbda4b75ce7a290c0aa/scripts/build_demo_web.sh

set -eu
script_path=$( cd "$(dirname "${BASH_SOURCE[0]}")" ; pwd -P )
cd "$script_path/.."

# Starts a local web-server that serves the contents of the `web_demo/` folder,
# i.e. the web-version of `egui_demo_app`.

PORT=8765

echo "ensuring basic-http-server is installed…"
cargo install basic-http-server

echo "starting server…"
echo "serving at http://localhost:${PORT}"

(cd web_demo && basic-http-server --addr 0.0.0.0:${PORT} .)
# (cd web_demo && python3 -m http.server ${PORT} --bind 0.0.0.0)
