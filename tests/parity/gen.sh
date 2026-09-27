#!/bin/sh
# Write the JS reference data that the Rust parity tests read, into ref/parity/.
# Every tests/parity/*.js script is run from the repository root.
set -e
cd "$(dirname "$0")/../.."
mkdir -p ref/parity
for f in tests/parity/*.js; do
  echo "$f"
  node --max-old-space-size=4096 "$f"
done

echo
echo "Reference data written to ref/parity/. Run the parity tests with:"
echo "  tests/parity/gen.sh && cargo test --release --workspace --features sfcore/v8,engine/capture_raw"
echo "(the v8 feature switches sfcore's transcendentals to the bit-exact V8 port;"
echo "the default build is fast approximate math and is not held to bit parity)."
