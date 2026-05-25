//! aarch64 macOS syscall trampoline.
//!
//! ABI summary (from XNU `osfmk/arm64/locore.s` + `bsd/dev/arm64/systemcalls.c`):
//! - syscall number → `x16` (NOT `x8` like Linux aarch64)
//! - args → `x0..x5` (up to 6 args)
//! - trap instruction → `svc #0x80`
//! - return value → `x0` (result on success, positive `errno` on
//!   error). **Error is indicated by the carry flag (`C` in NZCV)
//!   being SET** — NOT by `x0 < 0` like Linux. A positive `x0`
//!   could legitimately be either a result or an errno depending
//!   on the carry bit.
//!
//! The trampoline re-encodes the carry-set case as a negative
//! return (`neg x0, x0` post-svc) so the safe-wrapper layer can
//! decode using the canonical Linux convention (`raw < 0 → -errno`).

use core::arch::asm;

/// 6-argument raw syscall. Returns `x0` verbatim — negative
/// values are `-errno`.
///
/// # Safety
///
/// Caller is responsible for passing a valid sysno and
/// well-formed args matching the kernel's expected types for that
/// number. Calling `SYS_MUNMAP` on memory that wasn't `SYS_MMAP`'d
/// is UB at the kernel level; calling `SYS_READ` with a buf
/// pointer outside the current process's address space is UB; etc.
#[inline]
pub unsafe fn syscall6(sysno: u32, a0: i64, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "svc #0x80",
            // If carry set (= error), negate x0 so caller sees
            // Linux-style `-errno`. b.cc = "branch if carry clear"
            // (success path); skips the neg.
            "b.cc 1f",
            "neg x0, x0",
            "1:",
            in("x16") sysno as i64,
            inlateout("x0") a0 => ret,
            in("x1") a1,
            in("x2") a2,
            in("x3") a3,
            in("x4") a4,
            in("x5") a5,
            options(nostack),
        );
    }
    ret
}

/// 3-argument raw syscall — convenience for the dominant case
/// (`write` / `read` / `lseek` etc).
#[inline]
pub unsafe fn syscall3(sysno: u32, a0: i64, a1: i64, a2: i64) -> i64 {
    unsafe { syscall6(sysno, a0, a1, a2, 0, 0, 0) }
}

/// 1-argument raw syscall — `exit` / `close` / `getpid`-style.
#[inline]
pub unsafe fn syscall1(sysno: u32, a0: i64) -> i64 {
    unsafe { syscall6(sysno, a0, 0, 0, 0, 0, 0) }
}

/// 0-argument raw syscall — `getpid` etc.
#[inline]
pub unsafe fn syscall0(sysno: u32) -> i64 {
    unsafe { syscall6(sysno, 0, 0, 0, 0, 0, 0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysno::{STDOUT_FD, SYS_GETPID, SYS_WRITE};

    /// raw write to stdout via syscall — verifies the trampoline
    /// passes args + returns the byte count.
    #[test]
    fn raw_write_to_stdout_returns_byte_count() {
        const MSG: &str = "v0.7-A1 step3 raw syscall write OK\n";
        let n = unsafe {
            syscall3(
                SYS_WRITE,
                STDOUT_FD as i64,
                MSG.as_ptr() as i64,
                MSG.len() as i64,
            )
        };
        assert_eq!(
            n,
            MSG.len() as i64,
            "write returned {n}, expected {}",
            MSG.len()
        );
    }

    /// `getpid` is a 0-arg syscall that always succeeds with a
    /// positive value — sanity-checks the trampoline's no-arg
    /// path + the return-value plumbing.
    #[test]
    fn getpid_returns_positive() {
        let pid = unsafe { syscall0(SYS_GETPID) };
        assert!(pid > 0, "getpid returned {pid}");
    }
}
