#!/usr/bin/env bash
# Build against a private rust-src tree; never modify the installed toolchain.
set -euo pipefail
cd "$(dirname "$0")"
mode=${1:?usage: ./run.sh stock\|shared [test\|rustc\|run ...]}
shift
case $mode in stock|shared);; *) exit 2;; esac
export RUSTUP_TOOLCHAIN=${TOOLCHAIN:-nightly}
stock=$(rustc --print sysroot)
[[ $(rustc -vV) == *19833af7224424e8b53e288097d56b6af43d5bcf* ]] || {
  echo "Need nightly 19833af72 (rustup toolchain install nightly-2026-10-21 -c rust-src; TOOLCHAIN=nightly-2026-10-21 ./run.sh ...)" >&2
  exit 1
}
export CARGO_TARGET_DIR=$PWD/target/$mode
root=$CARGO_TARGET_DIR/sysroot
lib=$root/lib/rustlib/src/rust/library
if [[ ! -d $lib ]]; then
  mkdir -p "$(dirname "$lib")"
  for p in "$stock"/lib/rustlib/*; do
    [[ ${p##*/} == src ]] || ln -s "$p" "$root/lib/rustlib/"
  done
  cp -R "$stock/lib/rustlib/src/rust/library" "$lib"
fi
if [[ $mode == shared ]]; then
  # Update changed files only so incremental runs are cheap.
  rsync -rc overlay/ "$lib/"
fi
export PROBE_SYSROOT=$root PROBE_RUSTC=$stock/bin/rustc RUSTC=$PWD/rustc.sh
if [[ ${1:-} == upstream ]]; then
  shift
  export PROBE_UPSTREAM=1
  harness=$CARGO_TARGET_DIR/upstream
  mkdir -p "$harness"
  cp upstream.toml "$harness/Cargo.toml"
  [[ -L $harness/tests ]] || ln -s "$lib/std/tests" "$harness/tests"
  set -- test --release --manifest-path "$harness/Cargo.toml" --test sync -- "$@"
elif [[ $# -eq 0 ]]; then
  # tests/poison_cell.rs needs the shared std.
  if [[ $mode == shared ]]; then set -- test --tests; else set -- test --test contract; fi
fi
cargo -Zbuild-std "$@"
