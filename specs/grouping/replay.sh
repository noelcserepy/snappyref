#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
mkdir -p specs/traces
quint run specs/grouping/spec.qnt --mbt --n-traces=200 --max-samples=200 --max-steps=20 --seed=0x1 --out-itf=specs/traces/short.itf.json --verbosity=1
quint run specs/grouping/spec.qnt --mbt --n-traces=50 --max-samples=50 --max-steps=60 --seed=0x2 --out-itf=specs/traces/long.itf.json --verbosity=1
cargo test --locked grouping
