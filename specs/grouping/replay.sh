#!/bin/sh
# Traces from the interaction modules, replayed against the Rust grouping state; layout and
# enablement are checked by Rust tests over their own input sets.
set -eu
cd "$(dirname "$0")/../.."
mkdir -p specs/traces
for module in sizing board; do
  quint run "specs/grouping/$module.qnt" --mbt --n-traces=200 --max-samples=200 --max-steps=30 --seed=0x1 --out-itf="specs/traces/$module.itf.json" --verbosity=1
done
cargo test --locked grouping
