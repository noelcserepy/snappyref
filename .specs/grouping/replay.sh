#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
mkdir -p .specs/traces
quint run .specs/grouping/board.qnt --mbt --n-traces=200 --max-samples=200 --max-steps=30 --seed=0x1 --out-itf=.specs/traces/board.itf.json --verbosity=1
cargo test --locked grouping::tests::replay_every_board_trace
