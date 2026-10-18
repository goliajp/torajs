//! `performance.now()` / `performance.timeOrigin` (W3C High Resolution
//! Time §4–§5): milliseconds since the time origin on a monotonic
//! clock, and the origin itself as a wall-clock time.
//!
//! The origin is the process's start, which the kernel keeps
//! (`kern.proc.pid` → `p_starttime`). The first call reads it once,
//! together with the wall clock and the monotonic counter at that
//! moment; every later reading is that offset plus the counter's
//! advance, so readings never go backwards when the wall clock is set.

use core::sync::atomic::{AtomicU64, Ordering};

/// `sysctl(int *name, u_int namelen, void *old, size_t *oldlenp,
/// void *new, size_t newlen)` (`<sys/syscall.h>` `SYS_sysctl`).
const SYS_SYSCTL: u32 = 202;
/// `getpid(void)`.
const SYS_GETPID: u32 = 20;
/// `{CTL_KERN, KERN_PROC, KERN_PROC_PID}` (`<sys/sysctl.h>`).
const MIB_KERN_PROC_PID: [i32; 3] = [1, 14, 1];
/// `sizeof(struct kinfo_proc)` on 64-bit Darwin; `kp_proc.p_starttime`
/// (a `struct timeval`) opens it.
const KINFO_PROC_SIZE: usize = 648;

/// Counter ticks at the first reading; 0 until then.
static BASE_TICKS: AtomicU64 = AtomicU64::new(0);
/// `performance.now()` at `BASE_TICKS`, as f64 bits.
static BASE_MS: AtomicU64 = AtomicU64::new(0);
/// `performance.timeOrigin`, as f64 bits.
static ORIGIN_MS: AtomicU64 = AtomicU64::new(0);

/// The generic timer's virtual count (`CNTVCT_EL0`): monotonic, and
/// never set back with the wall clock.
fn ticks() -> u64 {
    let t: u64;
    unsafe { core::arch::asm!("mrs {}, cntvct_el0", out(reg) t, options(nomem, nostack)) };
    t
}

fn tick_hz() -> u64 {
    let f: u64;
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f, options(nomem, nostack)) };
    f
}

fn wall_ms() -> f64 {
    match torajs_syscall::gettimeofday() {
        Ok((sec, usec)) => sec as f64 * 1000.0 + usec as f64 / 1000.0,
        Err(_) => 0.0,
    }
}

/// The process's start as wall-clock milliseconds, `None` when the
/// kernel does not answer.
fn process_start_ms() -> Option<f64> {
    let pid = unsafe { torajs_syscall::syscall0(SYS_GETPID) };
    if pid <= 0 {
        return None;
    }
    let mib = [
        MIB_KERN_PROC_PID[0],
        MIB_KERN_PROC_PID[1],
        MIB_KERN_PROC_PID[2],
        pid as i32,
    ];
    let mut info = [0u8; KINFO_PROC_SIZE];
    let mut len: usize = KINFO_PROC_SIZE;
    let raw = unsafe {
        torajs_syscall::syscall6(
            SYS_SYSCTL,
            mib.as_ptr() as i64,
            mib.len() as i64,
            info.as_mut_ptr() as i64,
            &mut len as *mut usize as i64,
            0,
            0,
        )
    };
    if raw < 0 || len < 16 {
        return None;
    }
    let sec = i64::from_ne_bytes(info[0..8].try_into().ok()?);
    let usec = i32::from_ne_bytes(info[8..12].try_into().ok()?);
    (sec > 0).then(|| sec as f64 * 1000.0 + usec as f64 / 1000.0)
}

fn base() -> (u64, f64) {
    let t = BASE_TICKS.load(Ordering::Relaxed);
    if t != 0 {
        return (t, f64::from_bits(BASE_MS.load(Ordering::Relaxed)));
    }
    let t = ticks();
    let wall = wall_ms();
    let origin = process_start_ms().filter(|s| *s <= wall).unwrap_or(wall);
    let at = wall - origin;
    ORIGIN_MS.store(origin.to_bits(), Ordering::Relaxed);
    BASE_MS.store(at.to_bits(), Ordering::Relaxed);
    BASE_TICKS.store(t.max(1), Ordering::Relaxed);
    (t.max(1), at)
}

/// `performance.now()`.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_perf_now() -> f64 {
    let (t0, at) = base();
    let dt = ticks().saturating_sub(t0);
    at + dt as f64 * 1000.0 / tick_hz() as f64
}

/// `performance.timeOrigin`.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_perf_time_origin() -> f64 {
    base();
    f64::from_bits(ORIGIN_MS.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_monotonic_and_after_the_origin() {
        let a = __torajs_perf_now();
        let b = __torajs_perf_now();
        assert!(a >= 0.0 && b >= a);
        let origin = __torajs_perf_time_origin();
        let wall = wall_ms();
        assert!((origin + b - wall).abs() < 1000.0, "{origin} {b} {wall}");
    }
}
