//! The one implementation behind both `sync::poison` and `sync::nonpoison`.
//!
//! Every lock here is generic over a [`Poison`] policy `P` that is stored
//! *flat* next to the system lock and the data: `{ inner, poison: P, data }`.
//! `sync::nonpoison` instantiates it with `()`, a zero-sized policy whose hooks
//! compile to nothing, so it is exactly the old nonpoison lock. `sync::poison`
//! instantiates it with `poison::Flag`, so it is exactly the old poisoning lock:
//! the flag can share padding with the system lock, and an exclusive guard
//! reaches it through the one lock pointer it already holds.
//!
//! The public types in both modules are newtypes around these, which only
//! translate results and errors.
//!
//! Both families compile to the same code as their separate implementations
//! did. Three details keep it that way:
//!
//! - Acquisitions are `#[rustc_force_inline]` and take the public guard's
//!   constructor, so a public `lock` has the same MIR as before and builds its
//!   result in one step.
//! - Public guards hold the shared guard in `ManuallyDrop` and release it from
//!   their own `Drop`, so a guard still has a single destructor.
//! - Constructors are `Copy`, so a panic during acquisition needs no cleanup
//!   for them.

mod condvar;
mod mutex;
mod rwlock;

pub(crate) use condvar::Condvar;
pub(crate) use mutex::{MappedMutexGuard, Mutex, MutexGuard};
pub(crate) use rwlock::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};

/// What a lock does about panics. Each method is called at a fixed point of
/// the shared implementation; `()` does nothing at all.
pub(crate) trait Poison {
    /// What an acquisition returns: the guard itself, or a `LockResult` of it.
    type Result<G>;
    /// State an exclusive guard carries from acquisition to release.
    type Guard: Clone;
    /// How a mapped guard, which no longer knows the lock's type, reaches the
    /// policy. It has no lifetime so that mapped guards stay covariant.
    type Ptr: Copy;

    /// Called right after a shared acquisition.
    fn check<G>(&self, guard: G) -> Self::Result<G>;
    /// Called right after an exclusive acquisition. `guard` builds the guard;
    /// constructors are `Copy` throughout so that no cleanup is needed for them
    /// if acquiring unwinds (a landing pad changes codegen on some targets).
    fn enter<G>(&self, guard: impl FnOnce(Self::Guard) -> G + Copy) -> Self::Result<G>;
    /// Called right before an exclusive release.
    fn leave(&self, guard: &Self::Guard);

    fn ptr(&self) -> Self::Ptr;
    /// [`Poison::leave`] for a mapped guard.
    ///
    /// # Safety
    /// `ptr` must come from [`Poison::ptr`] on a policy that is still alive.
    unsafe fn leave_ptr(ptr: Self::Ptr, guard: &Self::Guard);
}

/// No poisoning: nothing is stored, captured or checked.
impl Poison for () {
    type Result<G> = G;
    type Guard = ();
    type Ptr = ();

    #[inline(always)]
    fn check<G>(&self, guard: G) -> G {
        guard
    }
    #[inline(always)]
    fn enter<G>(&self, guard: impl FnOnce(()) -> G + Copy) -> G {
        guard(())
    }
    #[inline(always)]
    fn leave(&self, _: &()) {}
    #[inline(always)]
    fn ptr(&self) {}
    #[inline(always)]
    unsafe fn leave_ptr(_: (), _: &()) {}
}

// A public guard holds its shared guard in `ManuallyDrop` and releases it from
// its own `Drop`, so dropping it is one destructor, as before, not two nested
// ones. Take the shared guard back out, armed, without running that `Drop`.
macro_rules! into_inner {
    ($guard:expr) => {{
        let mut guard = crate::mem::ManuallyDrop::new($guard);
        // SAFETY: the public guard is consumed and its destructor suppressed.
        unsafe { crate::mem::ManuallyDrop::take(&mut guard.inner) }
    }};
}
pub(crate) use into_inner;
