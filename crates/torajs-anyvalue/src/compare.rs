//! Relational comparison `<` / `<=` / `>` / `>=` for Any-typed
//! operands (JS spec §7.2.13 IsLessThan + §13.10).
//!
//! Two paths:
//! - both operands are heap-Str → lexicographic byte compare with
//!   length tie-break (ES §7.2.13 step 4.b)
//! - otherwise both run through `ToNumber` + IEEE 754 compare.
//!   NaN on either side makes ALL ops return `false` per ES.
//!
//! Extracted from `lib.rs` (2026-05-25, anyvalue god-file decomp
//! batch 12).
//!
//! The `pub(crate)` `any_compare` symbol is the entry point ffi.rs
//! wraps as `__torajs_any_compare`.

use std::cmp::Ordering;

use torajs_rc::{HeapHeader, Tag};

use crate::{AnySlotTag, STR_HDR_SIZE, any_to_number};

/// Byte offset of the `u64 len` field inside the Str heap layout
/// `[header:8][len:8][bytes:N]`. Used by [`any_compare`] for the
/// String-String lexicographic byte-compare path.
pub(crate) const STR_LEN_OFF: usize = 8;

/// Op code for ordering compare per ssa_lower's emission.
/// Mirror of the C `__torajs_any_compare` switch on the `op`
/// argument: 0=Lt, 1=Le, 2=Gt, 3=Ge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompareOp {
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    /// Decode the i64 wire format ssa_lower emits.
    pub(crate) fn from_i64(op: i64) -> Option<CompareOp> {
        match op {
            0 => Some(CompareOp::Lt),
            1 => Some(CompareOp::Le),
            2 => Some(CompareOp::Gt),
            3 => Some(CompareOp::Ge),
            _ => None,
        }
    }

    /// Apply the op to a canonical `Ordering` result. NaN is
    /// handled by the caller (all four ops return `false` when
    /// either operand is NaN per ES §7.2.13).
    #[inline]
    pub(crate) fn apply(self, cmp: Ordering) -> bool {
        match self {
            CompareOp::Lt => cmp.is_lt(),
            CompareOp::Le => cmp.is_le(),
            CompareOp::Gt => cmp.is_gt(),
            CompareOp::Ge => cmp.is_ge(),
        }
    }
}

/// Returns `true` iff the `(tag, value)` pair points to a live
/// heap object tagged [`Tag::Str`]. Null and non-Heap tags return
/// `false`.
///
/// # Safety
///
/// If `tag == AnySlotTag::Heap as i64`, `value` must be null or a
/// valid `*const HeapHeader`.
#[inline]
pub(crate) unsafe fn is_heap_str(tag: i64, value: i64) -> bool {
    if tag != AnySlotTag::Heap as i64 {
        return false;
    }
    let p = value as *const HeapHeader;
    if p.is_null() {
        return false;
    }
    // SAFETY: non-null + runtime invariant says it points to a
    // live heap header.
    matches!(unsafe { (*p).tag() }, Tag::Str)
}

/// Lexicographic byte compare of two Str-tagged heap pointers,
/// matching ES §7.2.13 step 4.b's "Is Less Than" tie-break by
/// length. Both `la` / `ra` must be non-null `*const u8` pointing
/// at a Str's HeapHeader; the layout is `[header:8][len:u64@8]
/// [bytes:len@16]`.
///
/// # Safety
///
/// `la` and `ra` must be non-null and point to live Str heap
/// objects. Caller guarantees by virtue of `is_heap_str` having
/// returned true for both.
unsafe fn compare_str_lexicographic(la: i64, ra: i64) -> Ordering {
    let la = la as *const u8;
    let ra = ra as *const u8;
    // SAFETY: la/ra non-null per caller invariant; layout-aware
    // unaligned reads at byte offset 8 (u64-aligned since the
    // Str heap is 8-aligned).
    let (l_len, r_len) = unsafe {
        (
            (la.add(STR_LEN_OFF) as *const u64).read() as usize,
            (ra.add(STR_LEN_OFF) as *const u64).read() as usize,
        )
    };
    let min_len = l_len.min(r_len);
    // SAFETY: byte payload starts at offset STR_HDR_SIZE; each is
    // at least min_len bytes long (we took the min).
    let (lb, rb) = unsafe {
        (
            std::slice::from_raw_parts(la.add(STR_HDR_SIZE), min_len),
            std::slice::from_raw_parts(ra.add(STR_HDR_SIZE), min_len),
        )
    };
    match lb.cmp(rb) {
        Ordering::Equal => l_len.cmp(&r_len),
        other => other,
    }
}

/// `<`, `<=`, `>`, `>=` on two Any-tagged `(tag, value)` pairs per
/// ES §7.2.13 IsLessThan + §13.10. Both sides go through
/// `ToPrimitive(hint=Number)`; if BOTH result in String the path
/// is a lexicographic byte-compare, otherwise both run through
/// ToNumber and IEEE 754 compare. NaN makes ALL ops return
/// `false`.
///
/// Returns `false` defensively for any unknown `op` value.
///
/// # Safety
///
/// If either tag is `Heap`, the corresponding value must be null
/// or a valid `*mut HeapHeader`. ToNumber's Heap+Str path
/// delegates to the still-C `__torajs_str_to_number`, which
/// requires the pointer be Tag::Str-headed.
pub(crate) unsafe fn any_compare(op: i64, lt: i64, lv: i64, rt: i64, rv: i64) -> bool {
    let op = match CompareOp::from_i64(op) {
        Some(o) => o,
        None => return false,
    };
    // SAFETY: caller invariant — propagated.
    let l_is_str = unsafe { is_heap_str(lt, lv) };
    let r_is_str = unsafe { is_heap_str(rt, rv) };
    let cmp = if l_is_str && r_is_str {
        // SAFETY: is_heap_str checked both pointers non-null + Str-
        // headed; compare_str_lexicographic's invariants hold.
        unsafe { compare_str_lexicographic(lv, rv) }
    } else {
        // SAFETY: caller invariant — propagated to any_to_number.
        let l = unsafe { any_to_number(lt, lv) };
        let r = unsafe { any_to_number(rt, rv) };
        if l.is_nan() || r.is_nan() {
            return false;
        }
        // partial_cmp is total for non-NaN f64 (we just excluded
        // NaN above); use unsafe-unchecked to avoid the Result path
        // pulling Rust's panic machinery into the user binary
        // (polish A3).
        unsafe { l.partial_cmp(&r).unwrap_unchecked() }
    };
    op.apply(cmp)
}
