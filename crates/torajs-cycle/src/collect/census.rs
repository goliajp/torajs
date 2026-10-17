//! Collector counters, present only under `--cfg torajs_census`: how
//! often it ran, how many roots it walked, and how long in counter
//! ticks (`cntvct_el0`, frequency reported alongside).

use core::sync::atomic::{AtomicU64, Ordering};

static COLLECTS: AtomicU64 = AtomicU64::new(0);
static ROOTS: AtomicU64 = AtomicU64::new(0);
static PARKED_CELLS: AtomicU64 = AtomicU64::new(0);
static PARKED_ENVS: AtomicU64 = AtomicU64::new(0);
static TICKS: AtomicU64 = AtomicU64::new(0);
static MAX_TICKS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn __torajs_census_emit(name: *const u8, len: usize, value: u64);
}

pub fn now() -> u64 {
    let t: u64;
    unsafe { core::arch::asm!("mrs {}, cntvct_el0", out(reg) t, options(nomem, nostack)) };
    t
}

fn freq() -> u64 {
    let f: u64;
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f, options(nomem, nostack)) };
    f
}

/// A cell the root walk freed because it died while buffered.
pub fn parked(env: bool) {
    PARKED_CELLS.fetch_add(1, Ordering::Relaxed);
    if env {
        PARKED_ENVS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn record(roots: u64, start: u64) {
    let dt = now().wrapping_sub(start);
    COLLECTS.fetch_add(1, Ordering::Relaxed);
    ROOTS.fetch_add(roots, Ordering::Relaxed);
    TICKS.fetch_add(dt, Ordering::Relaxed);
    MAX_TICKS.fetch_max(dt, Ordering::Relaxed);
}

fn emit(name: &[u8], v: u64) {
    unsafe { __torajs_census_emit(name.as_ptr(), name.len(), v) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_census_dump() {
    emit(b"cycle.collects", COLLECTS.load(Ordering::Relaxed));
    emit(b"cycle.roots", ROOTS.load(Ordering::Relaxed));
    emit(b"cycle.parked_cells", PARKED_CELLS.load(Ordering::Relaxed));
    emit(b"cycle.parked_envs", PARKED_ENVS.load(Ordering::Relaxed));
    emit(b"cycle.ticks", TICKS.load(Ordering::Relaxed));
    emit(b"cycle.max_ticks", MAX_TICKS.load(Ordering::Relaxed));
    emit(b"cycle.tick_hz", freq());
}
