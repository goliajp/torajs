//! Per-receiver-shape support table — the mid dispatch a receiver
//! arm resolves for VALUE reads (`typeof s.slice`,
//! `Object.getOwnPropertyDescriptor(s, "slice")`). One arm per
//! `method_call_*` dispatch module; extend together when an arm
//! grows a method.
//!
//! rotation-197 file-size sweep — extracted verbatim from
//! `method_value.rs`; the parent had drifted to 525 prod LOC on top
//! of the wrap of `Symbol` / `BigInt` / `Promise` / `DynObj/Obj`
//! method-value reads (rotations 141-159 wedges).

use torajs_rc::{ANY_METHOD_NEXT, ANY_METHOD_TO_STRING, Tag};

use crate::method_support::{
    arr_supports, closure_supports, date_supports, map_supports, num_supports, regexp_supports,
    set_supports, str_supports, weakmap_supports, weakref_supports, weakset_supports,
};
use crate::nanbox::{AnyValue, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str};

/// Exact per-receiver-shape support table — one arm per
/// `method_call_*` dispatch module, listing the ids that arm
/// resolves (extend together when an arm grows a method).
pub(crate) fn builtin_method_supported(recv: AnyValue, mid: i64) -> bool {
    // chunk D-1 — the universal own-property probes resolve on every
    // receiver shape (Object.prototype methods; primitives coerce
    // through ToObject and simply answer false-valued Bools).
    // valueOf joins them (§20.1.4.7 — identity on every cell, the
    // immediate itself on primitives; the dispatcher's universal
    // arm makes it callable everywhere).
    // isPrototypeOf joins them for the same reason (§20.1.3.3 — an
    // Object.prototype method the dispatcher already answers on every
    // receiver); it was callable but not readable, so `typeof
    // o.isPrototypeOf` needed a name-based shortcut in ssa_lower to
    // avoid saying "undefined".
    if mid == torajs_rc::ANY_METHOD_HAS_OWN_PROPERTY
        || mid == torajs_rc::ANY_METHOD_PROPERTY_IS_ENUMERABLE
        || mid == torajs_rc::ANY_METHOD_VALUE_OF
        || mid == torajs_rc::ANY_METHOD_IS_PROTOTYPE_OF
    {
        return true;
    }
    if is_short_str(recv) {
        return str_supports(mid);
    }
    if is_int32(recv) || is_double(recv) {
        return num_supports(mid);
    }
    if is_bool(recv) {
        return mid == ANY_METHOD_TO_STRING || mid == torajs_rc::ANY_METHOD_TO_LOCALE_STRING;
    }
    if !is_cell(recv) {
        return false;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live heap pointer.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    // §20.1.3.6 / §20.1.4.6 — every cell's chain ends at
    // Object.prototype, and `cell_method_inheriting` answers both
    // calls for a tag whose own arm claims neither. Asking that once
    // here is what the per-tag rows kept getting wrong one family at
    // a time: Obj / DynObj, then Symbol, then BigInt each had to be
    // told separately, and Map / Set / Promise never were — reading
    // `m.toString` said undefined while calling `m.toString()`
    // answered "[object Map]".
    if mid == ANY_METHOD_TO_STRING || mid == torajs_rc::ANY_METHOD_TO_LOCALE_STRING {
        return true;
    }
    match tag {
        t if t == Tag::Str as u16 => str_supports(mid),
        t if t == Tag::Arr as u16 => arr_supports(mid),
        t if t == Tag::Map as u16 => map_supports(mid),
        t if t == Tag::Set as u16 => set_supports(mid),
        // Iterator-protocol cells (RFC 20260730-iterator-global
        // 刀 2c): next plus the %Iterator.prototype% helper family —
        // the reified cell re-dispatches the mid against the
        // receiver, landing in the same arms the direct call takes.
        // ArrIter joins MapIter (its missing `next` read was a
        // recorded asymmetry).
        t if t == Tag::MapIter as u16
            || t == Tag::ArrIter as u16
            || t == Tag::IterHelper as u16 =>
        {
            iter_face_supports(t, mid)
        }
        t if t == Tag::Date as u16 => date_supports(mid),
        t if t == Tag::RegExp as u16 => regexp_supports(mid),
        t if t == Tag::WeakMap as u16 => weakmap_supports(mid),
        t if t == Tag::WeakSet as u16 => weakset_supports(mid),
        t if t == Tag::WeakRef as u16 => weakref_supports(mid),
        t if t == Tag::Closure as u16 => closure_supports(mid),
        // §27.2.5 Promise.prototype — then / catch / finally own
        // methods; the dispatcher has always answered the calls, but
        // reading one as a value (`const t = p.then`) said undefined.
        // Universal Object.prototype probes answer above.
        t if t == Tag::Promise as u16 => {
            mid == torajs_rc::ANY_METHOD_THEN
                || mid == torajs_rc::ANY_METHOD_CATCH
                || mid == torajs_rc::ANY_METHOD_FINALLY
        }
        // Plain objects (dynobj / static-layout struct) reach the
        // Annex B §B.2.2.2-5 legacy accessor four, which they inherit
        // from Object.prototype like any other object — the dispatcher
        // has answered those calls since RFC
        // 20260713-annexb-legacy-accessor, but reading one as a value
        // (`typeof o.__defineGetter__`, `f.call(o, …)`) went through
        // here and said undefined. Their remaining methods resolve by
        // name probe, not by mid. Symbol / BigInt need no row at all
        // now: the SymbolDescriptiveString and radix toString arms
        // are reached through the universal answer above, and valueOf
        // through the identity arm at the top.
        t if t == Tag::DynObj as u16 || t == Tag::Obj as u16 => {
            (torajs_rc::ANY_METHOD_DEFINE_GETTER..=torajs_rc::ANY_METHOD_LOOKUP_SETTER)
                .contains(&mid)
        }
        _ => false,
    }
}

/// The iterator-protocol face (MapIter / ArrIter / IterHelper —
/// RFC 20260730-iterator-global 刀 2c): next plus the helper family
/// every iterator inherits from %Iterator.prototype% (flatMap
/// included since 刀 4).
///
/// `return` belongs to %IteratorHelperPrototype% ALONE (§27.1.5.2)
/// — the array / map iterator prototypes never had one (§23.1.5 /
/// §24.1.5), a read there stays undefined and the §7.4.9 close tier
/// treats it as the spec's silent no-op. The first 刀 2c cut listed
/// `return` for all three tags; the close tier then read the
/// reified cell off a MapIter and bare-invoked it (no receiver) —
/// the 3-fail gate behind the `e93c16a5` revert.
fn iter_face_supports(tag: u16, mid: i64) -> bool {
    if mid == torajs_rc::any_method::ANY_METHOD_ITER_RETURN {
        return tag == Tag::IterHelper as u16;
    }
    mid == ANY_METHOD_NEXT
        || mid == torajs_rc::ANY_METHOD_MAP
        || mid == torajs_rc::ANY_METHOD_FILTER
        || mid == torajs_rc::ANY_METHOD_FLAT_MAP
        || mid == torajs_rc::any_method_iter::ANY_METHOD_TAKE
        || mid == torajs_rc::any_method_iter::ANY_METHOD_DROP
        || mid == torajs_rc::any_method_iter::ANY_METHOD_TO_ARRAY
        || mid == torajs_rc::ANY_METHOD_FOR_EACH
        || mid == torajs_rc::ANY_METHOD_SOME
        || mid == torajs_rc::ANY_METHOD_EVERY
        || mid == torajs_rc::ANY_METHOD_FIND
        || mid == torajs_rc::ANY_METHOD_REDUCE
}
