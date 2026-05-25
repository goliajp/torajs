//! Arithmetic (`-` / `*` / `/` / `%`) and `+` dispatch on
//! Any-tagged operands (JS spec §13.6–§13.9 + §13.15.3
//! ApplyStringOrNumericBinaryOperator).
//!
//! Two entry points, both `pub(crate)` so the FFI shims in `ffi.rs`
//! can wrap them as `__torajs_any_arith` + `__torajs_any_add`:
//!
//! - [`any_arith`] — the four arithmetic ops. Both operands go
//!   through `ToNumber`, then IEEE 754 math. Integer fast-path when
//!   both inputs are i64-shaped (Null/Bool/I64) AND the op is not
//!   `Div` AND the f64 result round-trips through i64 losslessly.
//! - [`any_add`] — `+`. If either operand is `Heap` + `Tag::Str`,
//!   take the String-concat path (both sides `ToString`, then
//!   `__torajs_str_concat`). Otherwise same numeric path as
//!   `any_arith` but always sums.
//!
//! Extracted from `lib.rs` (2026-05-25, anyvalue god-file decomp
//! batch 13).

use std::ffi::c_void;

use torajs_rc::AnySlotTag;

use crate::compare::is_heap_str;
use crate::{__torajs_str_concat, __torajs_str_drop, AnyBox, any_to_number, any_to_str};

/// Op code for `-`, `*`, `/`, `%` per ssa_lower's emission. Mirror
/// of the C `__torajs_any_arith` switch on the `op` argument:
/// 0=Sub, 1=Mul, 2=Div, 3=Mod.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArithOp {
    Sub,
    Mul,
    Div,
    Mod,
}

impl ArithOp {
    /// Decode the i64 wire format ssa_lower emits.
    pub(crate) fn from_i64(op: i64) -> Option<ArithOp> {
        match op {
            0 => Some(ArithOp::Sub),
            1 => Some(ArithOp::Mul),
            2 => Some(ArithOp::Div),
            3 => Some(ArithOp::Mod),
            _ => None,
        }
    }

    /// Apply the op to two already-ToNumber-d operands. ES §13.9
    /// `%` matches C's `fmod` (sign of dividend; NaN on `y == 0`);
    /// Rust's `f64 % f64` lowers to `fmod` on every host we target,
    /// so the Mod arm is a one-liner with no special-casing.
    #[inline]
    pub(crate) fn apply(self, l: f64, r: f64) -> f64 {
        match self {
            ArithOp::Sub => l - r,
            ArithOp::Mul => l * r,
            ArithOp::Div => l / r,
            ArithOp::Mod => l % r,
        }
    }

    /// Whether the integer fast-path applies to this op. `Div`
    /// always yields f64 even for integer operands (`1/2 === 0.5`,
    /// not `0`), so it's excluded; the rest qualify.
    #[inline]
    pub(crate) fn allows_i64_fast_path(self) -> bool {
        !matches!(self, ArithOp::Div)
    }
}

/// Whether an Any tag's ToNumber result is "i64-shaped" — i.e.
/// always an exact integer in i64 range. Null=0, Bool=0/1,
/// I64=value all qualify; F64 doesn't (may be fractional), Undef
/// doesn't (ToNumber → NaN), Heap+anything doesn't (Str parse can
/// produce f64, object → NaN). Used by `any_arith` to decide the
/// I64-vs-F64 boxing of integer-valued results.
#[inline]
pub(crate) fn tag_is_i64_shaped(tag: i64) -> bool {
    tag == AnySlotTag::Null as i64
        || tag == AnySlotTag::Bool as i64
        || tag == AnySlotTag::I64 as i64
}

/// `-`, `*`, `/`, `%` on two Any-tagged `(tag, value)` pairs per
/// ES §13.6–§13.9. Both operands `ToNumber`-ed then the arithmetic
/// happens in IEEE 754. Result is boxed as either I64 (integer
/// fast-path; see [`ArithOp::allows_i64_fast_path`] +
/// [`tag_is_i64_shaped`]) or F64.
///
/// Out-of-range `op` → NaN-boxed (defensive; IR should never emit
/// this).
///
/// # Safety
///
/// If either `tag` is Heap, the corresponding `value` must be
/// null or a valid `*mut HeapHeader` — propagated through
/// [`any_to_number`].
pub(crate) unsafe fn any_arith(op: i64, lt: i64, lv: i64, rt: i64, rv: i64) -> *mut c_void {
    let arith_op = match ArithOp::from_i64(op) {
        Some(o) => o,
        // Defensive — match the C `default: NaN` branch.
        None => return alloc_number_f64(f64::NAN),
    };
    // SAFETY: caller invariant — propagated.
    let l = unsafe { any_to_number(lt, lv) };
    let r = unsafe { any_to_number(rt, rv) };
    let result = arith_op.apply(l, r);

    if arith_op.allows_i64_fast_path()
        && tag_is_i64_shaped(lt)
        && tag_is_i64_shaped(rt)
        && result >= i64::MIN as f64
        && result <= i64::MAX as f64
    {
        let int_result = result as i64;
        // Round-trip check: only box as I64 if the cast is lossless.
        if (int_result as f64) == result {
            return alloc_number_i64(int_result);
        }
    }
    alloc_number_f64(result)
}

/// Box an f64 into a fresh AnyBox tagged F64. Matches the C ABI's
/// `__torajs_any_box(ANY_F64, bitcast(f64).i64)` pattern. Shared
/// helper because any_arith has two callsites for it (defensive
/// NaN return + main F64 path) and any_add reuses it.
#[inline]
fn alloc_number_f64(value: f64) -> *mut c_void {
    AnyBox::alloc(AnySlotTag::F64, value.to_bits() as i64).as_ptr() as *mut c_void
}

/// Box an i64 into a fresh AnyBox tagged I64. Shared by every
/// integer-fast-path callsite (any_arith + any_add).
#[inline]
fn alloc_number_i64(value: i64) -> *mut c_void {
    AnyBox::alloc(AnySlotTag::I64, value).as_ptr() as *mut c_void
}

/// `+` on two Any-tagged `(tag, value)` pairs per ES §13.15.3.
/// If either operand is `Heap` + [`torajs_rc::Tag::Str`] the result
/// is the String concatenation of both operands' `ToString`s.
/// Otherwise both operands go through ToNumber and the f64 sum is
/// boxed — I64 when both inputs are i64-shaped (Null/Bool/I64) AND
/// the sum round-trips through i64 losslessly, else F64.
///
/// Returns a fresh owned AnyBox (refcount = 1); caller drops.
///
/// # Safety
///
/// If either tag is `Heap`, the corresponding value must be null
/// or a valid `*mut HeapHeader` — propagated through both the
/// Str-path (where C-side `__torajs_str_concat` reads the Str
/// layout) and the numeric path (via [`any_to_number`]).
pub(crate) unsafe fn any_add(lt: i64, lv: i64, rt: i64, rv: i64) -> *mut c_void {
    // SAFETY: caller invariant — propagated.
    let l_is_str = unsafe { is_heap_str(lt, lv) };
    let r_is_str = unsafe { is_heap_str(rt, rv) };

    // String concatenation path (ES §13.15.3 — either side String
    // wins). Both operands go through ToString; the two
    // intermediates are dropped after the concat owns its own
    // copy of the bytes.
    if l_is_str || r_is_str {
        // SAFETY: any_to_str preserves the Heap-payload Safety
        // contract; result is a freshly-owned Str (refcount=1).
        let l_str = unsafe { any_to_str(lt, lv) };
        let r_str = unsafe { any_to_str(rt, rv) };
        // SAFETY: both pointers are freshly-owned Strs whose layout
        // begins with HeapHeader. __torajs_str_concat reads the
        // layout, allocates a new Str, returns ownership to us.
        let concat = unsafe { __torajs_str_concat(l_str as *const u8, r_str as *const u8) };
        // SAFETY: both Strs were rc=1 from any_to_str; rc_dec to 0
        // frees them.
        unsafe {
            __torajs_str_drop(l_str);
            __torajs_str_drop(r_str);
        }
        return AnyBox::alloc(AnySlotTag::Heap, concat as i64).as_ptr() as *mut c_void;
    }

    // Numeric path. ToNumber reuses the per-tag dispatch from
    // P2.3-d.1; same predicates as any_arith for the I64 fast-
    // path (i64-shaped tags + lossless f64↔i64 round-trip).
    //
    // SAFETY: caller invariant — propagated.
    let l = unsafe { any_to_number(lt, lv) };
    let r = unsafe { any_to_number(rt, rv) };
    let sum = l + r;

    if tag_is_i64_shaped(lt)
        && tag_is_i64_shaped(rt)
        && sum >= i64::MIN as f64
        && sum <= i64::MAX as f64
    {
        let int_sum = sum as i64;
        if (int_sum as f64) == sum {
            return alloc_number_i64(int_sum);
        }
    }
    alloc_number_f64(sum)
}
