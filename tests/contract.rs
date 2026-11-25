#![feature(mapped_lock_guards, sync_nonpoison, nonpoison_mutex, nonpoison_rwlock)]
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, RefUnwindSafe, UnwindSafe, catch_unwind};
use std::sync::{
    Condvar, MappedMutexGuard, Mutex, MutexGuard, RwLock, RwLockWriteGuard, TryLockError,
    nonpoison as plain,
};
use std::thread;
use std::time::Duration;

fn panics(f: impl FnOnce()) {
    assert!(catch_unwind(AssertUnwindSafe(f)).is_err());
}

#[test]
fn try_lock_distinguishes_poison_from_contention_and_preserves_data() {
    let mut m = Mutex::new(0);
    panics(|| {
        let mut g = m.lock().unwrap();
        *g = 42;
        panic!();
    });
    let Err(TryLockError::Poisoned(error)) = m.try_lock() else {
        panic!()
    };
    let g = error.into_inner();
    assert_eq!(*g, 42);
    assert!(matches!(m.try_lock(), Err(TryLockError::WouldBlock)));
    drop(g);
    assert_eq!(*m.get_mut().unwrap_err().into_inner(), 42);
    m.clear_poison();
    assert_eq!(m.into_inner().unwrap(), 42);
}

#[test]
fn filter_miss_keeps_guard_and_panicking_remap_poisons() {
    let m = Mutex::new([3]);
    panics(|| {
        let g = m.lock().unwrap();
        let g = MutexGuard::filter_map::<u8, _>(g, |_| None).unwrap_err();
        assert!(matches!(m.try_lock(), Err(TryLockError::WouldBlock)));
        let g = MutexGuard::map(g, |a| &mut a[0]);
        let _g = MappedMutexGuard::filter_map::<u8, _>(g, |_| panic!());
    });
    assert_eq!(*m.lock().unwrap_err().into_inner(), [3]);
}

#[test]
fn mapped_writer_drops_before_unlock_but_mapped_readers_never_poison() {
    let r = RwLock::new([1]);
    panics(|| {
        let g = std::sync::RwLockReadGuard::map(r.read().unwrap(), |a| &a[0]);
        assert_eq!(*g, 1);
        panic!();
    });
    assert!(!r.is_poisoned());
    panics(|| {
        let mut g = RwLockWriteGuard::map(r.write().unwrap(), |a| &mut a[0]);
        *g = 2;
        panic!();
    });
    let w = r.write().unwrap_err().into_inner();
    let read = RwLockWriteGuard::downgrade(w);
    assert_eq!(read[0], 2);
    assert!(matches!(r.try_read(), Err(TryLockError::Poisoned(_))));
    assert!(matches!(r.try_write(), Err(TryLockError::WouldBlock)));
}

#[test]
fn acquisition_during_unwind_is_not_a_new_panic() {
    struct DuringDrop<'a>(&'a Mutex<[u32; 1]>);
    impl Drop for DuringDrop<'_> {
        fn drop(&mut self) {
            let g = self.0.lock().unwrap();
            *MutexGuard::map(g, |a| &mut a[0]) = 7;
        }
    }
    let m = Mutex::new([0]);
    panics(|| {
        let _during = DuringDrop(&m);
        panic!();
    });
    assert_eq!(m.into_inner().unwrap(), [7]);
}

#[test]
fn mapping_does_not_reset_a_pre_panic_snapshot() {
    struct MapOnDrop<'a>(
        Option<MutexGuard<'a, [u8; 1]>>,
        &'a Mutex<[u8; 1]>,
        &'a Cell<bool>,
    );
    impl Drop for MapOnDrop<'_> {
        fn drop(&mut self) {
            let g = MutexGuard::map(self.0.take().unwrap(), |a| &mut a[0]);
            self.2.set(self.1.is_poisoned());
            drop(g);
        }
    }
    let m = Mutex::new([0]);
    let poisoned_while_held = Cell::new(true);
    panics(|| {
        let _during = MapOnDrop(Some(m.lock().unwrap()), &m, &poisoned_while_held);
        panic!();
    });
    assert!(!poisoned_while_held.get(), "mapping is not releasing");
    assert!(m.is_poisoned());
}

#[test]
fn wait_observes_the_other_threads_poison_before_rerunning_user_code() {
    let m = Mutex::new(false);
    let cv = Condvar::new();
    thread::scope(|s| {
        let g = m.lock().unwrap();
        let worker = s.spawn(|| {
            let mut g = m.lock().unwrap();
            *g = true;
            cv.notify_one();
            panic!();
        });
        let err = cv
            .wait_timeout_while(g, Duration::from_secs(10), |ready| {
                assert!(!*ready, "predicate called after poison");
                true
            })
            .unwrap_err();
        let (g, _) = err.into_inner();
        assert!(*g);
        assert!(matches!(m.try_lock(), Err(TryLockError::WouldBlock)));
        assert!(worker.join().is_err());
    });
}

#[test]
fn plain_exclusive_guards_unlock_without_poison() {
    let m = plain::Mutex::new(0);
    let r = plain::RwLock::new(0);
    panics(|| {
        let mut a = m.lock();
        let mut b = r.write();
        *a = 2;
        *b = 3;
        panic!();
    });
    assert_eq!(*m.try_lock().unwrap(), 2);
    assert_eq!(*r.try_read().unwrap(), 3);
}

#[test]
fn unwindsafe_contract_and_unsized_payload_survive_wrapping() {
    fn unwind_safe<T: UnwindSafe + RefUnwindSafe>() {}
    unwind_safe::<MutexGuard<'_, Cell<i32>>>();
    unwind_safe::<RwLockWriteGuard<'_, Cell<i32>>>();
    let boxed: Box<Mutex<[u8]>> = Box::new(Mutex::new([1, 2, 3]));
    assert_eq!(&*boxed.lock().unwrap(), &[1, 2, 3]);
}
