use super::{MutexGuard, Poison};
use crate::sys::sync as sys;
use crate::time::Duration;

pub(crate) struct Condvar {
    inner: sys::Condvar,
}

impl Condvar {
    #[inline(always)]
    pub(crate) const fn new() -> Condvar {
        Condvar { inner: sys::Condvar::new() }
    }

    #[inline(always)]
    pub(crate) fn wait<P: Poison, T: ?Sized>(&self, guard: &mut MutexGuard<'_, P, T>) {
        // SAFETY: the guard proves that its mutex is locked by this thread.
        unsafe { self.inner.wait(&guard.lock.inner) }
    }

    /// Returns whether the wait timed out.
    #[inline(always)]
    pub(crate) fn wait_timeout<P: Poison, T: ?Sized>(
        &self,
        guard: &mut MutexGuard<'_, P, T>,
        dur: Duration,
    ) -> bool {
        // SAFETY: the guard proves that its mutex is locked by this thread.
        unsafe { !self.inner.wait_timeout(&guard.lock.inner, dur) }
    }

    #[inline(always)]
    pub(crate) fn notify_one(&self) {
        self.inner.notify_one()
    }

    #[inline(always)]
    pub(crate) fn notify_all(&self) {
        self.inner.notify_all()
    }
}
