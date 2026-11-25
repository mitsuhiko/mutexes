#![feature(
    nonpoison_mutex,
    nonpoison_rwlock,
    nonpoison_condvar,
    sync_nonpoison,
    mapped_lock_guards
)]
use std::mem::{align_of, size_of};
use std::sync::{self as p, nonpoison as n};

#[repr(align(64))]
#[allow(dead_code)]
struct CacheLine([u8; 64]);

fn layout<T>(name: &str) {
    println!("{name:43} {:3} {:3}", size_of::<T>(), align_of::<T>());
}

macro_rules! pairs {
    ($($t:ident $(<$($arg:tt),+>)?),+ $(,)?) => {
        $(
            layout::<p::$t $(<$($arg),+>)?>(concat!("poison ", stringify!($t $(<$($arg),+>)?)));
            layout::<n::$t $(<$($arg),+>)?>(concat!("plain  ", stringify!($t $(<$($arg),+>)?)));
        )+
    };
}

fn main() {
    println!("type                                        size align");
    pairs!(
        Mutex<()>,
        Mutex<u8>,
        Mutex<u32>,
        Mutex<u64>,
        Mutex<CacheLine>,
        MutexGuard<'static, u32>,
        MappedMutexGuard<'static, u32>,
        RwLock<()>,
        RwLock<u32>,
        RwLock<u64>,
        RwLock<CacheLine>,
        RwLockReadGuard<'static, u32>,
        RwLockWriteGuard<'static, u32>,
        MappedRwLockReadGuard<'static, u32>,
        MappedRwLockWriteGuard<'static, u32>,
        Condvar
    );
}
