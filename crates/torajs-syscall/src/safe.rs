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

/// `getentropy(buf) -> Ok(()) | Err(errno)` — fill `buf` with
/// `buf.len()` cryptographically-secure random bytes from the kernel
/// CSPRNG. The kernel caps a single request at 256 bytes; larger
/// requests fail with `EIO`.
///
/// This is the metal-level seed source for `Math.random` — it
/// replaces `SystemTime::now()` (which drags libc `clock_gettime` /
/// `__error` / `strerror_r` / `memset` into the user binary) with a
/// single `svc` and gives crypto-quality entropy (the seed quality
/// V8 / SpiderMonkey use), not wall-clock time.
///
/// # Safety
///
/// `buf` must point to at least `buf.len()` writable bytes.
pub unsafe fn getentropy(buf: &mut [u8]) -> Result<(), Errno> {
    let raw = unsafe { syscall3(SYS_GETENTROPY, buf.as_mut_ptr() as i64, buf.len() as i64, 0) };
    decode(raw).map(|_| ())
}

/// macOS 64-bit userland `struct timeval`. `tv_sec` is
/// `__darwin_time_t` (`long`, 64-bit); `tv_usec` is
/// `__darwin_suseconds_t` (`__int32_t`, 32-bit) — verified against the
/// SDK `<sys/_types/_timeval.h>`. Under `#[repr(C)]` this lays out as
/// 8 bytes `tv_sec` + 4 bytes `tv_usec` + 4 bytes tail padding = 16
/// bytes, matching XNU's `user64_timeval` `copyout` layout exactly.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Timeval {
    tv_sec: i64,
    tv_usec: i32,
}

/// `gettimeofday() -> Ok((tv_sec, tv_usec)) | Err(errno)` — wall-clock
/// time since the UNIX epoch (`tv_sec` seconds, `tv_usec` the
/// sub-second microseconds remainder).
///
/// This is the metal-level time source for `Date.now()` / `new Date()`.
/// It replaces `std::time::SystemTime::now()`, which drags libc
/// `clock_gettime` / `__error` / `strerror_r` into the user binary:
/// macOS has no `clock_gettime` syscall, so std routes it through the
/// libc commpage wrapper. A single `svc` to `SYS_gettimeofday` has the
/// kernel `copyout` the result into our `timeval`, and `decode` never
/// touches libc `errno` — which is exactly why the syscall path stays
/// free of `__error` / `strerror_r`.
///
/// Safe (no caller-supplied pointers): the `timeval` is stack-local and
/// the `tzp` / `mach_absolute_time` out-pointers are passed NULL.
pub fn gettimeofday() -> Result<(i64, i32), Errno> {
    let mut tv = Timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let raw = unsafe { syscall3(SYS_GETTIMEOFDAY, &mut tv as *mut Timeval as i64, 0, 0) };
    // On success XNU returns 0 (or echoes tv_sec via x0 on some
    // kernels) — both non-negative, so `decode` yields `Ok` and we read
    // the authoritative value from the `copyout`'d struct. On failure
    // x0 carries a negative errno, which `decode` surfaces as `Err`.
    decode(raw).map(|_| (tv.tv_sec, tv.tv_usec))
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

    #[test]
    fn getentropy_fills_distinct_bytes() {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        unsafe { getentropy(&mut a) }.expect("getentropy a");
        unsafe { getentropy(&mut b) }.expect("getentropy b");
        // not all-zero (kernel filled it) and two draws differ
        // (collision over 128 bits is astronomically unlikely)
        assert_ne!(a, [0u8; 16], "buffer left all-zero");
        assert_ne!(a, b, "two entropy draws were identical");
    }

    #[test]
    fn gettimeofday_returns_plausible_epoch() {
        let (sec, usec) = gettimeofday().expect("gettimeofday");
        // any real wall clock is well past 2023-11-14 (1_700_000_000);
        // a zero/garbage result would mean the kernel copyout layout is
        // wrong and the struct never got filled.
        assert!(sec > 1_700_000_000, "tv_sec = {sec} not a plausible epoch");
        assert!(
            (0..1_000_000).contains(&usec),
            "tv_usec = {usec} out of [0, 1_000_000) range"
        );
    }

    #[test]
    fn gettimeofday_monotone_nondecreasing() {
        let (s1, u1) = gettimeofday().expect("first");
        let (s2, u2) = gettimeofday().expect("second");
        // wall clock must not run backwards across two back-to-back reads
        let t1 = s1 as i128 * 1_000_000 + u1 as i128;
        let t2 = s2 as i128 * 1_000_000 + u2 as i128;
        assert!(t2 >= t1, "clock went backwards: {t1} -> {t2}");
    }

    #[test]
    fn getentropy_over_256_fails() {
        // kernel caps a single request at 256 bytes; a 257-byte ask
        // returns an error. The exact errno is kernel-specific (macOS
        // returns EINVAL=22, the man page documents EIO) so we only
        // assert it failed with a real (non-zero) errno, not the value.
        let mut too_big = [0u8; 257];
        let err = unsafe { getentropy(&mut too_big) }.expect_err("expected error for >256");
        assert_ne!(err.0, 0, "errno should be non-zero on failure");
    }
}
