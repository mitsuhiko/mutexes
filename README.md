# Can std's two lock families share their implementation?

**Yes. Make nonpoison the implementation, and poison the client.** This is a
working libstd overlay for [rust-lang/rust#134645](https://github.com/rust-lang/rust/issues/134645),
not a model lock or a benchmark of `lock().unwrap_or_else(...)`. No changes to
the nonpoison types or platform backends. Both public sets of types, signatures,
docs and stability gates remain separate.

## Regressions compared to stock `std::sync::Mutex`

On arm64 macOS (nightly-2026-10-21, release, `panic=unwind`), both
**`Mutex<u32>` and `MutexGuard<'_, u32>` grow from 16 to 24 bytes (+50%)**.
The flag itself did not grow: the stock mutex can fit it into padding that the
wrapper cannot reuse. The stock guard can find the flag through its mutex
pointer; the wrapper guard needs a second pointer.

The lock-size change depends on the payload: `Mutex<u64>` stays at 24 bytes,
while a mutex around a 64-byte, 64-aligned payload grows from 128 to 192 bytes.
These are **space regressions**. We did not benchmark poisoning-path latency or
throughput; the [identical assembly results](#cost) apply only to nonpoison.

## Implementation

Start with [wrapper.rs](overlay/std/src/sync/poison/wrapper.rs), then
[`Mutex::lock`](overlay/std/src/sync/poison/mutex.rs). The field is literally:

```rust
inner: Poison<plain::Mutex<T>>,
```

`Poison<L>` is a flag and an ordinary lock. After acquiring that lock, the
poisoning facade attaches a panic-at-entry snapshot to the returned guard.
The watcher is declared **before** the plain guard, so Rust drops it first:
another thread cannot acquire an unlocked-but-not-yet-marked mutex. The
poisoned error owns the live guard, just as it does today.

Why not `plain::Mutex<Poison<T>>`? `is_poisoned` and `clear_poison` must work
without locking, including while the caller already owns the mutex. A flag
inside the payload would need special projections to access it independently
of an exclusive payload borrow. Putting the flag outside avoids that aliasing
problem and requires no new interface from plain locks.

## What gets shared

- **Mutex and RwLock:** acquisition, try-acquisition, storage, dereferencing,
  unlocking, all guard projections/remaps, and atomic write-to-read downgrade
  go through the existing nonpoison implementations. A read guard checks old
  poison on acquisition but carries no watcher.
- **Condvar:** the poisoning condvar contains a plain condvar. It lends it the
  inner mutex guard and checks the flag *after reacquisition*. The poisoning
  predicate loops stay separate: they must return the poisoned guard (and
  timeout result) before calling user code again.
- **What stays public-side:** result/error translation, value accessors with
  different error ownership, formatting, nominal types and their trait impls.
  Source-line savings are smaller than the implementation sharing: the public
  APIs and docs still have two spellings. The patch is +231 / −418 lines. All
  39 unsafe blocks in the three poisoning primitive files disappear; one
  private field-move helper remains in `poison.rs` to preserve *explicit*
  public `Drop` impls. No macro generates lock algorithms.

The subtle case is mapping an exclusive guard. The plain mapper owns the guard
while it calls user code. Marking poison in its *caller* on unwind would be too
late: the plain mapper would already have unlocked. `Watch::project` instead
moves the watcher **into the projection closure**. If it panics, poison is
published there. If it succeeds (including a filter miss), the returned guard
inherits the original snapshot. Mapping is not another acquisition.

## Cost

There is no “poisoning disabled” runtime path: a nonpoison lock has no
`Poison`, flag, watcher, extra pointer, panic query or policy branch. As a check,
we rebuilt **stock and modified std with the same compiler** and compared the
*entire assembly output* of [11 nonpoison probes](src/lib.rs). Byte-identical
on **aarch64-apple-darwin** (pthread mutex) and **x86_64-unknown-linux-gnu**
(futex mutex). Only source directory names were remapped, before compilation;
no instructions were stripped. Linux was cross-compiled, not executed.

Selected sizes in bytes on aarch64 macOS, release, unwinding enabled:

| Type | Nonpoison (both) | Poison stock | Poison wrapper |
|---|---:|---:|---:|
| `Mutex<u32>` | 16 | 16 | 24 |
| `Mutex<u64>` | 16 | 24 | 24 |
| `Mutex<CacheLine>` (64-byte payload and alignment) | 128 | 128 | 192 |
| `MutexGuard<u32>` | 8 | 16 | 24 |
| `MappedMutexGuard<u32>` | 16 | 32 | 32 |
| `RwLock<u32>` | 16 | 16 | 24 |
| `RwLockReadGuard<u32>` | 16 | 16 | 16 |
| `RwLockWriteGuard<u32>` | 8 | 16 | 24 |
| `Condvar` | 16 | 16 | 16 |

**Recommendation:** this is a readable way to share lock behavior with zero
added nonpoison cost. It is **not** a layout-neutral replacement for the
existing poisoning types. The outer flag cannot occupy padding inside a nested
lock; an ordinary exclusive guard now carries a separate pointer to that flag.
If the poisoning regressions are unacceptable, the next design to test is a
private flat `{ raw, header, data }` lock with header `()` or `Flag`, so the
poisoning guard can refer to the whole lock again. That's a different trade:
more intrusion into the plain implementation for better poisoning layout.
Nothing here establishes free *selectable-at-lock-time* poisoning, or measures
poison-path throughput.

## Reproduce

Baseline: Rust [19833af72](https://github.com/rust-lang/rust/commit/19833af7224424e8b53e288097d56b6af43d5bcf),
nightly-2026-10-21. Requires rustup, rust-src, bash, rsync and a host linker.
Cargo may fetch dependencies on first use. If `nightly` already points at that
commit with rust-src installed, no setup is needed. Otherwise:

```sh
export TOOLCHAIN=nightly-2026-10-21
rustup toolchain install "$TOOLCHAIN" --component rust-src
```

```sh
./run.sh shared test --release --test contract   # 8 targeted tests; also pass in stock mode
./run.sh shared upstream                         # original std/tests/sync: 339 pass, 1 ignored
./compare.sh                                     # native assembly + size/alignment diff
TARGET=x86_64-unknown-linux-gnu ./compare.sh      # cross-target assembly only
CARGO_PROFILE_RELEASE_PANIC=abort ./run.sh shared build --release --lib
./patch.sh > std.patch                            # review/apply against pinned rust-lang/rust
```

`run.sh` copies rust-src under `target/{stock,shared}/sysroot` and uses
`cargo -Zbuild-std`. It never edits rustup or a Rust checkout. Use the script,
not bare `cargo test`, to test the modified implementation. The refactor is
only the five files under [overlay/](overlay/); everything else is the harness.
This is a feasibility experiment, not a full bootstrap, debugger-visualizer or
all-platform compatibility audit.
