# One lock implementation for `std::sync::poison` and `std::sync::nonpoison`

A libstd overlay for [rust-lang/rust#134645](https://github.com/rust-lang/rust/issues/134645).
`Mutex`, `RwLock` and `Condvar` are implemented once, in a private module
[`sync::lock`](overlay/std/src/sync/lock.rs). Both public families are thin
newtypes over it. Compared to stock std:

- **Same layout and auto traits.** Every lock and guard keeps its size,
  alignment and auto traits in both families, including over-aligned payloads (`Mutex<u32>` is
  still 16 bytes, `Mutex<CacheLine>` still 128, a poisoning `MutexGuard`
  still 16).
- **No cost for nonpoison.** The nonpoison probes compile to byte-identical
  assembly.
- **Same cost for poison.** 20 of the 24 probes are byte-identical. The other
  4 are 4–6 instructions *shorter*, with the same atomics and calls.
- Public types, signatures, docs and stability attributes are unchanged.

## How

The shared lock carries a poison policy `P` as an ordinary field, between the
system lock and the data:

```rust
pub(crate) struct Mutex<P, T: ?Sized> {
    inner: sys::Mutex,
    poison: P,          // `()` for nonpoison, `poison::Flag` for poison
    data: UnsafeCell<T>,
}
```

`nonpoison::Mutex<T>` wraps `lock::Mutex<(), T>` and `poison::Mutex<T>` wraps
`lock::Mutex<Flag, T>`. Those are the stock layouts field for field, so rustc
lays them out the same way: the flag still fits into padding next to the
system lock. Exclusive guards are `{ &lock, P::Guard }`, so a poisoning guard
reaches its flag through the lock pointer it already holds.

The [`Poison`](overlay/std/src/sync/lock.rs) trait hooks the policy in at
fixed points: `enter` after an exclusive acquisition, `check` after a shared
one, `leave` before an exclusive release. For `()` every hook is an inlined
no-op and stores nothing. For `Flag` they are the stock `Flag::guard`, `borrow`
and `done`. Acquisition results are an associated
type, `P::Result<G>`: `G` for `()`, `LockResult<G>` for `Flag`. The shared
lock takes the public guard's constructor, so the poisoning `lock` builds
its result in a single `map_result`, just as stock does. Both families call it
the same way:

```rust
pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {  // nonpoison: MutexGuard<'_, T>
    self.inner.lock(|inner| MutexGuard { inner })
}
```

The public files keep their docs, result and error translation, and
`is_poisoned`/`clear_poison`. Locking, guards, mapping, downgrading and
unlocking exist once. The poisoning condvar checks the flag after waking. Its
predicate loops stay separate, because they have to return a poisoned guard
before calling the predicate again.

Details that matter:

- Mapped guards store `P::Ptr` (`()` or `NonNull<Flag>`). A lifetime-carrying
  associated type would make `'a` invariant, but the public mapped guards are
  covariant in `'a`.
- Mapping keeps the panic snapshot taken at acquisition. If the map closure
  panics, the shared guard is still the owner, so its drop poisons before
  unlocking.
- `downgrade` does not poison. It still reports and discards existing poison,
  like stock.
- Public guards keep their explicit `Drop` impls. Moving the shared guard out
  of one (for `map`, `downgrade`) is the one `ptr::read` in
  [`into_inner!`](overlay/std/src/sync/lock.rs).
- The shared types only have auto-derived traits. An explicit `UnwindSafe`
  impl for `lock::Mutex<Flag, T>` would switch off the auto impl for
  `lock::Mutex<(), T>` too, and change nonpoison. The two poisoning exclusive
  guards used to inherit unwind safety from `&Mutex<T>`/`&RwLock<T>`, so they
  now state it explicitly (rustdoc lists these impls under trait
  implementations instead of auto traits).

The patch is +953 / −709 lines, most of it the ~800-line shared module. The
duplicated logic is gone, but both public APIs and their docs remain.

## Why not `Poison<plain::Mutex<T>>` or `plain::Mutex<Poison<T>>`

Both nest the flag inside a struct, and a nested struct's padding can't be
reused. Outside the lock, `Mutex<u32>` grows from 16 to 24 bytes and the guard
needs a second pointer. Inside the payload, `Poison<CacheLine>` is already 128
bytes and the mutex becomes 192. Only a flat field matches stock for every `T`.

## Evidence

Measured on arm64 macOS (pthread) and x86_64 Linux (futex, cross-compiled
only), nightly-2026-10-21, release, `panic=unwind`. Stock and modified std
are built with the same compiler. [`compare.sh`](compare.sh) diffs the whole
assembly output of the [probes](src/lib.rs) function by function:

| | identical functions | differing functions |
|---|---|---|
| aarch64 | 101 of 105 | `p_lock_inc`, `p_lock_recover`, `p_map`, `p_write`: 4–6 fewer instructions |
| x86_64 | 32 of 41 | the same 4 probes (5 fewer each), plus 5 cold `PoisonError` drop/unwrap paths with equal counts |

All differences are in poisoning code, and all have the same atomics and calls
as stock. Stock sinks the flag address into both branches of the `panicking()`
check and needs an extra callee-saved register on the hot path; the shared
version computes it once. The [layout table](src/layout.rs) (size, alignment,
`Send`, `Sync`, `UnwindSafe`, `RefUnwindSafe` and `Unpin` of 42 types) is
compiled into the probes as a static and is byte-identical on both targets.
On arm64, uncontended `lock`/`write`/`read` throughput
([`examples/bench.rs`](examples/bench.rs)) is the same within noise: about
5.7 ns/op for both families on both builds.

Tests: the 8 [contract tests](tests/contract.rs) and the original
`std/tests/sync` suite (339 pass, 1 ignored) pass. std builds without new
warnings, with `panic=abort` too.

Not covered: debugger visualizers (they read the old field paths) and
platforms other than the two above.

## Reproduce

Requires rustup, nightly-2026-10-21 with `rust-src`, rsync and python3.
`run.sh` copies rust-src under `target/{stock,shared}/sysroot`, applies the
[overlay](overlay/) to the shared copy, and builds with `-Zbuild-std`. It
never touches the toolchain.

```sh
export TOOLCHAIN=nightly-2026-10-21
rustup toolchain install "$TOOLCHAIN" --component rust-src

./run.sh shared test --release --test contract   # also passes in stock mode
./run.sh shared upstream                         # std/tests/sync
./compare.sh                                     # layouts + native assembly
TARGET=x86_64-unknown-linux-gnu ./compare.sh      # the same, cross-compiled
./run.sh shared run --release --example layout    # print the layout table
./run.sh stock run --release --example bench      # then: shared
CARGO_PROFILE_RELEASE_PANIC=abort ./run.sh shared build --release --lib
./patch.sh > std.patch                            # against rust-lang/rust 19833af72
```
