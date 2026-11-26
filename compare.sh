#!/usr/bin/env bash
# Stock vs shared std, natively or for a cross TARGET: the layout table (sizes,
# alignments, auto traits) and the probe assembly of both families.
set -euo pipefail
cd "$(dirname "$0")"
# Make file!()/panic-location strings identical too; don't strip instructions.
for mode in stock shared; do
  RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$PWD/target/$mode/sysroot=/probe/sysroot --remap-path-prefix=target/$mode/sysroot=/probe/sysroot"
done
export RUSTFLAGS
target=${TARGET:-native}
for mode in stock shared; do
  touch src/lib.rs # cargo would skip rustc, and the --emit, for a fresh lib
  ./run.sh "$mode" rustc ${TARGET:+--target "$TARGET"} --release --lib -- --emit="asm=$PWD/target/$mode.$target.s"
done
echo "== $target, stock -> shared"
python3 asmdiff.py "target/stock.$target.s" "target/shared.$target.s"
