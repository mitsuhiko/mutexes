use super::{Flag, Guard, LockResult, map_result};
use crate::fmt;
use crate::ops::{Deref, DerefMut};

/// A value with a poison flag, for any container that hands out exclusive
/// access.
///
/// `Poison<T>` adds the poisoning of [`Mutex`] to other ways of getting
/// `&mut T`: a [`RefCell`], a [`nonpoison::Mutex`], a lock from another crate,
/// or plain ownership. Exclusive access goes through [`guard`]. If the thread
/// starts panicking while the [`PoisonGuard`] is alive, the value is poisoned,
/// and later [`guard`], [`get`] and [`into_inner`] calls return a
/// [`PoisonError`] that still carries the access.
///
/// It uses the same flag and the same rules as [`Mutex`] and [`RwLock`]: a
/// guard taken while the thread is already panicking does not poison, and
/// shared access only reports existing poison.
///
/// # Not a replacement for the poisoning locks
///
/// `nonpoison::Mutex<Poison<T>>` behaves like [`Mutex<T>`], but it is not the
/// same size for many `T`. `Poison<T>` has to carry its flag, because it can be
/// moved and swapped on its own, so the flag is padded out to `T`'s
/// alignment. Rust never places a field into another field's padding, so the
/// mutex cannot reuse that space. [`Mutex<T>`] instead keeps the flag next to the
/// system lock, where it usually fits into padding that is already there. On
/// Linux a `Mutex<Vec<u8>>` is 32 bytes, but a
/// `nonpoison::Mutex<Poison<Vec<u8>>>` is 40. For this reason the poisoning
/// locks are not built from `Poison<T>`. They share their flag handling with it
/// instead.
///
/// # Examples
///
/// ```
/// #![feature(poison_cell, sync_poison_mod)]
/// use std::cell::RefCell;
/// use std::panic::{self, AssertUnwindSafe};
/// use std::sync::poison::Poison;
///
/// let cell = RefCell::new(Poison::new(vec![1, 2, 3]));
///
/// let _ = panic::catch_unwind(AssertUnwindSafe(|| {
///     let mut cell = cell.borrow_mut();
///     let mut v = cell.guard().unwrap();
///     v.push(4);
///     panic!("half-way through an update");
/// }));
///
/// let cell = cell.borrow();
/// let err = cell.get().unwrap_err();
/// assert_eq!(**err.get_ref(), [1, 2, 3, 4]);
/// ```
///
/// [`Mutex`]: super::Mutex
/// [`Mutex<T>`]: super::Mutex
/// [`RwLock`]: super::RwLock
/// [`RefCell`]: crate::cell::RefCell
/// [`nonpoison::Mutex`]: crate::sync::nonpoison::Mutex
/// [`PoisonError`]: super::PoisonError
/// [`guard`]: Poison::guard
/// [`get`]: Poison::get
/// [`into_inner`]: Poison::into_inner
#[unstable(feature = "poison_cell", issue = "none")]
pub struct Poison<T: ?Sized> {
    flag: Flag,
    value: T,
}

/// Exclusive access to the value of a [`Poison`], created by [`Poison::guard`].
///
/// If the thread starts panicking while this guard is alive, dropping it
/// poisons the value.
#[must_use = "if unused the guard immediately ends its access"]
#[unstable(feature = "poison_cell", issue = "none")]
#[clippy::has_significant_drop]
pub struct PoisonGuard<'a, T: ?Sized + 'a> {
    poison: &'a mut Poison<T>,
    guard: Guard,
}

impl<T> Poison<T> {
    /// Creates a new, unpoisoned `Poison<T>`.
    #[unstable(feature = "poison_cell", issue = "none")]
    #[inline]
    pub const fn new(value: T) -> Poison<T> {
        Poison { flag: Flag::new(), value }
    }

    /// Returns the value, or a [`PoisonError`](super::PoisonError) carrying it
    /// if it is poisoned.
    #[unstable(feature = "poison_cell", issue = "none")]
    #[inline]
    pub fn into_inner(self) -> LockResult<T> {
        map_result(self.flag.borrow(), |()| self.value)
    }
}

impl<T: ?Sized> Poison<T> {
    /// Takes exclusive access, which poisons the value if the thread starts
    /// panicking before the guard is dropped.
    ///
    /// Returns an error carrying the guard if the value is already poisoned.
    #[unstable(feature = "poison_cell", issue = "none")]
    #[inline]
    pub fn guard(&mut self) -> LockResult<PoisonGuard<'_, T>> {
        map_result(self.flag.guard(), |guard| PoisonGuard { poison: self, guard })
    }

    /// Returns shared access, or an error carrying it if the value is poisoned.
    ///
    /// Shared access never poisons. Changes made through interior mutability
    /// are not tracked.
    #[unstable(feature = "poison_cell", issue = "none")]
    #[inline]
    pub fn get(&self) -> LockResult<&T> {
        map_result(self.flag.borrow(), |()| &self.value)
    }

    /// Returns whether the value is poisoned.
    #[unstable(feature = "poison_cell", issue = "none")]
    #[inline]
    pub fn is_poisoned(&self) -> bool {
        self.flag.get()
    }

    /// Clears the poisoned state.
    #[unstable(feature = "poison_cell", issue = "none")]
    #[inline]
    pub fn clear_poison(&self) {
        self.flag.clear();
    }
}

#[unstable(feature = "poison_cell", issue = "none")]
impl<T: Default> Default for Poison<T> {
    fn default() -> Poison<T> {
        Poison::new(T::default())
    }
}

#[unstable(feature = "poison_cell", issue = "none")]
impl<T> From<T> for Poison<T> {
    fn from(value: T) -> Poison<T> {
        Poison::new(value)
    }
}

#[unstable(feature = "poison_cell", issue = "none")]
impl<T: ?Sized + fmt::Debug> fmt::Debug for Poison<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Poison")
            .field("data", &&self.value)
            .field("poisoned", &self.flag.get())
            .finish_non_exhaustive()
    }
}

#[unstable(feature = "poison_cell", issue = "none")]
impl<T: ?Sized> Deref for PoisonGuard<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.poison.value
    }
}

#[unstable(feature = "poison_cell", issue = "none")]
impl<T: ?Sized> DerefMut for PoisonGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.poison.value
    }
}

#[unstable(feature = "poison_cell", issue = "none")]
impl<T: ?Sized> Drop for PoisonGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        self.poison.flag.done(&self.guard);
    }
}

#[unstable(feature = "poison_cell", issue = "none")]
impl<T: ?Sized + fmt::Debug> fmt::Debug for PoisonGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

#[unstable(feature = "poison_cell", issue = "none")]
impl<T: ?Sized + fmt::Display> fmt::Display for PoisonGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
