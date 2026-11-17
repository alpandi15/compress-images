#!/bin/sh
# Regenerates THIRD_PARTY_LICENSES.html from Cargo.lock (needs `cargo install cargo-about
# --features cli --locked`). Run after changing dependencies; the release workflow fails
# when the committed file is stale.
set -eu
cd "$(dirname "$0")/.."
cargo about generate --all-features --workspace --locked about.hbs -o THIRD_PARTY_LICENSES.html
