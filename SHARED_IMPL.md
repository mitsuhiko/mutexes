# Sharing one implementation between `poison` and `nonpoison`

## The shape

A private module, `sync::lock`, owns `Mutex`, `RwLock` and `Condvar`. Each lock
is generic over a poison policy `P` that sits in a flat field:

```rust
struct Mutex<P, T: ?Sized> { inner: sys::Mutex, poison: P, data: UnsafeCell<T> }
struct MutexGuard<'a, P: Poison, T: ?Sized> { lock: &'a Mutex<P, T>, poison: P::Guard }
```

`nonpoison` uses `P = ()` and `poison` uses `P = Flag`. Each public type is a
newtype that only translates results. The policy is this trait:

```rust
trait Poison {
    type Result<G>;      // `G` for (), `LockResult<G>` for Flag
    type Guard: Clone;   // snapshot taken at acquisition: (), or "was panicking"
    type Ptr: Copy;      // mapped guards' way back: (), or NonNull<Flag>

    fn check<G>(&self, guard: G) -> Self::Result<G>;                          // shared acquire
    fn enter<G>(&self, guard: impl FnOnce(Self::Guard) -> G + Copy) -> Self::Result<G>; // exclusive acquire
    fn leave(&self, guard: &Self::Guard);                                     // exclusive release
    fn ptr(&self) -> Self::Ptr;
    unsafe fn leave_ptr(ptr: Self::Ptr, guard: &Self::Guard);                 // release, mapped guard
}
```

For `()` every method is a trivial `#[inline(always)]` body that stores and
checks nothing. For `Flag`,
`enter` is `map_result(self.guard(), guard)`, `check` is
`map_result(self.borrow(), |()| guard)` and `leave` is `self.done(guard)`,
which is the stock poisoning code unchanged.

- **Flat field, not a wrapper.** `Poison<plain::Mutex<T>>` and
  `plain::Mutex<Poison<T>>` both put the flag inside a nested struct. Rust never
  reuses a nested struct's padding, so both grow the mutex for common `T`. On
  Linux, `Mutex<Vec<u8>>` goes from 32 to 40 bytes. A sibling field gives
  exactly the stock field list, so the compiler produces the stock layout. The
  guard reaches the flag through the lock pointer it already holds.
- **Fixed hooks.** The shared code calls the hooks and nothing more, so it
  never has to ask which family it is.
- **The result type belongs to the policy.** `P::Result<G>` is `G` for `()` and
  `LockResult<G>` for `Flag`. The shared `lock` returns it directly, so neither
  family converts a result it doesn't need.
- **The shared lock builds the public guard.** It takes the public guard's
  constructor, so `pub fn lock(&self) { self.inner.lock(MutexGuard::new) }` is
  the same line in both families. It also produces the result in one step, as
  stock does.
- **Mapped guards hold `P::Ptr`, not `&'a P`.** That is `()` or `NonNull<Flag>`.
  A nonpoison mapped guard stays two words, and having no lifetime in the
  associated type keeps the guards covariant in `'a`.
- **The public guard owns the release.** It holds `ManuallyDrop<shared guard>`
  and unlocks from its own `Drop`, so it keeps the single destructor it always
  had.

## For

- **One implementation of the risky part.** Locking, unlocking, `map` and
  `filter_map`, `downgrade`, condvar waits and their `unsafe` blocks exist once
  instead of twice. A fix or an audit covers both families.
- **Neither family pays.** Layout, auto traits and the whole probe assembly are
  identical to stock on every buildable lock implementation: futex, pthread,
  Windows 7, Fuchsia, SGX, queue RwLock and no_threads.
- **Room to grow.** Platform-specific poisoning becomes a new policy, not a new
  copy of the locks. One example is a poison bit kept in the futex word, which
  has spare bits. `panic=abort` is already such a case: the flag is zero-sized.
- **The rules are shared too.** `Poison<T>` reuses the same `Flag` and panic
  snapshot. "When does a lock poison" has one answer.

## Against

- **Zero cost holds up only because of care, not by design.** The obvious
  version was equivalent but not identical. Getting identical code took three
  extra measures: `#[rustc_force_inline]` on acquisitions, flattened drop glue,
  and `Copy` constructors so acquisition has no cleanup. Without CI guarding
  it, a future refactor or LLVM update can bring the drift back. Merged-function
  order still differs on xous.
- **Indirection for readers.** A reader of `Mutex::lock` now follows a newtype,
  a generic shared lock and a trait hook. Before, a single function had the
  whole code path. The trait is small (three hooks, three associated types),
  but it is still an abstraction.
- **The public files barely shrink.** Docs, signatures, stability attributes
  and result translation remain duplicated. The patch is +1293 / −692 lines,
  so the win is in risk, not size.
- **Some details become explicit.** The two poisoning exclusive guards need
  explicit `UnwindSafe` impls, which rustdoc lists differently. Taking a shared
  guard out of a public guard needs one `ManuallyDrop::take`. Debugger
  visualizers have to learn the new field paths.
- **Internal attributes.** `rustc_force_inline` is std-only. That's acceptable
  inside std, but it is a commitment.

## Verdict

Share the implementation. The objection in the issue was that unifying would
be wasted if poisoning later has to become free on some platforms. The answer
is that the shared code is free on all platforms today, and per-platform
poisoning would plug into the same hooks. The real ongoing cost is
maintaining codegen identity, so `compare.sh` (layout table plus per-function
assembly diff) should run in CI on a few targets.
