//! Add advisory poisoning to an existing lock, not locking to a poison flag.
use super::{Flag, Guard, LockResult, map_result};
use crate::mem;

/// The flag is outside the lock: checking or clearing it must not acquire the lock.
pub(super) struct Poison<L: ?Sized> {
    pub(super) flag: Flag,
    pub(super) lock: L,
}

impl<L> Poison<L> {
    pub(super) const fn new(lock: L) -> Self {
        Self { flag: Flag::new(), lock }
    }
}

impl Flag {
    /// Readers and exclusive access through &mut self only observe old poison.
    pub(super) fn check<T>(&self, value: T) -> LockResult<T> {
        map_result(self.borrow(), |()| value)
    }

    /// Called *after* exclusive acquisition, including on a successful try_lock.
    pub(super) fn hold<'a, G, T>(
        &'a self,
        guard: G,
        wrap: impl FnOnce(Held<'a, G>) -> T,
    ) -> LockResult<T> {
        map_result(self.guard(), |entry| wrap(Held { watch: Watch { flag: self, entry }, guard }))
    }
}

/// Field drop order matters: publish poison *before* the underlying guard unlocks.
pub(super) struct Held<'a, G> {
    watch: Watch<'a>,
    pub(super) guard: G,
}

pub(super) struct Watch<'a> {
    flag: &'a Flag,
    entry: Guard,
}

impl Drop for Watch<'_> {
    fn drop(&mut self) {
        self.flag.done(&self.entry)
    }
}

impl Watch<'_> {
    /// The mapper owns the plain guard. If user code unwinds, mark it HERE,
    /// before the mapper drops that guard. On success, transfer the obligation.
    pub(super) fn project<R>(self, f: impl FnOnce() -> R) -> R {
        let result = f();
        mem::forget(self);
        result
    }
}

impl<'a, G> Held<'a, G> {
    pub(super) fn flag(&self) -> &'a Flag {
        self.watch.flag
    }

    pub(super) fn downgrade(self) -> G {
        mem::forget(self.watch);
        self.guard
    }

    /// Keep the original panic-at-entry snapshot across maps (never resample).
    pub(super) fn transfer<H>(self, f: impl FnOnce(G, Watch<'a>) -> H) -> Held<'a, H> {
        let flag = self.watch.flag;
        let entry = self.watch.entry.clone();
        let guard = f(self.guard, self.watch);
        Held { watch: Watch { flag, entry }, guard }
    }
}

impl<'a, G, E> Held<'a, Result<G, E>> {
    pub(super) fn transpose(self) -> Result<Held<'a, G>, Held<'a, E>> {
        match self.guard {
            Ok(guard) => Ok(Held { watch: self.watch, guard }),
            Err(guard) => Err(Held { watch: self.watch, guard }),
        }
    }
}
