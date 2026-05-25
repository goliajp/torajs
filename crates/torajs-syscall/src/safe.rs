//! Safe-ish typed wrappers over the raw syscall trampoline.
//!
//! Each function decodes the negative-errno convention from the
//! raw trampoline into a `Result<T, Errno>`. They stay `unsafe`
//! where the underlying syscall demands valid pointers / mapped
//! memory (write/read/munmap), and safe where they don't (exit,
//! getpid).

use crate::sysno::*;

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
use crate::arch_aarch64_macos::{syscall0, syscall1, syscall3, syscall6};

/// Raw `errno` value (positive, matches `<sys/errno.h>`). Zero is
/// not a valid errno — wrappers that report success use `Ok(...)`
/// instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Errno(pub i32);

#[inline]
fn decode(raw: i64) -> Result<i64, Errno> {
    if raw < 0 {
        Err(Errno((-raw) as i32))
    } else {
        Ok(raw)
    }
}

/// `write(fd, buf) -> Ok(bytes_written) | Err(errno)`.
///
/// # Safety
///
/// `buf` must point to at least `buf.len()` valid bytes.
pub unsafe fn write(fd: i32, buf: &[u8]) -> Result<usize, Errno> {
    let n = unsafe { syscall3(SYS_WRITE, fd as i64, buf.as_ptr() as i64, buf.len() as i64) };
    decode(n).map(|x| x as usize)
}

/// `read(fd, buf) -> Ok(bytes_read) | Err(errno)`. Zero bytes
/// means EOF.
///
/// # Safety
///
/// `buf` must point to at least `buf.len()` writable bytes.
pub unsafe fn read(fd: i32, buf: &mut [u8]) -> Result<usize, Errno> {
    let n = unsafe {
        syscall3(
            SYS_READ,
            fd as i64,
            buf.as_mut_ptr() as i64,
            buf.len() as i64,
        )
    };
    decode(n).map(|x| x as usize)
}

/// `exit(code) -> !`. Process terminates; never returns.
pub fn exit(code: i32) -> ! {
    unsafe { syscall1(SYS_EXIT, code as i64) };
    // Unreachable — kernel won't return from SYS_EXIT. The loop
    // satisfies the `!` return type if the syscall ever did.
    loop {
        core::hint::spin_loop();
    }
}

/// `getpid() -> pid` — current process id. Always succeeds with a
/// positive value.
pub fn getpid() -> i32 {
    let n = unsafe { syscall0(SYS_GETPID) };
    n as i32
}

/// `mmap` with the canonical "give me fresh zero-filled memory"
/// flags (`MAP_PRIVATE | MAP_ANON`, read+write, no backing file).
///
/// Returns a pointer to the start of the new region (kernel
/// zero-initialized) or `Err(errno)` on failure (the kernel
/// returns `MAP_FAILED` = -1 as a userspace sentinel, but at the
/// raw syscall level we see negative errno directly).
pub fn mmap_anon_rw(len: usize) -> Result<*mut u8, Errno> {
    let raw = unsafe {
        syscall6(
            SYS_MMAP,
            0, // addr — let kernel pick
            len as i64,
            (PROT_READ | PROT_WRITE) as i64,
            (MAP_PRIVATE | MAP_ANON) as i64,
            -1, // fd — required to be -1 for anon
            0,  // offset
        )
    };
    decode(raw).map(|p| p as *mut u8)
}

/// `munmap(addr, len)`. Caller MUST pass an `addr` that was
/// returned by a prior `mmap_anon_rw` (or other mmap variant)
/// and a matching `len`.
///
/// # Safety
///
/// Caller is responsible for the addr/len round-trip — passing
/// `(NULL, 0)` is harmless; passing arbitrary addresses is UB.
pub unsafe fn munmap(addr: *mut u8, len: usize) -> Result<(), Errno> {
    let raw = unsafe { syscall3(SYS_MUNMAP, addr as i64, len as i64, 0) };
    decode(raw).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_to_stdout() {
        let msg = b"v0.7-A1 step4 safe wrapper write\n";
        let n = unsafe { write(STDOUT_FD, msg) }.expect("write");
        assert_eq!(n, msg.len());
    }

    #[test]
    fn getpid_is_positive() {
        let pid = getpid();
        assert!(pid > 0);
    }

    #[test]
    fn mmap_roundtrip() {
        let len = 4096;
        let p = mmap_anon_rw(len).expect("mmap");
        // kernel zeroes anon memory; write + read sanity check
        unsafe {
            *p = 0xab;
            *p.add(4095) = 0xcd;
            assert_eq!(*p, 0xab);
            assert_eq!(*p.add(4095), 0xcd);
        }
        unsafe { munmap(p, len) }.expect("munmap");
    }

    #[test]
    fn write_with_bad_fd_returns_errno() {
        let bad_fd = 99999;
        let err = unsafe { write(bad_fd, b"x") }.expect_err("expected EBADF");
        // EBADF on macOS = 9
        assert_eq!(err.0, 9, "got errno {}", err.0);
    }
}
