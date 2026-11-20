#!/bin/sh
set -eu

fixture_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
manifest="$fixture_dir/Cargo.toml"

# Keep nested cargo metadata offline and send the mock registry's requests
# directly to loopback, including on machines with a configured HTTP proxy.
export CARGO_NET_OFFLINE=true
export NO_PROXY=127.0.0.1 no_proxy=127.0.0.1

normal_tree=$(cargo tree --offline --locked --manifest-path "$manifest" \
    --edges normal --target all --prefix none --format '{p}')
if printf '%s\n' "$normal_tree" | grep -E '^(shipper-cli|clap|indicatif) v'; then
    echo 'unexpected CLI package in the normal dependency graph' >&2
    exit 1
fi

cargo test --offline --locked --manifest-path "$manifest"
cargo run --offline --locked --manifest-path "$manifest"
