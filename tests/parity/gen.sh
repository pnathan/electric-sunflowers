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
