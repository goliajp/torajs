//! The operand-side ToPrimitive of the any binary operators
//! (`any_add` hint default, the §13.6-§13.12 numeric family hint
//! number) — split from `arith.rs` under the 500-line file rule when
//! the numeric twin joined (rotation 628). Bodies verbatim.

use core::ffi::c_void;

use crate::nanbox::AnyValue;
use torajs_rc::AnySlotTag;

/// §13.15.3 step 1/2 for one `any_add` operand — a Heap non-Str
/// cell replaces itself with its ToPrimitive(default) `(tag,
/// value)`; the owned prim box rides back so the caller releases
/// it after use (`None` = the operand was already primitive /
/// Str). A ToPrimitive failure (both hooks object) answers
/// `(i64::MIN, 0, None)` with the TypeError pending.
pub(crate) unsafe fn add_operand_to_primitive(
    tag: i64,
    value: i64,
) -> (i64, i64, Option<AnyValue>) {
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
            // A ShortStr prim materialized an owned rc=1 Str in the
            // unbox above, and the caller's release (anyv_rc_dec)
            // no-ops on the immediate — hand back the re-boxed
            // materialization as the owner instead, so the release
            // frees it (546-02 M1 family; `({toString:()=>"ab"}) + 1`
            // leaked one Str per add).
            if crate::nanbox::is_short_str(prim) && pv != 0 {
                return (pt, pv, Some(crate::nanbox::box_void_ptr(pv as *mut c_void)));
            }
            (pt, pv, Some(prim))
        }
        None => (i64::MIN, 0, None),
    }
}

/// The hint-NUMBER twin of [`add_operand_to_primitive`] for the
/// §13.6-§13.12 numeric family (rotation 628): an ordinary object
/// operand replaces itself with its ToPrimitive(number) pair
/// (`[Symbol.toPrimitive]` → valueOf → toString), the owned prim box
/// rides back for the caller to release; primitives and Str / Symbol
/// / BigInt cells pass untouched (`None`). A failure answers
/// `(i64::MIN, 0, None)` with the TypeError pending.
pub(crate) unsafe fn numeric_operand_to_primitive(
    tag: i64,
    value: i64,
) -> (i64, i64, Option<AnyValue>) {
    if tag != AnySlotTag::Heap as i64 || value == 0 {
        return (tag, value, None);
    }
    let h = unsafe { &*(value as *const torajs_rc::HeapHeader) };
    if matches!(
        h.tag(),
        torajs_rc::Tag::Str | torajs_rc::Tag::Symbol | torajs_rc::Tag::BigInt
    ) {
        return (tag, value, None);
    }
    match unsafe { crate::to_primitive::heap_to_primitive(value as *mut c_void, false) } {
        Some(prim) => {
            let pt = crate::nanbox_encode::__torajs_anyv_unbox_tag(prim);
            let pv = crate::nanbox_encode::__torajs_anyv_unbox_value(prim);
            if crate::nanbox::is_short_str(prim) && pv != 0 {
                return (pt, pv, Some(crate::nanbox::box_void_ptr(pv as *mut c_void)));
            }
            (pt, pv, Some(prim))
        }
        None => (i64::MIN, 0, None),
    }
}

/// ToNumeric's one refusal on a PRIMITIVE (§7.1.3 → §7.1.4 step 5 —
/// a Symbol has no numeric value): records the TypeError and answers
/// `true`. Consulted right after each operand's ToPrimitive in the
/// numeric family, so a Symbol-yielding LEFT operand throws before
/// the right operand's hooks run (the order-of-evaluation family's
/// `?ToNumeric(lhs) throws` step).
///
/// # Safety
/// A Heap `tag` carries a live cell in `value`.
pub(crate) unsafe fn symbol_refuses_to_numeric(tag: i64, value: i64) -> bool {
    if tag != AnySlotTag::Heap as i64 || value == 0 {
        return false;
    }
    let h = unsafe { &*(value as *const torajs_rc::HeapHeader) };
    if !matches!(h.tag(), torajs_rc::Tag::Symbol) {
        return false;
    }
    unsafe {
        crate::member_set::__torajs_throw_type_error(
            c"Cannot convert a symbol to a number".as_ptr(),
        );
    }
    true
}
