#!/usr/bin/env bash
# Taken from the egui demo
# https://github.com/emilk/egui/blob/42c2fc58c90af92f19cc6bbda4b75ce7a290c0aa/scripts/build_demo_web.sh

set -eu
script_path=$( cd "$(dirname "${BASH_SOURCE[0]}")" ; pwd -P )
cd "$script_path/.."

set -x

# Pre-requisites:
rustup target add wasm32-unknown-unknown

# For generating JS bindings:
if ! cargo install --list | grep -q 'wasm-bindgen-cli v0.2.97'; then
    cargo install --force --quiet wasm-bindgen-cli --version 0.2.97
fi
