//! `strlen(s) -> usize` — NUL-terminated C string length.
//!
//! Pulled in by Rust std's c_str / CStr / Display chain
//! (panic-handler message formatting, std::io::Error
//! display, etc.). Providing a self-bound impl drops one
//! more libSystem dyld import per tr-built user binary.

use core::ffi::c_char;

/// `strlen(s) -> usize`.
///
/// Walks bytes from `s` until the first NUL. Standard libc
/// semantics. Signature matches the libc prototype exactly
/// (`c_char`, i8 on Apple targets) — see `memcpy` for the
/// `suspicious_runtime_symbol_definitions` rationale.
///
/// # Safety
///
/// `s` must point at a NUL-terminated C string. Reading past
/// the NUL is undefined; the caller must guarantee at least
/// one `\0` byte exists at some offset from `s`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const c_char) -> usize {
    let p = s.cast::<u8>();
    let mut i = 0;
    // SAFETY: caller covenant — NUL-terminated string.
    while unsafe { *p.add(i) } != 0 {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let s = b"\0";
        let n = unsafe { strlen(s.as_ptr().cast()) };
        assert_eq!(n, 0);
    }

    #[test]
    fn basic() {
        let s = b"hello\0";
        let n = unsafe { strlen(s.as_ptr().cast()) };
        assert_eq!(n, 5);
    }

    #[test]
    fn longer() {
        let s = b"the quick brown fox\0extra-after-nul";
        let n = unsafe { strlen(s.as_ptr().cast()) };
        assert_eq!(n, 19);
    }
}
