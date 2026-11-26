//! `Mutex<T>` vs `nonpoison::Mutex<Poison<T>>`, for composition.sh.
//! Only built against the shared std (`--cfg poison_cell`).
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::poison::Poison;
use std::sync::{self as p, nonpoison as n};

use crate::layout::CacheLine;

macro_rules! sizes {
    ($($t:ty),+) => {
        /// Keeps the layouts alive so that `-Zprint-type-sizes` reports them.
        #[unsafe(no_mangle)]
        #[used]
        pub static COMPOSITION: [usize; 4 * [$(stringify!($t)),+].len()] = [$(
            size_of::<p::Mutex<$t>>(),
            size_of::<n::Mutex<Poison<$t>>>(),
            size_of::<RefCell<$t>>(),
            size_of::<RefCell<Poison<$t>>>(),
        )+];
    };
}

sizes!((), u8, u32, u64, u128, Box<u8>, Vec<u8>, String, HashMap<u8, u8>, CacheLine);
