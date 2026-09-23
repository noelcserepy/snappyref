#!/bin/sh
# Exhaustive checks, one module at a time (the checker server holds one port). Algorithm modules
# pick their inputs in init and stutter, so Apalache checks every input at depth zero. Interaction
# modules enumerate every reachable state with TLC. Each finishes within two minutes on a laptop;
# a module that stops finishing is split, never skipped.
set -eu
cd "$(dirname "$0")/../.."
invariants() { grep -o 'val inv_[A-Za-z_]*' "$1" | sed 's/val //' | tr '\n' ' '; }
for module in layout enablement; do
  # shellcheck disable=SC2046
  quint verify "specs/grouping/$module.qnt" --max-steps=0 --invariants $(invariants "specs/grouping/$module.qnt")
done
for module in sizing board; do
  # shellcheck disable=SC2046
  quint verify "specs/grouping/$module.qnt" --backend=tlc --invariants $(invariants "specs/grouping/$module.qnt")
done
