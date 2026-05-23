//! Str equality operations + extern "C" wrappers.
//!
//! Two FFI entry points:
//!
//! - [`__torajs_str_eq`] — `===` / `!==` between two `Type::Str`
//!   values. Bytewise comparison after length check, per ES spec
//!   §7.2.16 step 3. Returns `i64` 0/1 to match the legacy C ABI
//!   that anyvalue + other runtime helpers already consume.
//! - [`__torajs_str_eq_cstr`] — compare a heap Str against a raw
//!   C-style byte slice (e.g. a literal `"undefined"` baked into
//!   ssa_inkwell IR for typeof comparisons). Same shape as above
//!   but the second operand carries its length separately rather
//!   than via the Str header.
//!
//! Both delegate to a single Rust-idiomatic [`bytes_eq`] core so
//! the comparison logic has one source of truth.

use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

/// Bytewise equality on two slices. Same as `a == b` for `&[u8]`
/// but written as an `#[inline]` free fn so both extern "C"
/// wrappers below can route through the same body (and inline at
/// the call sites at fat-LTO link time).
#[inline]
pub fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    a == b
}

/// Read the `len` u64 from a Str block (offset 8). Internal layout-
/// aware helper used by both extern "C" entry points.
///
/// # Safety
///
/// `p` must point at a valid Str block whose layout matches
/// [`crate::layout`].
#[inline]
unsafe fn str_len(p: *const u8) -> u64 {
    unsafe { (p.add(STR_LEN_OFF) as *const u64).read() }
}

/// Borrow a Str block's payload as a `&[u8]`. The lifetime is
/// caller-chosen since the underlying memory is heap-managed
/// outside Rust's lifetime model (refcount on the header).
///
/// # Safety
///
/// `p` must point at a valid Str block whose layout matches
/// [`crate::layout`], and the bytes at `p + STR_DATA_OFF .. + len`
/// must remain valid for the borrowed lifetime.
#[inline]
unsafe fn str_bytes<'a>(p: *const u8, len: u64) -> &'a [u8] {
    // SAFETY: caller contract.
    unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize) }
}

/// `===` / `!==` between two `Type::Str` values. Mirrors the
/// pre-rewrite C `__torajs_str_eq(const uint8_t *a, const uint8_t
/// *b) -> int64_t`. Returns 1 if bytes are equal, 0 otherwise.
///
/// # Safety
///
/// `a` and `b` must point at valid Str blocks (or null — null is
/// treated as a zero-length Str which is what the pre-rewrite C
/// did via the unchecked `__TORAJS_STR_LEN(NULL)` deref, and
/// callers rely on that quirk via the SSA Type::Str invariant
/// guaranteeing non-null at call sites).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64 {
    // SAFETY: caller's invariant — Type::Str pointers are
    // guaranteed non-null at the SSA-emit layer.
    let a_len = unsafe { str_len(a) };
    let b_len = unsafe { str_len(b) };
    if a_len != b_len {
        return 0;
    }
    if a_len == 0 {
        return 1;
    }
    // SAFETY: lengths are equal and non-zero; payload offsets are
    // STR_DATA_OFF past the header. Lifetimes are immediate (the
    // borrow doesn't escape this fn) so the heap blocks don't
    // need to outlive Rust's stack frame.
    let aa = unsafe { str_bytes(a, a_len) };
    let bb = unsafe { str_bytes(b, b_len) };
    if bytes_eq(aa, bb) { 1 } else { 0 }
}

/// Heap-Str equals raw-C-string check. Mirrors the pre-rewrite C
/// `__torajs_str_eq_cstr(const uint8_t *s, const uint8_t *cstr_bytes,
/// int64_t cstr_len) -> int64_t`. Returns 1 if the Str's bytes
/// equal `cstr_bytes[..cstr_len]`, 0 otherwise.
///
/// # Safety
///
/// `s` must point at a valid Str block; `cstr_bytes` must point at
/// `cstr_len` readable bytes (typically a `.rodata` literal baked
/// into ssa_inkwell IR).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_eq_cstr(
    s: *const u8,
    cstr_bytes: *const u8,
    cstr_len: i64,
) -> i64 {
    // SAFETY: caller's invariant.
    let s_len = unsafe { str_len(s) };
    if s_len as i64 != cstr_len {
        return 0;
    }
    if cstr_len == 0 {
        return 1;
    }
    // SAFETY: lengths match; both regions are `cstr_len` long.
    let ss = unsafe { str_bytes(s, s_len) };
    let cs = unsafe { core::slice::from_raw_parts(cstr_bytes, cstr_len as usize) };
    if bytes_eq(ss, cs) { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::StrBlock;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn make_str(bytes: &[u8]) -> StrBlock {
        let mut block = StrBlock::alloc(bytes.len() as u64);
        unsafe {
            block
                .as_bytes_mut(bytes.len() as u64)
                .copy_from_slice(bytes)
        };
        block
    }

    #[test]
    fn empty_strings_compare_equal() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let a = make_str(b"");
        let b = make_str(b"");
        let eq = unsafe { __torajs_str_eq(a.0.as_ptr(), b.0.as_ptr()) };
        assert_eq!(eq, 1);
        a.free_pool_aware();
        b.free_pool_aware();
    }

    #[test]
    fn equal_length_equal_bytes() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let a = make_str(b"hello");
        let b = make_str(b"hello");
        let eq = unsafe { __torajs_str_eq(a.0.as_ptr(), b.0.as_ptr()) };
        assert_eq!(eq, 1);
        a.free_pool_aware();
        b.free_pool_aware();
    }

    #[test]
    fn equal_length_diff_bytes() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let a = make_str(b"hello");
        let b = make_str(b"hellO");
        let eq = unsafe { __torajs_str_eq(a.0.as_ptr(), b.0.as_ptr()) };
        assert_eq!(eq, 0);
        a.free_pool_aware();
        b.free_pool_aware();
    }

    #[test]
    fn diff_length_short_circuits() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let a = make_str(b"hi");
        let b = make_str(b"hi there");
        let eq = unsafe { __torajs_str_eq(a.0.as_ptr(), b.0.as_ptr()) };
        assert_eq!(eq, 0);
        a.free_pool_aware();
        b.free_pool_aware();
    }

    #[test]
    fn cstr_compare_match() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let s = make_str(b"undefined");
        let lit = b"undefined";
        let eq = unsafe { __torajs_str_eq_cstr(s.0.as_ptr(), lit.as_ptr(), lit.len() as i64) };
        assert_eq!(eq, 1);
        s.free_pool_aware();
    }

    #[test]
    fn cstr_compare_diff_length() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let s = make_str(b"undefin");
        let lit = b"undefined";
        let eq = unsafe { __torajs_str_eq_cstr(s.0.as_ptr(), lit.as_ptr(), lit.len() as i64) };
        assert_eq!(eq, 0);
        s.free_pool_aware();
    }

    #[test]
    fn cstr_empty_match_empty() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let s = make_str(b"");
        let eq = unsafe { __torajs_str_eq_cstr(s.0.as_ptr(), b"".as_ptr(), 0) };
        assert_eq!(eq, 1);
        s.free_pool_aware();
    }

    #[test]
    fn cstr_diff_bytes_same_length() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let s = make_str(b"foo");
        let eq = unsafe { __torajs_str_eq_cstr(s.0.as_ptr(), b"bar".as_ptr(), 3) };
        assert_eq!(eq, 0);
        s.free_pool_aware();
    }
}
