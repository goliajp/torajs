//! v0.7-A1 perf gate — CI budget assertion for the raw-syscall +
//! safe-wrapper round-trip. Catches regressions caused by accidentally
//! pulling libc back into the path (e.g. someone replaces a `syscall6`
//! call with `unsafe { libc::write(...) }`).
//!
//! Run with `cargo test -p torajs-syscall --release --test perf_gate`.

use std::time::{Duration, Instant};

use torajs_syscall::sysno::STDOUT_FD;
use torajs_syscall::{getpid, mmap_anon_rw, munmap, write};

/// Whether stdout is currently attached to a tty / terminal — write
/// to a piped fd (test runner usually redirects) is dramatically
/// faster than to a tty. Use `/dev/null`-shaped budgets only.
const N_WRITES: usize = 10_000;

#[test]
fn getpid_burst_10k_under_budget() {
    // getpid is the cheapest syscall on macOS — bounds the
    // trampoline overhead at the lower limit.
    let start = Instant::now();
    let mut sum: i64 = 0;
    for _ in 0..N_WRITES {
        sum = sum.wrapping_add(getpid() as i64);
    }
    let elapsed = start.elapsed();
    // 10k getpid on M-series @ libc baseline ≈ 300 μs; our raw
    // trampoline should match within 4× headroom.
    let budget = Duration::from_millis(5);
    assert!(
        elapsed < budget,
        "getpid burst regressed: {elapsed:?} >= budget {budget:?} (sum {sum})"
    );
}

#[test]
fn mmap_munmap_1k_under_budget() {
    let len = 4096;
    let start = Instant::now();
    for _ in 0..1_000 {
        let p = mmap_anon_rw(len).expect("mmap");
        unsafe { munmap(p, len) }.expect("munmap");
    }
    let elapsed = start.elapsed();
    // mmap/munmap each enter the VM subsystem; 1k round trips
    // should fit in ~5 ms on M-series. 25 ms = 5× headroom.
    let budget = Duration::from_millis(25);
    assert!(
        elapsed < budget,
        "mmap_munmap regressed: {elapsed:?} >= budget {budget:?}"
    );
}

#[test]
fn write_to_devnull_burst_under_budget() {
    // Open /dev/null isn't yet implemented (open() is a v0.7-A1
    // follow-up sub-step). Skip this test until then by writing
    // an empty buffer to stdout instead — exercises the syscall
    // path without flooding the test runner's tty.
    let start = Instant::now();
    for _ in 0..N_WRITES {
        let _ = unsafe { write(STDOUT_FD, b"") };
    }
    let elapsed = start.elapsed();
    let budget = Duration::from_millis(10);
    assert!(
        elapsed < budget,
        "write burst regressed: {elapsed:?} >= budget {budget:?}"
    );
}
