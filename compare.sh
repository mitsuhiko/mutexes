#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
# Make file!()/panic-location strings identical too; don't strip instructions.
for mode in stock shared; do
  RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$PWD/target/$mode/sysroot=/probe/sysroot --remap-path-prefix=target/$mode/sysroot=/probe/sysroot"
done
export RUSTFLAGS
if [[ -n ${TARGET:-} ]]; then
  for mode in stock shared; do
    ./run.sh "$mode" rustc --target "$TARGET" --release --lib -- --emit="asm=$PWD/target/$mode.$TARGET.s"
  done
  diff -u "target/stock.$TARGET.s" "target/shared.$TARGET.s"
  echo "Nonpoison release probe assembly is identical ($TARGET)."
  exit
fi
for mode in stock shared; do
  ./run.sh "$mode" rustc --release --lib -- --emit="asm=$PWD/target/$mode.s"
  ./run.sh "$mode" run --quiet --release --example layout > "target/$mode.layout"
  grep '^plain ' "target/$mode.layout" > "target/$mode.plain-layout"
done
diff -u target/{stock,shared}.plain-layout
diff -u target/{stock,shared}.s
echo "Nonpoison layouts and release probe assembly are identical."
echo "All layout differences:"
diff -u target/{stock,shared}.layout || [[ $? == 1 ]]
