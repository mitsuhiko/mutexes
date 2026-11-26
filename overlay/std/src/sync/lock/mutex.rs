use super::Poison;
use crate::cell::UnsafeCell;
use crate::marker::PhantomData;
use crate::mem::ManuallyDrop;
use crate::ops::{Deref, DerefMut};
use crate::ptr::NonNull;
use crate::sys::sync as sys;

pub(crate) struct Mutex<P, T: ?Sized> {
    pub(super) inner: sys::Mutex,
    pub(crate) poison: P,
    data: UnsafeCell<T>,
}

pub(crate) struct MutexGuard<'a, P: Poison, T: ?Sized + 'a> {
    pub(super) lock: &'a Mutex<P, T>,
    poison: P::Guard,
}

pub(crate) struct MappedMutexGuard<'a, P: Poison, T: ?Sized + 'a> {
    // NB: we use a pointer instead of `&'a mut T` to avoid `noalias` violations, because a
    // `MappedMutexGuard` argument doesn't hold uniqueness for its whole scope, only until it drops.
    // `NonNull` is covariant over `T`, so we add a `PhantomData<&'a mut T>` field
    // below for the correct variance over `T` (invariance).
    data: NonNull<T>,
    inner: &'a sys::Mutex,
    poison_flag: P::Ptr,
    poison: P::Guard,
    _variance: PhantomData<&'a mut T>,
}

impl<P, T> Mutex<P, T> {
    #[inline(always)]
    pub(crate) const fn new(poison: P, t: T) -> Self {
        Mutex { inner: sys::Mutex::new(), poison, data: UnsafeCell::new(t) }
    }

    #[inline(always)]
    pub(crate) fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

/// Acquisitions take the public guard's constructor, so that both families
/// build their guard in one step: `G` for nonpoison, `LockResult<G>` for poison.
impl<P: Poison, T: ?Sized> Mutex<P, T> {
    #[inline(always)]
    pub(crate) fn lock<'a, G>(
        &'a self,
        wrap: impl FnOnce(MutexGuard<'a, P, T>) -> G,
    ) -> P::Result<G> {
        unsafe {
            self.inner.lock();
            MutexGuard::new(self, wrap)
        }
    }

    #[inline(always)]
    pub(crate) fn try_lock<'a, G>(
        &'a self,
        wrap: impl FnOnce(MutexGuard<'a, P, T>) -> G,
    ) -> Option<P::Result<G>> {
        unsafe { if self.inner.try_lock() { Some(MutexGuard::new(self, wrap)) } else { None } }
    }
}

impl<P, T: ?Sized> Mutex<P, T> {
    #[inline(always)]
    pub(crate) fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }

    #[inline(always)]
    pub(crate) const fn data_ptr(&self) -> *mut T {
        self.data.get()
    }
}

impl<'a, P: Poison, T: ?Sized> MutexGuard<'a, P, T> {
    /// # Safety
    /// The mutex must be locked by the current thread.
    #[inline(always)]
    unsafe fn new<G>(lock: &'a Mutex<P, T>, wrap: impl FnOnce(Self) -> G) -> P::Result<G> {
        lock.poison.enter(|poison| wrap(MutexGuard { lock, poison }))
    }

    #[inline(always)]
    pub(crate) fn poison(&self) -> &'a P {
        &self.lock.poison
    }

    #[inline(always)]
    pub(crate) fn map<U, F>(orig: Self, f: F) -> MappedMutexGuard<'a, P, U>
    where
        F: FnOnce(&mut T) -> &mut U,
        U: ?Sized,
    {
        // SAFETY: the conditions of `MutexGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the reference
        // passed to it. If the closure panics, the guard will be dropped.
        let data = NonNull::from(f(unsafe { &mut *orig.lock.data.get() }));
        let orig = ManuallyDrop::new(orig);
        MappedMutexGuard {
            data,
            inner: &orig.lock.inner,
            poison_flag: orig.lock.poison.ptr(),
            poison: orig.poison.clone(),
            _variance: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn filter_map<U, F>(orig: Self, f: F) -> Result<MappedMutexGuard<'a, P, U>, Self>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
        U: ?Sized,
    {
        // SAFETY: the conditions of `MutexGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the reference
        // passed to it. If the closure panics, the guard will be dropped.
        match f(unsafe { &mut *orig.lock.data.get() }) {
            Some(data) => {
                let data = NonNull::from(data);
                let orig = ManuallyDrop::new(orig);
                Ok(MappedMutexGuard {
                    data,
                    inner: &orig.lock.inner,
                    poison_flag: orig.lock.poison.ptr(),
                    poison: orig.poison.clone(),
                    _variance: PhantomData,
                })
            }
            None => Err(orig),
        }
    }
}

impl<P: Poison, T: ?Sized> Deref for MutexGuard<'_, P, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<P: Poison, T: ?Sized> DerefMut for MutexGuard<'_, P, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<P: Poison, T: ?Sized> Drop for MutexGuard<'_, P, T> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            self.lock.poison.leave(&self.poison);
            self.lock.inner.unlock();
        }
    }
}

impl<'a, P: Poison, T: ?Sized> MappedMutexGuard<'a, P, T> {
    #[inline(always)]
    pub(crate) fn map<U, F>(mut orig: Self, f: F) -> MappedMutexGuard<'a, P, U>
    where
        F: FnOnce(&mut T) -> &mut U,
        U: ?Sized,
    {
        // SAFETY: the conditions of `MutexGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the reference
        // passed to it. If the closure panics, the guard will be dropped.
        let data = NonNull::from(f(unsafe { orig.data.as_mut() }));
        let orig = ManuallyDrop::new(orig);
        MappedMutexGuard {
            data,
            inner: orig.inner,
            poison_flag: orig.poison_flag,
            poison: orig.poison.clone(),
            _variance: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn filter_map<U, F>(mut orig: Self, f: F) -> Result<MappedMutexGuard<'a, P, U>, Self>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
        U: ?Sized,
    {
        // SAFETY: the conditions of `MutexGuard::new` were satisfied when the original guard
        // was created, and have been upheld throughout `map` and/or `filter_map`.
        // The signature of the closure guarantees that it will not "leak" the lifetime of the reference
        // passed to it. If the closure panics, the guard will be dropped.
        match f(unsafe { orig.data.as_mut() }) {
            Some(data) => {
                let data = NonNull::from(data);
                let orig = ManuallyDrop::new(orig);
                Ok(MappedMutexGuard {
                    data,
                    inner: orig.inner,
                    poison_flag: orig.poison_flag,
                    poison: orig.poison.clone(),
                    _variance: PhantomData,
                })
            }
            None => Err(orig),
        }
    }
}

impl<P: Poison, T: ?Sized> Deref for MappedMutexGuard<'_, P, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        unsafe { self.data.as_ref() }
    }
}

impl<P: Poison, T: ?Sized> DerefMut for MappedMutexGuard<'_, P, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.data.as_mut() }
    }
}

impl<P: Poison, T: ?Sized> Drop for MappedMutexGuard<'_, P, T> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            P::leave_ptr(self.poison_flag, &self.poison);
            self.inner.unlock();
        }
    }
}
