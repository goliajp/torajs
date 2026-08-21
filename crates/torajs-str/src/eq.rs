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
//!   the binary's rodata for typeof comparisons). Same shape as above
//!   but the second operand carries its length separately rather
//!   than via the Str header.
//!
//! Both delegate to a single Rust-idiomatic [`bytes_eq`] core so
//! the comparison logic has one source of truth.
//!
//! ## P11.1-S2.3 — canonical-encoding short-circuit
//!
//! Every Str allocation routes through either
//! [`StringLiteral::encode_from_str`] at build time or
//! [`__torajs_str_alloc`] at run time, both of which pick the
//! narrowest encoding that can represent the codepoints (Latin-1
//! when max codepoint ≤ 0xFF, else UTF-16 LE). That makes the
//! encoding a canonical form: two Strs whose content is value-
//! equal MUST share the same encoding flag — different
//! encodings ⇒ different content. `__torajs_str_eq` exploits
//! this to short-circuit on the encoding flag before walking
//! payloads. `__torajs_str_eq_cstr` already short-circuits via
//! the length-mismatch arm (cstr_len counts bytes, the Str's
//! `length` counts code units — a UTF-16 Str compared against
//! its UTF-8 cstr literal will mismatch on the length check
//! since UTF-16 payload = `length × 2` bytes vs cstr's UTF-8
//! byte count). All current `str_eq_cstr` call sites pass ASCII
//! identifier strings (typeof results, dynobj field names), so
//! the Str side is Latin-1 with `length == cstr_len` and bytes
//! are bit-identical.

use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use crate::substr::{FLAG_SUBSTR_INLINE, FLAG_SUBSTR_VIEW};
use crate::substr_methods::substr_view;
use crate::undef_sentinel::is_undef;
use torajs_rc::HeapHeader;

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
unsafe fn str_len(p: *const u8) -> u32 {
    unsafe { (p.add(STR_LEN_OFF) as *const u32).read() }
}

/// Read the `IS_LATIN1` bit out of a Str header's flags field
/// (universal heap header layout: `refcount u32 @0`, `type_tag
/// u16 @4`, `flags u16 @6`). Powers the encoding short-circuit
/// in [`__torajs_str_eq`] — under the canonical-encoding
/// invariant a mismatch on this bit means the two operands
/// cannot have equal content.
///
/// # Safety
///
/// `p` must point at a valid Str block.
#[inline]
unsafe fn str_is_latin1(p: *const u8) -> bool {
    let header = unsafe { &*(p as *const HeapHeader) };
    (header.flags & STR_FLAG_IS_LATIN1) != 0
}

/// Byte count occupied by a Str's payload, given its `length`
/// (code unit count) and `is_latin1` flag. Latin-1: 1 byte per
/// code unit; UTF-16 LE: 2 bytes per code unit. Used by
/// [`__torajs_str_eq`] to derive the memcmp range when the
/// encoding short-circuit didn't fire.
#[inline]
fn payload_byte_count(length: u32, is_latin1: bool) -> usize {
    if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    }
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
unsafe fn str_bytes<'a>(p: *const u8, len: u32) -> &'a [u8] {
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
    // RFC 20260707 chunk 2 — nullish operands compare by IDENTITY,
    // never content: the undefined sentinel cell (uncaptured regex
    // groups, `[.., undefined]` array-literal slots) carries the
    // TEXT "undefined" but `undefined === "undefined"` is false;
    // NULL (JS null, or a not-yet-flipped undefined producer)
    // would SIGSEGV in the content walk below (test262
    // S15.5.4.10_A2_T10 family).
    if a.is_null() || b.is_null() || is_undef(a) || is_undef(b) {
        return (a == b) as i64;
    }
    //
    // Chunk 562 — either operand can be a Substr VIEW/INLINE cell:
    // dispatch sites holding only the Tag::Str type_tag (the
    // any-world strict-eq, dynobj key compares) hand views straight
    // here, and the owned-Str layout read turned the view's
    // parent-ptr/offset fields into "payload" bytes (`v === "c"`
    // silently false). Resolve each side view-aware first; the
    // owned/owned fast path pays one flags test per operand.
    let (aa, a_latin1) = unsafe { resolve_payload(a) };
    let (bb, b_latin1) = unsafe { resolve_payload(b) };
    if a_latin1 == b_latin1 {
        // Same encoding ⇒ same stride: plain byte compare. Owned
        // Strs uphold the canonical-encoding invariant
        // (`StringLiteral::encode_from_str` at build time,
        // narrowing kernels at runtime), so two owned cells with
        // equal content always land here.
        if aa.len() != bb.len() {
            return 0;
        }
        return if bytes_eq(aa, bb) { 1 } else { 0 };
    }
    // Mixed encoding — a Substr VIEW inherits its parent's
    // encoding and cannot narrow, so a UTF-16 view whose units all
    // fit Latin-1 (`"\u{6C49}abc".slice(1)`) legitimately reaches
    // here with content equal to a canonical Latin-1 operand.
    // Compare per code unit (RFC 20260712-string-proto-cluster
    // chunk A2; the pre-A2 encoding short-circuit answered 0).
    let (l1, wide) = if a_latin1 { (aa, bb) } else { (bb, aa) };
    if l1.len() * 2 != wide.len() {
        return 0;
    }
    for (i, &c) in l1.iter().enumerate() {
        if u16::from_le_bytes([wide[2 * i], wide[2 * i + 1]]) != c as u16 {
            return 0;
        }
    }
    1
}

/// Resolve a Tag::Str-tagged cell to its `(payload bytes, latin1)`
/// pair, view-aware: VIEW/INLINE-flagged Substr cells read through
/// the parent (offset × encoding stride); owned Strs read their own
/// payload. Byte lengths already carry the encoding stride, so a
/// Latin-1 and a UTF-16 string of the same code-unit count differ
/// in byte length (the empty case stays encoding-agnostic).
///
/// # Safety
///
/// `p` must point at a valid owned-Str or Substr heap block.
#[inline]
pub(crate) unsafe fn resolve_payload<'a>(p: *const u8) -> (&'a [u8], bool) {
    let header = unsafe { &*(p as *const HeapHeader) };
    if header.flags & (FLAG_SUBSTR_VIEW | FLAG_SUBSTR_INLINE) != 0 {
        let (bytes, _, latin1) = unsafe { substr_view(p) };
        (bytes, latin1)
    } else {
        let len = unsafe { str_len(p) };
        let latin1 = unsafe { str_is_latin1(p) };
        let byte_cnt = payload_byte_count(len, latin1);
        // SAFETY: caller contract; payload starts at STR_DATA_OFF.
        let bytes = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), byte_cnt) };
        (bytes, latin1)
    }
}

/// Heap-Str equals raw-C-string check. Mirrors the pre-rewrite C
/// `__torajs_str_eq_cstr(const uint8_t *s, const uint8_t *cstr_bytes,
/// int64_t cstr_len) -> int64_t`. Returns 1 if the Str's bytes
/// equal `cstr_bytes[..cstr_len]`, 0 otherwise.
///
/// # Safety
///
/// `s` must point at a valid Str block; `cstr_bytes` must point at
/// `cstr_len` readable bytes (typically a toolchain-baked
/// `.rodata` literal).
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
    use crate::block::StrBlock;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn make_str(bytes: &[u8]) -> StrBlock {
        let mut block = StrBlock::alloc(bytes.len() as u32);
        unsafe {
            block
                .as_bytes_mut(bytes.len() as u32)
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
    fn cross_encoding_compares_by_code_unit() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        // A UTF-16 cell whose units all fit Latin-1 vs the
        // canonical Latin-1 cell with the same content. Owned Strs
        // uphold the canonical-encoding invariant, but Substr
        // VIEWs inherit their parent's encoding and reach the
        // mixed branch with genuinely equal content (RFC
        // 20260712-string-proto-cluster chunk A2) — the eq kernel
        // itself must compare per code unit, not short-circuit.
        let a = make_str(b"abc"); // Latin-1 via StrBlock::alloc
        let b = StrBlock::alloc_with_encoding(3, false);
        unsafe {
            b.0.as_ptr()
                .add(crate::layout::STR_DATA_OFF)
                .copy_from_nonoverlapping([0x61u8, 0x00, 0x62, 0x00, 0x63, 0x00].as_ptr(), 6)
        };
        let eq = unsafe { __torajs_str_eq(a.0.as_ptr(), b.0.as_ptr()) };
        assert_eq!(eq, 1);
        // Different content must still answer false in both orders.
        let c = make_str(b"abd");
        assert_eq!(unsafe { __torajs_str_eq(c.0.as_ptr(), b.0.as_ptr()) }, 0);
        assert_eq!(unsafe { __torajs_str_eq(b.0.as_ptr(), c.0.as_ptr()) }, 0);
        // A supra-Latin-1 unit on the wide side is never equal.
        let d = StrBlock::alloc_with_encoding(3, false);
        unsafe {
            d.0.as_ptr()
                .add(crate::layout::STR_DATA_OFF)
                .copy_from_nonoverlapping([0x61u8, 0x00, 0x62, 0x00, 0x49, 0x6c].as_ptr(), 6)
        };
        assert_eq!(unsafe { __torajs_str_eq(a.0.as_ptr(), d.0.as_ptr()) }, 0);
        a.free_pool_aware();
        b.free_pool_aware();
        c.free_pool_aware();
        d.free_pool_aware();
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
