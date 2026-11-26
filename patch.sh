#!/usr/bin/env bash
# Emit a patch applicable from the root of rust-lang/rust at the pinned revision.
set -euo pipefail
cd "$(dirname "$0")"
src=$(rustc +"${TOOLCHAIN:-nightly}" --print sysroot)/lib/rustlib/src/rust/library
find overlay -name '*.rs' | sort | while read -r new; do
  name=library/${new#overlay/}
  old=$src/${new#overlay/}
  label=a/$name
  if [[ ! -f $old ]]; then old=/dev/null; label=/dev/null; fi
  diff -u -L "$label" -L "b/$name" "$old" "$new" || [[ $? == 1 ]]
done
