//! Arithmetic (`-` / `*` / `/` / `%`) and `+` dispatch on
//! Any-tagged operands (JS spec §13.6–§13.9 + §13.15.3
//! ApplyStringOrNumericBinaryOperator).
//!
//! Two entry points, both `pub(crate)` so the NaN-box pair shims
//! in [`nanbox_encode`](crate::nanbox_encode) can wrap them as
//! `__torajs_anyv_arith_pair` + `__torajs_anyv_add_pair`:
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

use torajs_rc::{AnySlotTag, HeapHeader};

use crate::STR_HDR_SIZE;
use crate::coerce::{any_to_number, any_to_str};
use crate::compare::{STR_FLAG_IS_LATIN1, STR_LEN_OFF, is_heap_str};
use crate::nanbox::{
    AnyValue, SHORT_STR_CAP, box_double, box_int32, box_void_ptr, try_box_short_str,
};
use crate::{__torajs_str_concat, __torajs_str_drop};

/// Op code for `-`, `*`, `/`, `%`, `**` per ssa_lower's emission.
/// Mirror of the C-side arith switch on the `op` argument:
/// 0=Sub, 1=Mul, 2=Div, 3=Mod, 4=Pow (S2.43).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArithOp {
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

impl ArithOp {
    /// Decode the i64 wire format ssa_lower emits.
    pub(crate) fn from_i64(op: i64) -> Option<ArithOp> {
        match op {
            0 => Some(ArithOp::Sub),
            1 => Some(ArithOp::Mul),
            2 => Some(ArithOp::Div),
            3 => Some(ArithOp::Mod),
            4 => Some(ArithOp::Pow),
            _ => None,
        }
    }

    /// Apply the op to two already-ToNumber-d operands. ES §13.9
    /// `%` matches C's `fmod` (sign of dividend; NaN on `y == 0`);
    /// Rust's `f64 % f64` lowers to `fmod` on every host we target,
    /// so the Mod arm is a one-liner with no special-casing. `**`
    /// delegates to the self-ported `__torajs_math_pow` — its
    /// special-case lattice IS ES §6.1.6.1.3 Number::exponentiate
    /// (NaN exponent → NaN even for base 1, unlike C pow).
    #[inline]
    pub(crate) fn apply(self, l: f64, r: f64) -> f64 {
        unsafe extern "C" {
            fn __torajs_math_pow(x: f64, y: f64) -> f64;
        }
        match self {
            ArithOp::Sub => l - r,
            ArithOp::Mul => l * r,
            ArithOp::Div => l / r,
            ArithOp::Mod => l % r,
            // SAFETY: pure f64→f64 math, no pointers or state.
            ArithOp::Pow => unsafe { __torajs_math_pow(l, r) },
        }
    }

    /// Whether the integer fast-path applies to this op. `Div`
    /// always yields f64 even for integer operands (`1/2 === 0.5`,
    /// not `0`), so it's excluded; the rest qualify (`Pow`'s
    /// fractional results — negative exponents — are caught by the
    /// caller's lossless round-trip check).
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
pub(crate) unsafe fn any_arith(op: i64, lt: i64, lv: i64, rt: i64, rv: i64) -> AnyValue {
    let arith_op = match ArithOp::from_i64(op) {
        Some(o) => o,
        // Defensive — match the C `default: NaN` branch.
        None => return box_double(f64::NAN),
    };
    // §13.6-§13.9 ToNumeric dispatch — BigInt pair rides the bigint
    // kernel, a mixed pair throws, before either operand reaches
    // ToNumber (whose BigInt arm is the throw).
    let kernel = match arith_op {
        ArithOp::Sub => crate::loose_eq::bigint_ffi::__torajs_bigint_sub,
        ArithOp::Mul => crate::loose_eq::bigint_ffi::__torajs_bigint_mul,
        ArithOp::Div => crate::loose_eq::bigint_ffi::__torajs_bigint_div,
        ArithOp::Mod => crate::loose_eq::bigint_ffi::__torajs_bigint_mod,
        ArithOp::Pow => crate::loose_eq::bigint_ffi::__torajs_bigint_pow,
    };
    // SAFETY: caller invariant — propagated.
    if let Some(v) = unsafe { crate::arith_bigint::try_bigint_pair(lt, lv, rt, rv, kernel) } {
        return v;
    }
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
            return box_i64(int_result);
        }
    }
    box_double(result)
}

/// NaN-box an i64 — values that fit in i32 become Int32 immediates;
/// values outside that range promote to f64 (lossless within
/// ±2^53; precision loss beyond matches JS semantics). Mirror of
/// `__torajs_anyv_box_i64` kept inline so the arithmetic dispatch
/// stays self-contained.
#[inline]
fn box_i64(value: i64) -> AnyValue {
    if let Ok(n32) = i32::try_from(value) {
        box_int32(n32)
    } else {
        box_double(value as f64)
    }
}

/// `+` on two Any-tagged `(tag, value)` pairs per ES §13.15.3.
/// If either operand is `Heap` + [`torajs_rc::Tag::Str`] the result
/// is the String concatenation of both operands' `ToString`s.
/// Otherwise both operands go through ToNumber and the f64 sum is
/// boxed — I64 when both inputs are i64-shaped (Null/Bool/I64) AND
/// the sum round-trips through i64 losslessly, else F64.
///
/// Returns a fresh owned [`AnyValue`] immediate (caller owns the
/// rc for cell payloads).
///
/// # Safety
///
/// If either tag is `Heap`, the corresponding value must be null
/// or a valid `*mut HeapHeader` — propagated through both the
/// Str-path (where C-side `__torajs_str_concat` reads the Str
/// layout) and the numeric path (via [`any_to_number`]).
pub(crate) unsafe fn any_add(lt: i64, lv: i64, rt: i64, rv: i64) -> AnyValue {
    // §13.15.3 steps 1-2 — BOTH operands run ToPrimitive (default
    // hint) BEFORE the string/number split: an object operand's
    // valueOf decides the path (`"" + {valueOf: () => 1}` concats
    // "1", two valueOf-objects add numerically). Pre-fix the split
    // keyed off the RAW operands, so an object next to a string ran
    // hint-string ToString (toString ahead of valueOf). The
    // primitive replaces the pair; its owned box releases at the
    // end (primitives are immediates except Str, whose stake the
    // concat path's ToString covers with its own inc).
    let (lt, lv, l_prim) = unsafe { add_operand_to_primitive(lt, lv) };
    let (rt, rv, r_prim) = unsafe { add_operand_to_primitive(rt, rv) };
    let release = |p: Option<AnyValue>| {
        if let Some(b) = p {
            unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(b) };
        }
    };
    // A double-object ToPrimitive failure recorded the TypeError —
    // answer undefined for the caller's throw check.
    if lt == i64::MIN || rt == i64::MIN {
        release(l_prim);
        release(r_prim);
        return crate::nanbox::VALUE_UNDEFINED;
    }
    // SAFETY: caller invariant — propagated.
    let l_is_str = unsafe { is_heap_str(lt, lv) };
    let r_is_str = unsafe { is_heap_str(rt, rv) };

    // §13.15.3 step 3 ToNumeric dispatch — after the String split
    // decides against concat (a BigInt beside a Str legitimately
    // concatenates via ToString below), a BigInt pair adds on the
    // bigint kernel and a mixed pair throws.
    if !l_is_str && !r_is_str {
        // SAFETY: caller invariant — propagated.
        if let Some(v) = unsafe {
            crate::arith_bigint::try_bigint_pair(
                lt,
                lv,
                rt,
                rv,
                crate::loose_eq::bigint_ffi::__torajs_bigint_add,
            )
        } {
            release(l_prim);
            release(r_prim);
            return v;
        }
    }

    // String concatenation path (ES §13.15.3 — either side String
    // wins). Both operands go through ToString; the two
    // intermediates are dropped after the concat owns its own
    // copy of the bytes.
    if l_is_str || r_is_str {
        // SAFETY: any_to_str preserves the Heap-payload Safety
        // contract; result is a freshly-owned Str (refcount=1).
        let l_str = unsafe { any_to_str(lt, lv) };
        let r_str = unsafe { any_to_str(rt, rv) };
        // Step 8c — ShortStr fast-path. When the concat result fits
        // in `≤ SHORT_STR_CAP` bytes, emit the ShortStr immediate
        // directly: skips the heap allocation of the str_concat
        // result + the eventual box_void_ptr round-trip into a
        // Heap+Str cell. l_str / r_str are still dropped (they
        // were freshly owned from any_to_str).
        // SAFETY: both l_str / r_str are valid Str heap blocks
        // from any_to_str.
        if let Some(short) = unsafe { try_concat_short(l_str, r_str) } {
            // SAFETY: both Strs were rc=1 from any_to_str.
            unsafe {
                __torajs_str_drop(l_str);
                __torajs_str_drop(r_str);
            }
            release(l_prim);
            release(r_prim);
            return short;
        }
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
        release(l_prim);
        release(r_prim);
        // concat is a freshly-owned heap pointer (rc=1);
        // box_void_ptr stores it in the AnyValue cell slot.
        // Caller owns the rc.
        return box_void_ptr(concat as *mut c_void);
    }

    // Numeric path. ToNumber reuses the per-tag dispatch from
    // P2.3-d.1; same predicates as any_arith for the I64 fast-
    // path (i64-shaped tags + lossless f64↔i64 round-trip).
    //
    // SAFETY: caller invariant — propagated.
    let l = unsafe { any_to_number(lt, lv) };
    let r = unsafe { any_to_number(rt, rv) };
    release(l_prim);
    release(r_prim);
    let sum = l + r;

    if tag_is_i64_shaped(lt)
        && tag_is_i64_shaped(rt)
        && sum >= i64::MIN as f64
        && sum <= i64::MAX as f64
    {
        let int_sum = sum as i64;
        if (int_sum as f64) == sum {
            return box_i64(int_sum);
        }
    }
    box_double(sum)
}

/// §13.15.3 step 1/2 for one `any_add` operand — a Heap non-Str
/// cell replaces itself with its ToPrimitive(default) `(tag,
/// value)`; the owned prim box rides back so the caller releases
/// it after use (`None` = the operand was already primitive /
/// Str). A ToPrimitive failure (both hooks object) answers
/// `(i64::MIN, 0, None)` with the TypeError pending.
unsafe fn add_operand_to_primitive(tag: i64, value: i64) -> (i64, i64, Option<AnyValue>) {
    if tag != AnySlotTag::Heap as i64 || value == 0 {
        return (tag, value, None);
    }
    let h = unsafe { &*(value as *const torajs_rc::HeapHeader) };
    // §7.1.1 step 3 — a Str cell IS the string primitive and a
    // Symbol cell IS the symbol primitive: ToPrimitive answers both
    // unchanged. The downstream ToString / ToNumber then records the
    // §7.1.17 / §7.1.4 TypeError for a symbol. Pre-fix a Symbol fell
    // into OrdinaryToPrimitive, whose toString probe answered the
    // descriptive string — `sym + ""` concatenated instead of
    // throwing.
    if matches!(
        h.tag(),
        torajs_rc::Tag::Str | torajs_rc::Tag::Symbol | torajs_rc::Tag::BigInt
    ) {
        // A BigInt cell IS the bigint primitive (§7.1.1 step 3, same
        // as Str/Symbol) — pre-fix it fell into OrdinaryToPrimitive's
        // object-method machinery over a non-object layout (UB).
        return (tag, value, None);
    }
    match unsafe { crate::to_primitive::heap_to_primitive_default(value as *mut c_void) } {
        Some(prim) => {
            let pt = crate::nanbox_encode::__torajs_anyv_unbox_tag(prim);
            let pv = crate::nanbox_encode::__torajs_anyv_unbox_value(prim);
            (pt, pv, Some(prim))
        }
        None => (i64::MIN, 0, None),
    }
}

/// Step 8c — try the ShortStr fast-path for the `any_add` string-
/// concat branch. When the combined bytes of `l_str` and `r_str`
/// fit in `≤ SHORT_STR_CAP` (= 5) bytes, build an inline ShortStr
/// [`AnyValue`] without going through [`__torajs_str_concat`] —
/// skipping one heap allocation (the concat buffer) plus the
/// eventual `box_void_ptr` round-trip into a Heap+Str cell.
///
/// A ShortStr payload is **UTF-8**: `materialize_short_str` hands
/// it to `__torajs_str_alloc`, which decodes it as UTF-8, and
/// `__torajs_any_length_get` counts its code units the same way.
/// A heap Str payload is neither — post-P11.1-S2 it is Latin-1 or
/// UTF-16 LE, and its `length` field counts **code units**, not
/// bytes. Those two descriptions agree on exactly one set of
/// strings: the ASCII ones, where a code unit is one byte and that
/// byte is already its own UTF-8 encoding. So both sides must be
/// Latin-1 *and* ASCII before their bytes may be copied across
/// verbatim; everything else goes to `__torajs_str_concat`, which
/// reads the encoding flag and gets it right.
///
/// Copying without that gate minted a ShortStr whose payload was
/// not UTF-8 at all, and the damage surfaced one step later, at
/// materialization: `"é" + "x"` packed the Latin-1 byte `0xE9`
/// ahead of `0x78 0x79`, and reading `E9 78 79` back as UTF-8
/// makes one three-byte sequence — `"éxy"` printed as `"鸹"`. A
/// Latin-1 byte with nothing valid behind it walked off the end of
/// the five-byte buffer instead and took the process down.
///
/// Returns `None` when either side is non-ASCII or the combined
/// length exceeds the ShortStr capacity; the caller falls back to
/// the heap-allocating `str_concat` path. Substr-layout inputs
/// (Tag::Str with `FLAG_SUBSTR_INLINE`) keep their bytes behind an
/// offset+parent indirection rather than at `STR_HDR_SIZE`, so
/// reading one here would be wrong — `any_to_str` materializes
/// them, and this fn only ever sees plain owned Strs.
///
/// # Safety
///
/// Both `l_str` / `r_str` must be valid Str heap blocks per the
/// `__torajs_str_concat` contract (the same precondition the
/// caller already required for the heap-concat path). Ownership
/// stays with the caller — this fn neither bumps nor drops
/// refcounts; it only reads bytes off the live blocks.
#[inline]
unsafe fn try_concat_short(l_str: *mut c_void, r_str: *mut c_void) -> Option<AnyValue> {
    let l_ptr = l_str as *const u8;
    let r_ptr = r_str as *const u8;
    // SAFETY: Str layout invariant — `flags: u16` at byte offset 6
    // inside the HeapHeader, `length: u32` at STR_LEN_OFF (= 8)
    // with the capacity slot in the four bytes above it.
    let (l_len, r_len) = unsafe {
        let is_latin1 = |p: *const u8| (*(p as *const HeapHeader)).flags & STR_FLAG_IS_LATIN1 != 0;
        if !is_latin1(l_ptr) || !is_latin1(r_ptr) {
            // UTF-16 payload: `length` counts u16s, so it is not
            // the byte count the copy below would need — and a
            // string wide enough to be stored that way has a
            // codepoint past 0xFF in it, which is never ASCII.
            return None;
        }
        (
            (l_ptr.add(STR_LEN_OFF) as *const u32).read() as usize,
            (r_ptr.add(STR_LEN_OFF) as *const u32).read() as usize,
        )
    };
    let total = l_len + r_len;
    if total > SHORT_STR_CAP {
        return None;
    }
    let mut bytes = [0u8; SHORT_STR_CAP];
    // SAFETY: payload bytes start at offset STR_HDR_SIZE and span
    // exactly `len` bytes (live Str invariant). Destination is the
    // stack-resident SHORT_STR_CAP-byte buffer; `total ≤
    // SHORT_STR_CAP` was proven above so destination offsets stay
    // in-bounds.
    unsafe {
        if l_len > 0 {
            core::ptr::copy_nonoverlapping(l_ptr.add(STR_HDR_SIZE), bytes.as_mut_ptr(), l_len);
        }
        if r_len > 0 {
            core::ptr::copy_nonoverlapping(
                r_ptr.add(STR_HDR_SIZE),
                bytes.as_mut_ptr().add(l_len),
                r_len,
            );
        }
    }
    // Latin-1 and UTF-8 agree below 0x80 and nowhere above it, so
    // this is all that is left to ask. It runs after the capacity
    // check on purpose: at most five bytes are ever scanned, however
    // long the operands are.
    if bytes[..total].iter().any(|b| *b >= 0x80) {
        return None;
    }
    try_box_short_str(&bytes[..total])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanbox::{is_short_str, short_str_bytes, short_str_len};

    /// Build a fake `[header:8][length:u32@8][pad:u32@12][bytes:N]`
    /// Latin-1 Str block on the heap (Vec<u8>) for unit-testing the
    /// layout-aware `try_concat_short`. `length` is the code-unit
    /// count, which for Latin-1 is the payload byte count; the
    /// `IS_LATIN1` flag has to be set because the fast-path reads it
    /// to learn that fact.
    fn make_fake_str(payload: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; STR_HDR_SIZE + payload.len()];
        buf[6..8].copy_from_slice(&STR_FLAG_IS_LATIN1.to_le_bytes());
        let len_bytes = (payload.len() as u32).to_le_bytes();
        buf[STR_LEN_OFF..STR_LEN_OFF + 4].copy_from_slice(&len_bytes);
        buf[STR_HDR_SIZE..].copy_from_slice(payload);
        buf
    }

    /// Same, for a UTF-16 LE Str: `length` counts u16 code units and
    /// the payload is twice that many bytes, with `IS_LATIN1` clear.
    fn make_fake_utf16(units: &[u16]) -> Vec<u8> {
        let mut buf = vec![0u8; STR_HDR_SIZE + units.len() * 2];
        let len_bytes = (units.len() as u32).to_le_bytes();
        buf[STR_LEN_OFF..STR_LEN_OFF + 4].copy_from_slice(&len_bytes);
        for (i, &u) in units.iter().enumerate() {
            buf[STR_HDR_SIZE + i * 2..STR_HDR_SIZE + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        buf
    }

    #[test]
    fn try_concat_short_empty_plus_empty() {
        let l = make_fake_str(b"");
        let r = make_fake_str(b"");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) }
            .expect("empty + empty fits");
        assert!(is_short_str(v));
        assert_eq!(short_str_len(v), 0);
    }

    #[test]
    fn try_concat_short_single_char_each() {
        let l = make_fake_str(b"a");
        let r = make_fake_str(b"b");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) }
            .expect("a + b fits");
        assert!(is_short_str(v));
        assert_eq!(short_str_len(v), 2);
        assert_eq!(&short_str_bytes(v)[..2], b"ab");
    }

    #[test]
    fn try_concat_short_total_exactly_5() {
        let l = make_fake_str(b"abc");
        let r = make_fake_str(b"de");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) }
            .expect("3 + 2 = 5 = SHORT_STR_CAP fits exactly");
        assert!(is_short_str(v));
        assert_eq!(short_str_len(v), 5);
        assert_eq!(short_str_bytes(v), *b"abcde");
    }

    #[test]
    fn try_concat_short_total_6_returns_none() {
        let l = make_fake_str(b"abc");
        let r = make_fake_str(b"def");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) };
        assert!(v.is_none(), "6 byte total overflows ShortStr cap");
    }

    #[test]
    fn try_concat_short_one_side_empty_passes_other_through() {
        // Left empty.
        let l = make_fake_str(b"");
        let r = make_fake_str(b"hi");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) }
            .expect("0 + 2 fits");
        assert!(is_short_str(v));
        assert_eq!(&short_str_bytes(v)[..2], b"hi");

        // Right empty.
        let l = make_fake_str(b"ok");
        let r = make_fake_str(b"");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) }
            .expect("2 + 0 fits");
        assert!(is_short_str(v));
        assert_eq!(&short_str_bytes(v)[..2], b"ok");
    }

    #[test]
    fn try_concat_short_byte_order_left_then_right() {
        // Regression: ensure l's bytes precede r's bytes (not
        // swapped).
        let l = make_fake_str(b"X");
        let r = make_fake_str(b"YZ");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) }
            .expect("3 byte total fits");
        assert_eq!(&short_str_bytes(v)[..3], b"XYZ");
    }

    #[test]
    fn try_concat_short_declines_latin1_supplement_bytes() {
        // "é" is one Latin-1 code unit stored as the single byte
        // 0xE9 — which is not its UTF-8 encoding, and a ShortStr
        // payload is UTF-8. Copying it across produced `E9 78 79`,
        // read back as the one codepoint U+9E39.
        let l = make_fake_str(&[0xE9, b'x']);
        let r = make_fake_str(b"y");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) };
        assert!(
            v.is_none(),
            "non-ASCII Latin-1 must fall through to str_concat"
        );
    }

    #[test]
    fn try_concat_short_declines_lone_latin1_byte() {
        // The shape that crashed: 0xA0 announces a UTF-8 sequence
        // whose continuation bytes are not there, so decoding the
        // materialized ShortStr ran off the buffer.
        let l = make_fake_str(&[0xA0]);
        let r = make_fake_str(&[0xA0]);
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) };
        assert!(v.is_none());
    }

    #[test]
    fn try_concat_short_declines_utf16_operand() {
        // "中" is one code unit in two bytes, so `length` is not a
        // byte count here; the old copy took byte 0x2D alone and
        // answered "-a".
        let l = make_fake_utf16(&[0x4E2D]);
        let r = make_fake_str(b"a");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) };
        assert!(v.is_none());
        // and on the right-hand side too
        let l2 = make_fake_str(b"a");
        let r2 = make_fake_utf16(&[0x4E2D]);
        let v2 =
            unsafe { try_concat_short(l2.as_ptr() as *mut c_void, r2.as_ptr() as *mut c_void) };
        assert!(v2.is_none());
    }

    #[test]
    fn try_concat_short_still_takes_the_ascii_path() {
        // The gate must not cost the case it exists for.
        let l = make_fake_str(b"ab");
        let r = make_fake_str(b"cde");
        let v = unsafe { try_concat_short(l.as_ptr() as *mut c_void, r.as_ptr() as *mut c_void) }
            .expect("all-ASCII, 5 bytes");
        assert_eq!(short_str_len(v), 5);
        assert_eq!(short_str_bytes(v), *b"abcde");
    }
}
