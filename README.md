# One lock implementation for `std::sync::poison` and `std::sync::nonpoison`

A libstd overlay for [rust-lang/rust#134645](https://github.com/rust-lang/rust/issues/134645),
which asks:

> Again, we can make progress by unifying implementations, but if there are
> more ambitious plans to both unify the APIs and also make poisoning "free" on
> certain platforms (and not others) then the unification is probably a waste
> of time.

**The unification is free, on every lock implementation std has.** `Mutex`,
`RwLock` and `Condvar` are implemented once, in a private module
[`sync::lock`](overlay/std/src/sync/lock.rs), and both families are thin
newtypes over it. Compared to stock std, on every target checked:

- every lock and guard of both families has the same size, alignment and auto
  traits (`Send`, `Sync`, `UnwindSafe`, `RefUnwindSafe`, `Unpin`);
- every probe function of both families compiles to byte-identical assembly.

Neither side pays for sharing. The shared code also doesn't stand in the way
of making poisoning cheaper per platform. With `panic=abort` the flag is
already zero-sized, and it stays that way. A poison bit kept inside a
platform's lock word would be another implementation of the same policy hook.

A composable [`Poison<T>`](overlay/std/src/sync/poison/cell.rs) for
`RefCell<Poison<T>>` and similar is included as an extra. It shares the
poisoning code with the locks, but it can't be used to build them, for the
reason [below](#poisont).

## How

The shared lock takes the poison policy `P` as an ordinary field between the
system lock and the data:

```rust
pub(crate) struct Mutex<P, T: ?Sized> {
    inner: sys::Mutex,
    poison: P,          // `()` for nonpoison, `poison::Flag` for poison
    data: UnsafeCell<T>,
}
```

`lock::Mutex<(), T>` and `lock::Mutex<Flag, T>` have exactly the fields of the
old nonpoison and poisoning mutexes, so rustc lays them out the same way. In
particular, the flag still fits into padding next to the system lock. An
exclusive guard is `{ &lock, P::Guard }`, so a poisoning guard reaches its
flag through the lock pointer it already holds.

The `Poison` trait hooks the policy in at fixed points:

- `enter` after an exclusive acquisition;
- `check` after a shared one;
- `leave` before an exclusive release.

For `()` every hook is an inlined no-op that stores nothing. For `Flag` they
are the stock `Flag::guard`, `borrow` and `done`. An acquisition returns
`P::Result<G>`, which is `G` for `()` and `LockResult<G>` for `Flag`. Both
families therefore call the same code:

```rust
pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {  // nonpoison: MutexGuard<'_, T>
    self.inner.lock(MutexGuard::new)
}
```

Locking, guards, mapping, downgrading, unlocking and condvar waits exist once.
The public files keep the docs, the result and error translation, and
`is_poisoned`/`clear_poison`. The poisoning condvar loops stay separate,
because they must return a poisoned guard before calling the predicate again.

"Equivalent" code wasn't enough to be identical. The rest of the design exists
to make it identical:

- **MIR inlining.** Acquisitions are `#[rustc_force_inline]` and take the
  public guard's constructor. A public `lock` then has the same MIR as stock
  and builds its `LockResult` in a single `map_result`. Without this, the
  thin public functions got inlined into callers instead, and LLVM placed the
  flag address differently in 4 of the 13 poisoning probes.
- **One destructor.** Public guards hold the shared guard in `ManuallyDrop` and
  release it from their own `Drop`. A guard therefore has one destructor, not a
  public one wrapping the shared one. Nested drop glue lost `!range` and
  alignment metadata on the cold `PoisonError` paths.
- **No landing pads.** Guard constructors are `Copy`, so a panic during
  acquisition needs no cleanup for them. On Windows, a leftover `personality`
  alone changed instruction selection.
- **Variance.** Mapped guards store `P::Ptr` (`()` or `NonNull<Flag>`) rather
  than a lifetime-carrying associated type, which would make `'a` invariant.
- **Auto traits.** The shared types only have auto-derived traits. An explicit
  `UnwindSafe` impl for `lock::Mutex<Flag, T>` would turn off the auto impl for
  `lock::Mutex<(), T>` as well. The two poisoning exclusive guards now state
  their unconditional unwind safety explicitly; they used to get it through
  `&Mutex<T>`.
- **Same semantics.** Mapping keeps the panic snapshot taken at acquisition.
  `downgrade` reports and discards existing poison and never poisons, as in
  stock.

The patch is +1293 / −692 lines. About 890 of them are the shared module and
200 are `Poison<T>`.

## Evidence

[`compare.sh`](compare.sh) builds stock and modified std with the same
compiler (nightly-2026-10-21) and the same flags, for the host or any
`TARGET`. It diffs the whole assembly of the [probes](src/lib.rs) (11
nonpoison and 13 poisoning functions, plus the generic std code instantiated
for them) function by function. It also diffs the [layout table](src/layout.rs)
of 42 lock and guard types, which is compiled into the probes as a static.
Most targets are cross-compiled, so their results are compile-time checks.

| Lock implementations (mutex / rwlock / condvar) | Target | Functions identical |
|---|---|---|
| pthread / queue / pthread | aarch64-apple-darwin (native) | 36 of 36 |
| same, `panic=abort` | aarch64-apple-darwin | 21 of 21 |
| pthread / queue / pthread | x86_64-unknown-illumos | 37 of 37 |
| futex (Linux) | x86_64- and aarch64-unknown-linux-gnu | 33 of 33 |
| futex (FreeBSD umtx) | x86_64-unknown-freebsd | 33 of 33 |
| futex (WaitOnAddress) | x86_64-pc-windows-msvc | 33 of 33 |
| fuchsia / futex / futex | x86_64-unknown-fuchsia | 33 of 33 |
| windows7 (SRW) / queue / windows7 | x86_64-win7-windows-msvc | 33 of 33 |
| sgx / queue / sgx | x86_64-fortanix-unknown-sgx | 49 of 49 |
| futex (wasm atomics) | wasm32-wasip1-threads | 17 of 17 |
| no_threads, `panic=abort` | wasm32-unknown-unknown | 16 of 16 |
| xous / queue / xous | riscv32imac-unknown-xous-elf | see below |

The layout table is identical on every target.

On xous, LLVM's function merging folds the identical drop glue of several
poisoning `MutexGuard<T>`s into one. Stock keeps a different instance than
shared, so the callers' symbols differ (26 of 37). With
`RUSTFLAGS=-Zmerge-functions=disabled`, 38 of 41 functions are identical. The
3 left are one cold path: when a `MutexGuard::map` closure panics, shared drops
the shared guard type, and LLVM inlines that drop instead of calling an
outlined copy. The code is equivalent.

Not checked: `solid_asp3` (itron/solid) and hermit, where stock std doesn't
build at this nightly. Also not checked: debugger visualizers, which read the
old field paths.

Tests: the 8 [contract tests](tests/contract.rs) pass on stock and shared, the
4 [`Poison<T>` tests](tests/poison_cell.rs) pass, and so does the original
`std/tests/sync` suite (339 pass, 1 ignored). std builds without new warnings,
with unwinding and with `panic=abort`.

## `Poison<T>`

`std::sync::poison::Poison<T>` (feature `poison_cell`) adds `Mutex`-style
poisoning to anything that hands out `&mut T`. Examples are
`RefCell<Poison<T>>`, `nonpoison::Mutex<Poison<T>>`, a lock from another
crate, or plain ownership:

```rust
let cell = RefCell::new(Poison::new(vec![1, 2, 3]));
let mut cell = cell.borrow_mut();
let mut v = cell.guard()?;   // LockResult<PoisonGuard<'_, Vec<i32>>>
v.push(4);                   // a panic before `v` drops poisons the value
```

It uses the same `Flag`, panic snapshot and rules as the locks. Its API is
`guard(&mut self)`, `get(&self)`, `into_inner`, `is_poisoned` and
`clear_poison`.

**Why the poisoning mutex isn't `nonpoison::Mutex<Poison<T>>`.** That type
behaves like `Mutex<T>`, but it's bigger for many `T`. A `Poison<T>` can be
moved and swapped on its own, so it has to carry its flag, padded to `T`'s
alignment. Rust never places a field into another field's padding, so the
mutex can't reuse that space. `Mutex<T>` keeps the flag next to the system
lock, where there's usually room already. No layout trick gets around this
soundly:

- a packed `Poison<T>` loses `T`'s alignment;
- a mutex that specialises on `Poison<U>` and stores the flag itself leaves
  no `Poison<U>` to hand out as `&mut`;
- lending the payload's padding to the lock word lets `mem::swap` move a live
  lock.

Sizes from [`composition.sh`](composition.sh) (`-Zprint-type-sizes`, real std
types):

| `T` | Linux `Mutex<T>` | Linux `Mutex<Poison<T>>` | Windows `Mutex<T>` | Windows `Mutex<Poison<T>>` | macOS `Mutex<T>` | macOS `Mutex<Poison<T>>` |
|---|---:|---:|---:|---:|---:|---:|
| `u32` | 12 | 12 | 8 | 12 | 16 | 16 |
| `u64`, `Box<_>` | 16 | 24 | 16 | 24 | 24 | 24 |
| `Vec<u8>`, `String` | 32 | 40 | 32 | 40 | 40 | 40 |
| `HashMap<u8, u8>` | 56 | 64 | 56 | 64 | 64 | 64 |
| 64-byte aligned | 128 | 192 | 128 | 192 | 128 | 192 |

The composed form costs nothing with `panic=abort`, where the flag is
zero-sized (wasm32-unknown-unknown). For `RefCell<Poison<T>>` there is no
older poisoning cell whose size it could regress.

## Reproduce

Requires rustup, nightly-2026-10-21 with `rust-src`, rsync and python3.
`run.sh` copies rust-src under `target/{stock,shared}/sysroot`, applies the
[overlay](overlay/) to the shared copy, and builds with `-Zbuild-std`. It
never touches the installed toolchain, and cross targets need no extra
rustup components.

```sh
export TOOLCHAIN=nightly-2026-10-21
rustup toolchain install "$TOOLCHAIN" --component rust-src

./run.sh shared test --release --test contract     # also passes in stock mode
./run.sh shared test --release --test poison_cell  # shared only
./run.sh shared upstream                           # std/tests/sync
./compare.sh                                       # layouts + assembly, native
TARGET=x86_64-pc-windows-msvc ./compare.sh         # ... or any other target
CARGO_PROFILE_RELEASE_PANIC=abort ./compare.sh
TARGET=x86_64-unknown-linux-gnu ./composition.sh   # Mutex<T> vs Mutex<Poison<T>>
./run.sh shared run --release --example layout      # print the layout table
./run.sh stock run --release --example bench        # then: shared
./patch.sh > std.patch                              # against rust-lang/rust 19833af72
```
