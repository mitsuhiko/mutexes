#!/usr/bin/env bash
# Stock vs shared std: compare the layout table (sizes, alignments, auto
# traits) and the probe assembly of both families, function by function.
set -euo pipefail
cd "$(dirname "$0")"
# Make file!()/panic-location strings identical too; don't strip instructions.
for mode in stock shared; do
  RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$PWD/target/$mode/sysroot=/probe/sysroot --remap-path-prefix=target/$mode/sysroot=/probe/sysroot"
done
export RUSTFLAGS
target=${TARGET:-native}
for mode in stock shared; do
  ./run.sh "$mode" rustc ${TARGET:+--target "$TARGET"} --release --lib -- --emit="asm=$PWD/target/$mode.$target.s"
  # The table is a static; its bytes are the same on both sides or it changed.
  awk '/^_?LAYOUT:/,/\.size/' "target/$mode.$target.s" > "target/$mode.$target.layout"
done
[[ -s target/stock.$target.layout ]]
diff -u "target/stock.$target.layout" "target/shared.$target.layout"
echo "Sizes, alignments and auto traits are identical ($target)."
echo "Probe assembly ($target), stock -> shared:"
python3 asmdiff.py "target/stock.$target.s" "target/shared.$target.s"
