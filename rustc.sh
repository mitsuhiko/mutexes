#!/bin/sh
# The in-tree suite uses std::sync::poison; bootstrap normally enables its gate.
if [ "${PROBE_UPSTREAM:-}" = 1 ]; then
  case " $* " in *" --crate-name sync "*) set -- "$@" '-Zcrate-attr=feature(sync_poison_mod)';; esac
fi
exec "$PROBE_RUSTC" --sysroot "$PROBE_SYSROOT" "$@"
