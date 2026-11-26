use super::Poison;
use crate::cell::UnsafeCell;
use crate::marker::PhantomData;
use crate::mem::{ManuallyDrop, forget};
use crate::ops::{Deref, DerefMut};
use crate::ptr::NonNull;
use crate::sys::sync as sys;

pub(crate) struct RwLock<P, T: ?Sized> {
    inner: sys::RwLock,
    pub(crate) poison: P,
    data: UnsafeCell<T>,
}

/// Readers never poison, so there is a single read guard for every policy.
pub(crate) struct RwLockReadGuard<'rwlock, T: ?Sized + 'rwlock> {
    // NB: we use a pointer instead of `&'rwlock T` to avoid `noalias` violations, because a
    // `RwLockReadGuard` argument doesn't hold immutability for its whole scope, only until it drops.
    // `NonNull` is also covariant over `T`, just like we would have with `&T`. `NonNull`
    // is preferable over `const* T` to allow for niche optimization.
    data: NonNull<T>,
    inner_lock: &'rwlock sys::RwLock,
}

pub(crate) struct RwLockWriteGuard<'rwlock, P: Poison, T: ?Sized + 'rwlock> {
    lock: &'rwlock RwLock<P, T>,
    poison: P::Guard,
}

pub(crate) struct MappedRwLockReadGuard<'rwlock, T: ?Sized + 'rwlock> {
    // NB: we use a pointer instead of `&'rwlock T` to avoid `noalias` violations, because a
    // `MappedRwLockReadGuard` argument doesn't hold immutability for its whole scope, only until it drops.
    // `NonNull` is also covariant over `T`, just like we would have with `&T`. `NonNull`
    // is preferable over `const* T` to allow for niche optimization.
    data: NonNull<T>,
    inner_lock: &'rwlock sys::RwLock,
}

pub(crate) struct MappedRwLockWriteGuard<'rwlock, P: Poison, T: ?Sized + 'rwlock> {
    // NB: we use a pointer instead of `&'rwlock mut T` to avoid `noalias` violations, because a
    // `MappedRwLockWriteGuard` argument doesn't hold uniqueness for its whole scope, only until it drops.
    // `NonNull` is covariant over `T`, so we add a `PhantomData<&'rwlock mut T>` field
    // below for the correct variance over `T` (invariance).
    data: NonNull<T>,
    _variance: PhantomData<&'rwlock mut T>,
    inner_lock: &'rwlock sys::RwLock,
    poison_flag: P::Ptr,
    poison_guard: P::Guard,
}

impl<P, T> RwLock<P, T> {
    #[inline(always)]
    pub(crate) const fn new(poison: P, t: T) -> Self {
        RwLock { inner: sys::RwLock::new(), poison, data: UnsafeCell::new(t) }
    }

    #[inline(always)]
    pub(crate) fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<P, T: ?Sized> RwLock<P, T> {
    #[inline(always)]
    pub(crate) fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }

    #[inline(always)]
    pub(crate) const fn data_ptr(&self) -> *mut T {
        self.data.get()
    }
}

/// Acquisitions take the public guard's constructor, so that both families
/// build their guard in one step: `G` for nonpoison, `LockResult<G>` for poison.
impl<P: Poison, T: ?Sized> RwLock<P, T> {
    #[rustc_force_inline]
    pub(crate) fn read<'a, G>(
        &'a self,
        wrap: impl FnOnce(RwLockReadGuard<'a, T>) -> G + Copy,
    ) -> P::Result<G> {
        unsafe {
            self.inner.read();
            self.poison.check(wrap(RwLockReadGuard::new(&self.inner, &self.data)))
        }
    }

    #[rustc_force_inline]
    pub(crate) fn try_read<'a, G>(
        &'a self,
        wrap: impl FnOnce(RwLockReadGuard<'a, T>) -> G + Copy,
    ) -> Option<P::Result<G>> {
        unsafe {
            if self.inner.try_read() {
                Some(self.poison.check(wrap(RwLockReadGuard::new(&self.inner, &self.data))))
            } else {
                None
            }
        }
    }

    #[rustc_force_inline]
    pub(crate) fn write<'a, G>(
        &'a self,
        wrap: impl FnOnce(RwLockWriteGuard<'a, P, T>) -> G + Copy,
    ) -> P::Result<G> {
        unsafe {
            self.inner.write();
            RwLockWriteGuard::new(self, wrap)
        }
    }

    #[rustc_force_inline]
    pub(crate) fn try_write<'a, G>(
        &'a self,
        wrap: impl FnOnce(RwLockWriteGuard<'a, P, T>) -> G + Copy,
    ) -> Option<P::Result<G>> {
        unsafe {
            if self.inner.try_write() { Some(RwLockWriteGuard::new(self, wrap)) } else { None }
        }
    }
}

impl<'rwlock, T: ?Sized> RwLockReadGuard<'rwlock, T> {
    /// # Safety
    /// `inner_lock` must be read-locked by the current thread, and guard `data`.
    #[inline(always)]
    unsafe fn new(inner_lock: &'rwlock sys::RwLock, data: &'rwlock UnsafeCell<T>) -> Self {
        // SAFETY: `UnsafeCell::get` never returns null.
        RwLockReadGuard { data: unsafe { NonNull::new_unchecked(data.get()) }, inner_lock }
    }

    #[inline(always)]
    pub(crate) fn map<U, F>(orig: Self, f: F) -> MappedRwLockReadGuard<'rwlock, U>
    where
        F: FnOnce(&T) -> &U,
        U: ?Sized,
    {
        // SAFETY: the conditions of `RwLockReadGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the
        // reference passed to it. If the closure panics, the guard will be dropped.
        let data = NonNull::from(f(unsafe { orig.data.as_ref() }));
        let orig = ManuallyDrop::new(orig);
        MappedRwLockReadGuard { data, inner_lock: orig.inner_lock }
    }

    #[inline(always)]
    pub(crate) fn filter_map<U, F>(
        orig: Self,
        f: F,
    ) -> Result<MappedRwLockReadGuard<'rwlock, U>, Self>
    where
        F: FnOnce(&T) -> Option<&U>,
        U: ?Sized,
    {
        // SAFETY: the conditions of `RwLockReadGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the
        // reference passed to it. If the closure panics, the guard will be dropped.
        match f(unsafe { orig.data.as_ref() }) {
            Some(data) => {
                let data = NonNull::from(data);
                let orig = ManuallyDrop::new(orig);
                Ok(MappedRwLockReadGuard { data, inner_lock: orig.inner_lock })
            }
            None => Err(orig),
        }
    }
}

impl<'rwlock, P: Poison, T: ?Sized> RwLockWriteGuard<'rwlock, P, T> {
    /// # Safety
    /// The lock must be write-locked by the current thread.
    #[rustc_force_inline]
    unsafe fn new<G>(
        lock: &'rwlock RwLock<P, T>,
        wrap: impl FnOnce(Self) -> G + Copy,
    ) -> P::Result<G> {
        lock.poison.enter(|poison| wrap(RwLockWriteGuard { lock, poison }))
    }

    /// Downgrading is not a release: it does not poison.
    #[rustc_force_inline]
    pub(crate) fn downgrade<G>(
        s: Self,
        wrap: impl FnOnce(RwLockReadGuard<'rwlock, T>) -> G + Copy,
    ) -> P::Result<G> {
        let lock = s.lock;

        // We don't want to call the destructor since that calls `write_unlock`.
        forget(s);

        // SAFETY: We take ownership of a write guard, so we must already have the `RwLock` in write
        // mode, satisfying the `downgrade` contract.
        unsafe { lock.inner.downgrade() };

        // SAFETY: We have just successfully called `downgrade`, so we fulfill the safety contract.
        lock.poison.check(wrap(unsafe { RwLockReadGuard::new(&lock.inner, &lock.data) }))
    }

    #[inline(always)]
    pub(crate) fn map<U, F>(orig: Self, f: F) -> MappedRwLockWriteGuard<'rwlock, P, U>
    where
        F: FnOnce(&mut T) -> &mut U,
        U: ?Sized,
    {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the
        // reference passed to it. If the closure panics, the guard will be dropped.
        let data = NonNull::from(f(unsafe { &mut *orig.lock.data.get() }));
        let orig = ManuallyDrop::new(orig);
        MappedRwLockWriteGuard {
            data,
            inner_lock: &orig.lock.inner,
            poison_flag: orig.lock.poison.ptr(),
            poison_guard: orig.poison.clone(),
            _variance: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn filter_map<U, F>(
        orig: Self,
        f: F,
    ) -> Result<MappedRwLockWriteGuard<'rwlock, P, U>, Self>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
        U: ?Sized,
    {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the
        // reference passed to it. If the closure panics, the guard will be dropped.
        match f(unsafe { &mut *orig.lock.data.get() }) {
            Some(data) => {
                let data = NonNull::from(data);
                let orig = ManuallyDrop::new(orig);
                Ok(MappedRwLockWriteGuard {
                    data,
                    inner_lock: &orig.lock.inner,
                    poison_flag: orig.lock.poison.ptr(),
                    poison_guard: orig.poison.clone(),
                    _variance: PhantomData,
                })
            }
            None => Err(orig),
        }
    }
}

impl<'rwlock, T: ?Sized> MappedRwLockReadGuard<'rwlock, T> {
    #[inline(always)]
    pub(crate) fn map<U, F>(orig: Self, f: F) -> MappedRwLockReadGuard<'rwlock, U>
    where
        F: FnOnce(&T) -> &U,
        U: ?Sized,
    {
        // SAFETY: the conditions of `RwLockReadGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the
        // reference passed to it. If the closure panics, the guard will be dropped.
        let data = NonNull::from(f(unsafe { orig.data.as_ref() }));
        let orig = ManuallyDrop::new(orig);
        MappedRwLockReadGuard { data, inner_lock: orig.inner_lock }
    }

    #[inline(always)]
    pub(crate) fn filter_map<U, F>(
        orig: Self,
        f: F,
    ) -> Result<MappedRwLockReadGuard<'rwlock, U>, Self>
    where
        F: FnOnce(&T) -> Option<&U>,
        U: ?Sized,
    {
        // SAFETY: the conditions of `RwLockReadGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the
        // reference passed to it. If the closure panics, the guard will be dropped.
        match f(unsafe { orig.data.as_ref() }) {
            Some(data) => {
                let data = NonNull::from(data);
                let orig = ManuallyDrop::new(orig);
                Ok(MappedRwLockReadGuard { data, inner_lock: orig.inner_lock })
            }
            None => Err(orig),
        }
    }
}

impl<'rwlock, P: Poison, T: ?Sized> MappedRwLockWriteGuard<'rwlock, P, T> {
    #[inline(always)]
    pub(crate) fn map<U, F>(mut orig: Self, f: F) -> MappedRwLockWriteGuard<'rwlock, P, U>
    where
        F: FnOnce(&mut T) -> &mut U,
        U: ?Sized,
    {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the
        // reference passed to it. If the closure panics, the guard will be dropped.
        let data = NonNull::from(f(unsafe { orig.data.as_mut() }));
        let orig = ManuallyDrop::new(orig);
        MappedRwLockWriteGuard {
            data,
            inner_lock: orig.inner_lock,
            poison_flag: orig.poison_flag,
            poison_guard: orig.poison_guard.clone(),
            _variance: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn filter_map<U, F>(
        mut orig: Self,
        f: F,
    ) -> Result<MappedRwLockWriteGuard<'rwlock, P, U>, Self>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
        U: ?Sized,
    {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the
        // reference passed to it. If the closure panics, the guard will be dropped.
        match f(unsafe { orig.data.as_mut() }) {
            Some(data) => {
                let data = NonNull::from(data);
                let orig = ManuallyDrop::new(orig);
                Ok(MappedRwLockWriteGuard {
                    data,
                    inner_lock: orig.inner_lock,
                    poison_flag: orig.poison_flag,
                    poison_guard: orig.poison_guard.clone(),
                    _variance: PhantomData,
                })
            }
            None => Err(orig),
        }
    }
}

impl<T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // SAFETY: this is the release.
        unsafe { self.release() }
    }
}

impl<T: ?Sized> RwLockReadGuard<'_, T> {
    /// Unlocks. Public guards call this from their own `Drop`.
    ///
    /// # Safety
    /// Call once, and neither use nor drop the guard afterwards.
    #[inline(always)]
    pub(crate) unsafe fn release(&mut self) {
        // SAFETY: the conditions of `RwLockReadGuard::new` were satisfied when created.
        unsafe {
            self.inner_lock.read_unlock();
        }
    }
}

impl<P: Poison, T: ?Sized> Drop for RwLockWriteGuard<'_, P, T> {
    #[inline]
    fn drop(&mut self) {
        // SAFETY: this is the release.
        unsafe { self.release() }
    }
}

impl<P: Poison, T: ?Sized> RwLockWriteGuard<'_, P, T> {
    /// Unlocks. Public guards call this from their own `Drop`.
    ///
    /// # Safety
    /// Call once, and neither use nor drop the guard afterwards.
    #[inline(always)]
    pub(crate) unsafe fn release(&mut self) {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when created.
        unsafe {
            self.lock.poison.leave(&self.poison);
            self.lock.inner.write_unlock();
        }
    }
}

impl<T: ?Sized> Drop for MappedRwLockReadGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // SAFETY: this is the release.
        unsafe { self.release() }
    }
}

impl<T: ?Sized> MappedRwLockReadGuard<'_, T> {
    /// Unlocks. Public guards call this from their own `Drop`.
    ///
    /// # Safety
    /// Call once, and neither use nor drop the guard afterwards.
    #[inline(always)]
    pub(crate) unsafe fn release(&mut self) {
        // SAFETY: the conditions of `RwLockReadGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        unsafe {
            self.inner_lock.read_unlock();
        }
    }
}

impl<P: Poison, T: ?Sized> Drop for MappedRwLockWriteGuard<'_, P, T> {
    #[inline]
    fn drop(&mut self) {
        // SAFETY: this is the release.
        unsafe { self.release() }
    }
}

impl<P: Poison, T: ?Sized> MappedRwLockWriteGuard<'_, P, T> {
    /// Unlocks. Public guards call this from their own `Drop`.
    ///
    /// # Safety
    /// Call once, and neither use nor drop the guard afterwards.
    #[inline(always)]
    pub(crate) unsafe fn release(&mut self) {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        unsafe {
            P::leave_ptr(self.poison_flag, &self.poison_guard);
            self.inner_lock.write_unlock();
        }
    }
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        // SAFETY: the conditions of `RwLockReadGuard::new` were satisfied when created.
        unsafe { self.data.as_ref() }
    }
}

impl<P: Poison, T: ?Sized> Deref for RwLockWriteGuard<'_, P, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when created.
        unsafe { &*self.lock.data.get() }
    }
}

impl<P: Poison, T: ?Sized> DerefMut for RwLockWriteGuard<'_, P, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when created.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T: ?Sized> Deref for MappedRwLockReadGuard<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        // SAFETY: the conditions of `RwLockReadGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        unsafe { self.data.as_ref() }
    }
}

impl<P: Poison, T: ?Sized> Deref for MappedRwLockWriteGuard<'_, P, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        unsafe { self.data.as_ref() }
    }
}

impl<P: Poison, T: ?Sized> DerefMut for MappedRwLockWriteGuard<'_, P, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the conditions of `RwLockWriteGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        unsafe { self.data.as_mut() }
    }
}
