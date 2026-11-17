#!/usr/bin/env bash
# Runs a release CLI binary on the HEIC/AVIF fixtures in a temp dir and checks the outputs.
# Fails if the binary can't start (missing shared library/DLL) or can't decode either format.
set -euo pipefail
cli="$1"
tmp="$(mktemp -d)"
cp tests/fixtures/photo.heic tests/fixtures/rotated.heic tests/fixtures/photo.avif "$tmp/"
"$cli" "$tmp"
for out in photo.jpg rotated.jpg photo.avif; do
  test -s "$tmp/compressed/$out" || { echo "::error::missing output $out"; exit 1; }
done
echo "smoke test passed"
