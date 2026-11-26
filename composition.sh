#!/usr/bin/env bash
# Sizes of Mutex<T> vs nonpoison::Mutex<Poison<T>> (and RefCell<T> vs
# RefCell<Poison<T>>) in the shared std, for the native or a cross TARGET.
set -euo pipefail
cd "$(dirname "$0")"
touch src/lib.rs # the extra rustc flags are not part of cargo's fingerprint
./run.sh shared rustc ${TARGET:+--target "$TARGET"} --release --lib -- --cfg poison_cell -Zprint-type-sizes 2>/dev/null |
  python3 composition.py
