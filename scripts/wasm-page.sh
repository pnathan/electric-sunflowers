#!/usr/bin/env bash
# Builds the engine for wasm32 and assembles the single-file browser page
# (crates/wasm/page.tpl.html + short-demo.json + the .wasm as base64).
# Usage: scripts/wasm-page.sh OUT.html
# One-off: needs `rustup target add wasm32-unknown-unknown`. Uses cargo
# because Bazel has no wasm target set up yet.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
out="${1:?usage: wasm-page.sh OUT.html}"
RUSTFLAGS="-C link-arg=--import-undefined" CARGO_PROFILE_RELEASE_DEBUG=0 \
  CARGO_PROFILE_RELEASE_STRIP=true \
  cargo build --offline --release --target wasm32-unknown-unknown -p sfwasm
python3 - "$out" <<'PY'
import base64, sys
t = open('crates/wasm/page.tpl.html').read()
w = base64.b64encode(open('target/wasm32-unknown-unknown/release/sfwasm.wasm', 'rb').read()).decode()
s = open('crates/wasm/short-demo.json').read()
f = open('crates/wasm/full-demo.json').read()
import json, re
rs = open('crates/songwriter/src/prompt.rs').read()
raw = re.search(r'format!\(\s*r#"(.*?)"#,', rs, re.S).group(1)
st = open('crates/wasm/styles.json').read()
out = t.replace('__SONG__', s).replace('__FULL__', f).replace('__STYLES__', st).replace('__PROMPT__', json.dumps(raw).replace('</', '<\\/')).replace('__WASM__', w)
open(sys.argv[1], 'w').write(out)
PY
