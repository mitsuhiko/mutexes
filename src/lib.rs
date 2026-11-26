//! Codegen probes for both families, and the layout table. compare.sh diffs both.
#![feature(
    nonpoison_mutex,
    nonpoison_rwlock,
    nonpoison_condvar,
    sync_nonpoison,
    mapped_lock_guards
)]
use std::sync::{self as p, nonpoison as np};
use std::time::Duration;

pub mod layout;

#[unsafe(no_mangle)]
pub fn np_lock_inc(m: &np::Mutex<u32>) {
    *m.lock() += 1;
}

#[unsafe(no_mangle)]
pub fn np_try_lock_inc(m: &np::Mutex<u32>) -> bool {
    let Ok(mut guard) = m.try_lock() else {
        return false;
    };
    *guard += 1;
    true
}

#[unsafe(no_mangle)]
pub fn np_map(m: &np::Mutex<[u32; 2]>) {
    *np::MutexGuard::map(m.lock(), |v| &mut v[1]) += 1;
}

#[unsafe(no_mangle)]
pub fn np_read(m: &np::RwLock<u32>) -> u32 {
    *m.read()
}

#[unsafe(no_mangle)]
pub fn np_try_read(m: &np::RwLock<u32>) -> Option<u32> {
    Some(*m.try_read().ok()?)
}

#[unsafe(no_mangle)]
pub fn np_write(m: &np::RwLock<u32>) {
    *m.write() += 1;
}

#[unsafe(no_mangle)]
pub fn np_try_write(m: &np::RwLock<u32>) -> bool {
    let Ok(mut guard) = m.try_write() else {
        return false;
    };
    *guard += 1;
    true
}

#[unsafe(no_mangle)]
pub fn np_downgrade(m: &np::RwLock<u32>) -> u32 {
    *np::RwLockWriteGuard::downgrade(m.write())
}

#[unsafe(no_mangle)]
pub fn np_wait(c: &np::Condvar, g: &mut np::MutexGuard<'_, bool>) {
    c.wait_while(g, |ready| !*ready)
}

#[unsafe(no_mangle)]
pub fn np_timed_wait(c: &np::Condvar, g: &mut np::MutexGuard<'_, bool>, d: Duration) -> bool {
    c.wait_timeout_while(g, d, |ready| !*ready).timed_out()
}

#[unsafe(no_mangle)]
pub fn np_notify(c: &np::Condvar) {
    c.notify_one()
}

#[unsafe(no_mangle)]
pub fn p_lock_inc(m: &p::Mutex<u32>) {
    *m.lock().unwrap() += 1;
}

#[unsafe(no_mangle)]
pub fn p_lock_recover(m: &p::Mutex<u32>) {
    *m.lock().unwrap_or_else(p::PoisonError::into_inner) += 1;
}

#[unsafe(no_mangle)]
pub fn p_try_lock_inc(m: &p::Mutex<u32>) -> bool {
    let Ok(mut guard) = m.try_lock() else {
        return false;
    };
    *guard += 1;
    true
}

#[unsafe(no_mangle)]
pub fn p_map(m: &p::Mutex<[u32; 2]>) {
    *p::MutexGuard::map(m.lock().unwrap(), |v| &mut v[1]) += 1;
}

#[unsafe(no_mangle)]
pub fn p_is_poisoned(m: &p::Mutex<u32>) -> bool {
    m.is_poisoned()
}

#[unsafe(no_mangle)]
pub fn p_read(m: &p::RwLock<u32>) -> u32 {
    *m.read().unwrap()
}

#[unsafe(no_mangle)]
pub fn p_try_read(m: &p::RwLock<u32>) -> Option<u32> {
    Some(*m.try_read().ok()?)
}

#[unsafe(no_mangle)]
pub fn p_write(m: &p::RwLock<u32>) {
    *m.write().unwrap() += 1;
}

#[unsafe(no_mangle)]
pub fn p_try_write(m: &p::RwLock<u32>) -> bool {
    let Ok(mut guard) = m.try_write() else {
        return false;
    };
    *guard += 1;
    true
}

#[unsafe(no_mangle)]
pub fn p_downgrade(m: &p::RwLock<u32>) -> u32 {
    *p::RwLockWriteGuard::downgrade(m.write().unwrap())
}

#[unsafe(no_mangle)]
pub fn p_wait(c: &p::Condvar, g: p::MutexGuard<'_, bool>) -> bool {
    c.wait_while(g, |ready| !*ready).is_ok()
}

#[unsafe(no_mangle)]
pub fn p_timed_wait(c: &p::Condvar, g: p::MutexGuard<'_, bool>, d: Duration) -> bool {
    c.wait_timeout_while(g, d, |ready| !*ready)
        .is_ok_and(|(_, r)| r.timed_out())
}

#[unsafe(no_mangle)]
pub fn p_notify(c: &p::Condvar) {
    c.notify_one()
}
