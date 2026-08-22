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
//! The `pub(crate)` `any_compare` symbol is the entry point
//! [`nanbox_encode`](crate::nanbox_encode) wraps as
//! `__torajs_anyv_compare_pair` (NaN-box pair entry point).

use std::cmp::Ordering;

use torajs_rc::{HeapHeader, Tag};

use crate::coerce::any_to_number;
use crate::{AnySlotTag, STR_HDR_SIZE};

/// Byte offset of the `u64 len` field inside the Str heap layout
/// `[header:8][len:8][bytes:N]`. Used by [`any_compare`] for the
/// String-String lexicographic byte-compare path.
pub(crate) const STR_LEN_OFF: usize = 8;

/// Op code for ordering compare per ssa_lower's emission.
/// Mirror of the C-side compare switch on the `op` argument:
/// 0=Lt, 1=Le, 2=Gt, 3=Ge.
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

/// Returns `true` iff the `(tag, value)` pair points at a String-
/// shaped receiver: a live [`Tag::Str`] cell OR a [`Tag::StringWrapper`]
/// whose `[[StringData]]` inner cell is non-null. Null and non-Heap
/// tags return `false`; a wrapper with a NULL sentinel inner cell
/// also returns `false` so callers fall through to the numeric path
/// (which delegates the empty-string handling through `any_to_str`).
///
/// RFC 20260716 刀 5 — wrapper operands must trigger the ES §13.15.3
/// string-concat / §7.2.13 string-compare branch; before this the
/// wrapper-plus-primitive `+` fell into the numeric arm and gave
/// `1 + 1 === 2` where spec (ToPrimitive → String → concat) wants
/// `"11"`. This predicate governs both `any_add` (arith.rs) and
/// `any_compare` (below), so a single flip fixes both.
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
    let t = unsafe { (*p).tag() };
    if matches!(t, Tag::Str) {
        return true;
    }
    if matches!(t, Tag::StringWrapper) {
        // Inner cell at [[StringData]] offset 8. NULL sentinel
        // (`new String(NULL)` corner) → callers fall through.
        let inner = unsafe { ((p as *const u8).add(8) as *const *const HeapHeader).read() };
        return !inner.is_null();
    }
    false
}

/// Effective Str cell pointer for a str-shaped `(tag, value)` pair —
/// the same predicate as [`is_heap_str`] but returning the layout-
/// bearing pointer that [`compare_str_lexicographic`] / concat
/// kernels read against. A [`Tag::StringWrapper`] returns its
/// `[[StringData]]` inner cell, so wrapper receivers land on the
/// same str-layout code without duplicating it.
///
/// # Safety
/// Same as [`is_heap_str`].
#[inline]
pub(crate) unsafe fn str_effective_ptr(tag: i64, value: i64) -> Option<i64> {
    if tag != AnySlotTag::Heap as i64 {
        return None;
    }
    let p = value as *const HeapHeader;
    if p.is_null() {
        return None;
    }
    let t = unsafe { (*p).tag() };
    if matches!(t, Tag::Str) {
        return Some(value);
    }
    if matches!(t, Tag::StringWrapper) {
        let inner = unsafe { ((p as *const u8).add(8) as *const *const HeapHeader).read() };
        if inner.is_null() {
            return None;
        }
        return Some(inner as i64);
    }
    None
}

/// `flags u16 @6` bit 1 of a Str header — the payload is Latin-1
/// (one byte per code unit); clear means UTF-16 little-endian (two
/// bytes per unit). Mirror of torajs-str `layout::STR_FLAG_IS_LATIN1`.
pub(crate) const STR_FLAG_IS_LATIN1: u16 = 0x0002;

/// Ordinal compare of two OWNED Str heap pointers by UTF-16 code
/// unit — ES §7.2.13 step 3.d, with the length tie-break falling out
/// of the unit walk. Layout: `[header:8][len:u32@8][pad:u32@12]
/// [payload@16]`, payload width by the header's Latin-1 flag.
/// Comparing raw bytes was right only for two Latin-1 strings; two
/// UTF-16 strings compared their low bytes first (`"世" < "a"`
/// answered true in the any lane, rotation 468). Mirrors torajs-str
/// `lookup::code_unit_compare`, which this crate cannot import.
///
/// # Safety
///
/// `la` and `ra` must be non-null and point to live owned Str heap
/// objects. Caller guarantees by virtue of `is_heap_str` having
/// returned true for both.
unsafe fn compare_str_lexicographic(la: i64, ra: i64) -> Ordering {
    let la = la as *const u8;
    let ra = ra as *const u8;
    // SAFETY: la/ra non-null per caller invariant; header at 0, the
    // u32 length at STR_LEN_OFF, payload from STR_HDR_SIZE on.
    let (lb, l_latin1, rb, r_latin1) = unsafe {
        let side = |p: *const u8| {
            let latin1 = (*(p as *const HeapHeader)).flags & STR_FLAG_IS_LATIN1 != 0;
            let len = (p.add(STR_LEN_OFF) as *const u32).read() as usize;
            let bytes = std::slice::from_raw_parts(p.add(STR_HDR_SIZE), len << (!latin1 as usize));
            (bytes, latin1)
        };
        let (lb, l1) = side(la);
        let (rb, r1) = side(ra);
        (lb, l1, rb, r1)
    };
    fn wide(p: &[u8]) -> impl Iterator<Item = u16> + '_ {
        p.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))
    }
    fn narrow(p: &[u8]) -> impl Iterator<Item = u16> + '_ {
        p.iter().map(|&c| c as u16)
    }
    match (l_latin1, r_latin1) {
        (true, true) => lb.cmp(rb),
        (false, false) => wide(lb).cmp(wide(rb)),
        (true, false) => narrow(lb).cmp(wide(rb)),
        (false, true) => wide(lb).cmp(narrow(rb)),
    }
}

/// §7.2.13 BigInt-side comparison. Outer `None` = neither side is
/// a BigInt (caller proceeds to the ToNumber lane); `Some(None)` =
/// a NaN sat beside the BigInt (all compare ops answer false);
/// `Some(Some(ord))` = the mathematical-value ordering.
unsafe fn bigint_side_cmp(lt: i64, lv: i64, rt: i64, rv: i64) -> Option<Option<Ordering>> {
    use crate::arith_bigint::heap_bigint_ptr;
    use crate::loose_eq::bigint_ffi;
    // SAFETY: caller invariant — propagated.
    let l_big = unsafe { heap_bigint_ptr(lt, lv) };
    let r_big = unsafe { heap_bigint_ptr(rt, rv) };
    match (l_big, r_big) {
        (Some(a), Some(b)) => {
            // SAFETY: both are live BigInt cells.
            Some(Some(
                unsafe { bigint_ffi::__torajs_bigint_cmp(a, b) }.cmp(&0),
            ))
        }
        (Some(a), None) => {
            // SAFETY: rt/rv is non-BigInt — ToNumber is legal.
            let n = unsafe { any_to_number(rt, rv) };
            Some(unsafe { bigint_num_cmp(a, n) })
        }
        (None, Some(b)) => {
            // SAFETY: lt/lv is non-BigInt — ToNumber is legal.
            let n = unsafe { any_to_number(lt, lv) };
            Some(unsafe { bigint_num_cmp(b, n) }.map(Ordering::reverse))
        }
        (None, None) => None,
    }
}

/// Mathematical `bigint ? number` ordering (§7.2.13 step 4 /
/// §6.1.6.2's BigInt::lessThan against ℝ(number)). `None` = the
/// number is NaN. The f64 is NOT rounded through the BigInt's
/// magnitude: compare against `floor(n)` exactly (the loose-eq
/// `bigint_num_eq` mint-tmp-cmp-drop shape), then a fractional
/// remainder breaks the tie toward the number.
unsafe fn bigint_num_cmp(b: *const core::ffi::c_void, n: f64) -> Option<Ordering> {
    use crate::loose_eq::bigint_ffi;
    if n.is_nan() {
        return None;
    }
    if n == f64::INFINITY {
        return Some(Ordering::Less);
    }
    if n == f64::NEG_INFINITY {
        return Some(Ordering::Greater);
    }
    let fl = n.floor();
    // SAFETY: fl is a finite integral f64 — from_number stays off
    // its pending-RangeError path.
    let tmp = unsafe { bigint_ffi::__torajs_bigint_from_number(fl) };
    if tmp.is_null() {
        return None;
    }
    // SAFETY: b is a live BigInt cell; tmp is a fresh BigInt block.
    let c = unsafe { bigint_ffi::__torajs_bigint_cmp(b, tmp as *const core::ffi::c_void) };
    unsafe { bigint_ffi::__torajs_bigint_drop(tmp as *mut core::ffi::c_void) };
    Some(match c.cmp(&0) {
        // b == floor(n) with a fractional remainder → b < n.
        Ordering::Equal if n > fl => Ordering::Less,
        other => other,
    })
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
        // SAFETY: is_heap_str just cleared both sides;
        // str_effective_ptr answers the layout-bearing inner cell
        // (wrapper receivers unwrap to `[[StringData]]`), which
        // compare_str_lexicographic reads at the Str header offsets.
        let l_ptr = unsafe { str_effective_ptr(lt, lv).unwrap_unchecked() };
        let r_ptr = unsafe { str_effective_ptr(rt, rv).unwrap_unchecked() };
        unsafe { compare_str_lexicographic(l_ptr, r_ptr) }
    } else if let Some(cmp) =
        // §7.2.13 steps 3-4 — a BigInt operand compares by
        // MATHEMATICAL value against BigInt or Number (mixed
        // comparison is legal, unlike arithmetic). Intercepted
        // before ToNumber, whose BigInt arm is the mixed-arith
        // throw. The BigInt-vs-String leg approximates
        // StringToBigInt with ToNumber(string) — exact for the
        // integer strings the corpus uses; the non-integer /
        // radix-prefixed divergence is a registered residue.
        // SAFETY: caller invariant — propagated.
        unsafe { bigint_side_cmp(lt, lv, rt, rv) }
    {
        match cmp {
            Some(c) => c,
            // NaN beside a BigInt — all four ops answer false.
            None => return false,
        }
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

/// ES §23.1.3.30.2 SortCompare steps 5-8 pre-probe for the USER-
/// comparator lane over `Arr<Any>` elements — the NaN-box twin of
/// torajs-str's `__torajs_str_sort_undef_pre` (RFC 20260721 刀 7
/// G8a): an undefined element never reaches the comparator, it
/// sorts last unconditionally. Answers the SortCompare result
/// (`1` / `-1` / `0`) when either side is undefined, or `2` (no
/// undefined — proceed to the comparator call). `null` is an
/// ordinary comparator argument.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_any_sort_undef_pre(
    a: crate::nanbox::AnyValue,
    b: crate::nanbox::AnyValue,
) -> i64 {
    let a_undef = crate::nanbox::is_undefined(a);
    let b_undef = crate::nanbox::is_undefined(b);
    if a_undef || b_undef {
        (a_undef as i64) - (b_undef as i64)
    } else {
        2
    }
}
