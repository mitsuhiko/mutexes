//! Rough throughput check: `./run.sh stock run --release --example bench` vs `shared`.
#![feature(nonpoison_mutex, nonpoison_rwlock, sync_nonpoison)]
use std::hint::black_box;
use std::sync::{self as p, nonpoison as np};
use std::thread;
use std::time::Instant;

const N: u32 = 20_000_000;

fn time(name: &str, f: impl Fn()) {
    f();
    let best = (0..7)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1e9 / N as f64
        })
        .fold(f64::INFINITY, f64::min);
    println!("{name:28} {best:6.2} ns/op");
}

fn contended(f: impl Fn() + Sync) {
    thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| (0..N / 4).for_each(|_| f()));
        }
    });
}

fn main() {
    let pm = p::Mutex::new(0u32);
    let nm = np::Mutex::new(0u32);
    let pr = p::RwLock::new(0u32);
    let nr = np::RwLock::new(0u32);
    time("poison Mutex lock", || {
        (0..N).for_each(|_| *black_box(&pm).lock().unwrap() += 1)
    });
    time("nonpoison Mutex lock", || {
        (0..N).for_each(|_| *black_box(&nm).lock() += 1)
    });
    time("poison RwLock write", || {
        (0..N).for_each(|_| *black_box(&pr).write().unwrap() += 1)
    });
    time("nonpoison RwLock write", || {
        (0..N).for_each(|_| *black_box(&nr).write() += 1)
    });
    time("poison RwLock read", || {
        (0..N).for_each(|_| _ = black_box(*black_box(&pr).read().unwrap()))
    });
    time("poison Mutex 4 threads", || {
        contended(|| *pm.lock().unwrap() += 1)
    });
    time("nonpoison Mutex 4 threads", || {
        contended(|| *nm.lock() += 1)
    });
}
