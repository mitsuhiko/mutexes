//! Size, alignment and auto traits of every lock and guard, in both families.
use std::cell::Cell;
use std::marker::PhantomData;
use std::mem::{align_of, size_of};
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::{self as p, nonpoison as n};

#[repr(align(64))]
#[allow(dead_code)]
struct CacheLine([u8; 64]);

/// A payload that is `UnwindSafe` but neither `RefUnwindSafe` nor `Sync`.
type C = Cell<u32>;

/// Which auto traits `T` implements: an inherent const shadows the trait's.
struct Has<T: ?Sized>(PhantomData<T>);

macro_rules! has {
    ($($tr:ident $flag:ident $fallback:ident),+) => {
        $(
            #[allow(dead_code)]
            trait $fallback { const $flag: bool = false; }
            impl<T: ?Sized> $fallback for Has<T> {}
            #[allow(dead_code)]
            impl<T: ?Sized + $tr> Has<T> { const $flag: bool = true; }
        )+
    };
}
has!(Send SEND NoSend, Sync SYNC NoSync, UnwindSafe UNWIND NoUnwind,
     RefUnwindSafe REF_UNWIND NoRefUnwind, Unpin UNPIN NoUnpin);

pub const TRAITS: [&str; 5] = ["Send", "Sync", "UnwindSafe", "RefUnwindSafe", "Unpin"];

#[repr(C)]
pub struct Entry {
    pub size: usize,
    pub align: usize,
    pub traits: [bool; 5],
}

macro_rules! entry {
    ($t:ty) => {
        Entry {
            size: size_of::<$t>(),
            align: align_of::<$t>(),
            traits: [
                Has::<$t>::SEND,
                Has::<$t>::SYNC,
                Has::<$t>::UNWIND,
                Has::<$t>::REF_UNWIND,
                Has::<$t>::UNPIN,
            ],
        }
    };
}

macro_rules! table {
    ($($t:ident $(<$($arg:tt),+>)?),+ $(,)?) => {
        /// A static too, so that cross-compiled assembly contains the table.
        #[unsafe(no_mangle)]
        #[used]
        pub static LAYOUT: [Entry; 2 * [$(stringify!($t)),+].len()] = [$(
            entry!(p::$t $(<$($arg),+>)?),
            entry!(n::$t $(<$($arg),+>)?),
        )+];
        pub const NAMES: &[&str] = &[$(
            concat!("poison ", stringify!($t $(<$($arg),+>)?)),
            concat!("plain  ", stringify!($t $(<$($arg),+>)?)),
        )+];
    };
}

table!(
    Mutex<()>,
    Mutex<u8>,
    Mutex<u32>,
    Mutex<u64>,
    Mutex<CacheLine>,
    Mutex<C>,
    MutexGuard<'static, u32>,
    MutexGuard<'static, C>,
    MappedMutexGuard<'static, u32>,
    MappedMutexGuard<'static, C>,
    RwLock<()>,
    RwLock<u32>,
    RwLock<u64>,
    RwLock<CacheLine>,
    RwLock<C>,
    RwLockReadGuard<'static, u32>,
    RwLockWriteGuard<'static, u32>,
    RwLockWriteGuard<'static, C>,
    MappedRwLockReadGuard<'static, u32>,
    MappedRwLockWriteGuard<'static, u32>,
    Condvar
);
