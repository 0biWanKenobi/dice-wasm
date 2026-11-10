#!/bin/sh
set -eu
wasm-pack build --release --target bundler --out-dir dist --out-name chumsky_dice
