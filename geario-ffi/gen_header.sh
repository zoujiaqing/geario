#!/bin/sh
# Regenerates include/geario.h. Needs cbindgen: cargo install cbindgen
set -e
cd "$(dirname "$0")"
cbindgen --config cbindgen.toml --crate geario-ffi --output include/geario.h
echo "wrote include/geario.h"
