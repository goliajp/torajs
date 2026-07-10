//! `memset(s, c, n) -> *mut c_void` — fill `n` bytes at `s` with `c`.
//!
//! LLVM lowers `@llvm.memset.p0.i64` (struct init, `[u8; N]`
//! zeroing, `ptr::write_bytes`, slice `fill`) to a call to the
//! libc-shape `memset` symbol. Without an in-binary definition the
//! linker resolves it against libSystem, pulling libc into the user
//! binary's import table.
//!
//! Same leaf-shim pattern as [`memcpy`](super::memcpy::memcpy):
//! pure byte-level, no syscall / signal / control-flow effect, so
//! it is safe under the force_load 0-libc link (the memory-effect
//! shape matches what LLVM expects for the intrinsic target). See
//! `docs/v0.7-A5-finding.md` § "Step 16-a-3 audit" for why memset
//! is the low-risk half of the pair (memset was never part of the
//! reverted 16-a-2 bzero+abort batch).

use core::ffi::c_void;

/// `memset(s, c, n)` — write the low byte of `c`, `n` times.
///
/// Signature matches the libc prototype exactly (`c_void`
/// pointer) — see `memcpy` for the
/// `suspicious_runtime_symbol_definitions` rationale.
///
/// # Safety
///
/// `s` must be writable for ≥ `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(s: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let p = s.cast::<u8>();
    // Byte loop. LLVM idiom-matches this into NEON stp on aarch64
    // + SSE/AVX2 on x86_64 for n ≥ 16; small n stays scalar. Only
    // the low 8 bits of `c` are used, per libc semantics.
    let byte = c as u8;
    let mut i = 0;
    while i < n {
        unsafe {
            *p.add(i) = byte;
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_n_is_noop() {
        let mut buf = [0xAAu8; 4];
        let r = unsafe { memset(buf.as_mut_ptr().cast(), 0, 0) };
        assert_eq!(r.cast::<u8>(), buf.as_mut_ptr());
        assert_eq!(&buf, &[0xAA, 0xAA, 0xAA, 0xAA]);
    }

    #[test]
    fn fill_zero() {
        let mut buf = [0xFFu8; 8];
        unsafe { memset(buf.as_mut_ptr().cast(), 0, 8) };
        assert_eq!(&buf, &[0u8; 8]);
    }

    #[test]
    fn fill_nonzero() {
        let mut buf = [0u8; 5];
        unsafe { memset(buf.as_mut_ptr().cast(), 0x41, 5) };
        assert_eq!(&buf, b"AAAAA");
    }

    #[test]
    fn only_low_byte_used() {
        // 0x1_42 truncates to 0x42 ('B') — high bits ignored.
        let mut buf = [0u8; 3];
        unsafe { memset(buf.as_mut_ptr().cast(), 0x142, 3) };
        assert_eq!(&buf, b"BBB");
    }

    #[test]
    fn partial() {
        let mut buf = [b'.'; 6];
        unsafe { memset(buf.as_mut_ptr().cast(), b'x' as i32, 3) };
        assert_eq!(&buf, b"xxx...");
    }
}
