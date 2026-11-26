//! `Poison<T>` exists only in the shared std: `./run.sh shared test --release --test poison_cell`.
#![feature(poison_cell, sync_poison_mod, sync_nonpoison, nonpoison_mutex)]
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::nonpoison;
use std::sync::poison::Poison;
use std::thread;

fn panics(f: impl FnOnce()) {
    assert!(catch_unwind(AssertUnwindSafe(f)).is_err());
}

#[test]
fn refcell_borrow_poisons_on_panic_and_keeps_the_value() {
    let cell = RefCell::new(Poison::new(vec![1]));
    panics(|| {
        let mut cell = cell.borrow_mut();
        let mut v = cell.guard().unwrap();
        v.push(2);
        panic!();
    });
    assert!(cell.borrow().is_poisoned());
    assert_eq!(**cell.borrow().get().unwrap_err().get_ref(), [1, 2]);
    {
        let mut cell = cell.borrow_mut();
        let mut v = cell.guard().unwrap_err().into_inner();
        v.push(3);
    }
    cell.borrow().clear_poison();
    assert_eq!(cell.into_inner().into_inner().unwrap(), [1, 2, 3]);
}

#[test]
fn nonpoison_mutex_of_poison_behaves_like_a_poisoning_mutex() {
    let m = nonpoison::Mutex::new(Poison::new(0));
    thread::scope(|s| {
        let worker = s.spawn(|| {
            let mut m = m.lock();
            *m.guard().unwrap() = 1;
            let _g = m.guard().unwrap();
            panic!();
        });
        assert!(worker.join().is_err());
    });
    let mut m = m.lock();
    assert!(m.is_poisoned());
    assert_eq!(*m.guard().unwrap_err().into_inner(), 1);
}

#[test]
fn guard_taken_while_panicking_does_not_poison() {
    struct DuringDrop<'a>(&'a RefCell<Poison<u32>>);
    impl Drop for DuringDrop<'_> {
        fn drop(&mut self) {
            *self.0.borrow_mut().guard().unwrap() = 7;
        }
    }
    let cell = RefCell::new(Poison::new(0));
    panics(|| {
        let _during = DuringDrop(&cell);
        panic!();
    });
    assert_eq!(cell.into_inner().into_inner().unwrap(), 7);
}

#[test]
fn unsized_values_and_debug() {
    let mut boxed: Box<Poison<[u8]>> = Box::new(Poison::new([1, 2, 3]));
    boxed.guard().unwrap()[0] = 9;
    assert_eq!(boxed.get().unwrap(), &[9, 2, 3]);
    assert_eq!(
        format!("{:?}", Poison::new(5)),
        "Poison { data: 5, poisoned: false, .. }"
    );
}
