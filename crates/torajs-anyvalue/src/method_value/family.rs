//! Builtin-proto family axis of the method-cell intern (RFC
//! 20260721-string-proto-cluster 刀 3) — the family constants, the
//! cell-side family decode, and the receiver-shape → family map.
//! The intern table itself stays in the parent module (it packs
//! family and mid into one capture slot at mint time).

use core::ffi::c_void;

use torajs_rc::Tag;
use torajs_rc::{
    ANY_METHOD_HAS_OWN_PROPERTY, ANY_METHOD_IS_PROTOTYPE_OF, ANY_METHOD_PROPERTY_IS_ENUMERABLE,
    ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF,
};

use super::CLOSURE_CAP_BASE_OFF;
use crate::nanbox::{AnyValue, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str};

/// One intern row per builtin-proto family (torajs-rc
/// `builtin_proto.rs` tags, Number=0 … Function=13) plus row 0 for
/// family-less mints (the extern staticlib face / size getters).
/// ES §8.3: each prototype's method is a DISTINCT function object —
/// `String.prototype.concat` and `Array.prototype.concat` must not
/// intern to the same cell (a shared-mid cell made the `.call`
/// generic-coerce lane undecidable: G4).
pub(crate) const FAMILY_ROWS: usize = torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS + 1;

/// The String family's proto tag — the one family whose reified
/// methods run the §22.1.3 generic ToString(this) coerce on `.call`
/// / borrow re-dispatch.
pub(crate) const STR_PROTO_FAMILY: i64 = 3;

/// %TypedArray.prototype%'s family — every typed-array method cell is
/// minted on this one row, so all twelve kinds see the same function
/// object (§23.2.3.36's `values` identity among them).
pub(crate) const TYPEDARRAY_PROTO_FAMILY: i64 =
    torajs_rc::builtin_proto::TYPEDARRAY_PROTO_TAG as i64;

/// `Uint8Array.prototype`'s family (20 + Kind::Uint8) — the row of the
/// four text conversions it owns.
pub(crate) const UINT8_ARRAY_PROTO_FAMILY: i64 = 21;

/// The Array family's proto tag — its reified `concat` borrowed
/// onto a primitive receiver seeds `ToObject(this)` per §23.1.3.1
/// (the wrapper-object lane, see `method_call_closure::
/// generic_builtin_this`).
pub(crate) const ARR_PROTO_FAMILY: i64 = 2;

/// The Number family's proto tag — its reified toString / valueOf
/// brand-check thisNumberValue (§21.1.3) on borrow re-dispatch.
pub(crate) const NUM_PROTO_FAMILY: i64 = 0;

/// The Boolean family's proto tag — its reified toString / valueOf
/// brand-check thisBooleanValue (§20.3.3) on borrow re-dispatch.
pub(crate) const BOOL_PROTO_FAMILY: i64 = 4;

/// %ArrayBuffer.prototype% — §25.1.6's `[[ArrayBufferData]]` receivers.
pub(crate) const ARRAYBUFFER_PROTO_FAMILY: i64 = 19;

/// %DataView.prototype% — §25.3.4's `[[DataView]]` receivers.
pub(crate) const DATAVIEW_PROTO_FAMILY: i64 = 32;

/// %BigInt.prototype% — §21.2.3's `ThisBigIntValue` receivers.
pub(crate) const BIGINT_PROTO_FAMILY: i64 = 6;

/// %Function.prototype% — the row every Closure cell reads through.
pub(crate) const FN_PROTO_FAMILY: i64 = 13;
/// %Promise.prototype% — the `then` / `catch` / `finally` family
/// (`recv_proto_family` maps `Tag::Promise` here).
pub(crate) const PROMISE_PROTO_FAMILY: i64 = 10;

/// The Object-proto family row — the intern target for mids whose
/// implementation a family INHERITS from `Object.prototype`.
const OBJ_PROTO_FAMILY: i64 = 1;

/// Normalize a (family, mid) pair to the family whose cell identity
/// the reify answers. ES: a prototype without its OWN `valueOf` /
/// `toString` / `toLocaleString` / `hasOwnProperty` /
/// `isPrototypeOf` / `propertyIsEnumerable` inherits the ONE
/// function object on `Object.prototype`, so
/// `Function.prototype.valueOf === Object.prototype.valueOf` must
/// hold (test262 S15.3.4_A4). Per-family mint (the 刀 3 shape)
/// broke that identity for inherited reads — delegate those to the
/// Object row. The own table below is bun-verified
/// (`Object.prototype.hasOwnProperty.call(proto, mid)` per family):
/// families keep their own cells only where the spec gives the
/// prototype its own implementation.
pub(crate) fn intern_family(family: i64, mid: i64) -> i64 {
    if family < 0 {
        return family;
    }
    let owns = if mid == ANY_METHOD_VALUE_OF {
        // Number / Object / String / Boolean / Symbol / BigInt / Date
        matches!(family, 0 | 1 | 3 | 4 | 5 | 6 | 8)
    } else if family == TYPEDARRAY_PROTO_FAMILY && crate::method_support::is_uint8_text_method(mid)
    {
        // The base64 / hex four are `Uint8Array.prototype`'s own; a
        // typed array instance reads them from that row.
        return UINT8_ARRAY_PROTO_FAMILY;
    } else if mid == ANY_METHOD_TO_STRING && family == TYPEDARRAY_PROTO_FAMILY {
        // §23.2.3.33 — `%TypedArray%.prototype.toString` is the same
        // function object as `Array.prototype.toString`.
        return 2;
    } else if mid == ANY_METHOD_TO_STRING {
        // Every family except Promise / Map / Set and the weak
        // three has its own — those six brand themselves through
        // `Symbol.toStringTag` and inherit the one function object
        // on `Object.prototype` (bun: `WeakMap.prototype.toString
        // === Object.prototype.toString`).
        !matches!(family, 10 | 11 | 12 | 16 | 17 | 18)
    } else if mid == ANY_METHOD_TO_LOCALE_STRING {
        // Number / Object / Array / BigInt / Date / %TypedArray% —
        // §23.2.3.32's is its own function, not Array's
        // (`toString` is the one typed-array name that is shared).
        matches!(family, 0 | 1 | 2 | 6 | 8 | TYPEDARRAY_PROTO_FAMILY)
    } else if mid == ANY_METHOD_HAS_OWN_PROPERTY
        || mid == ANY_METHOD_IS_PROTOTYPE_OF
        || mid == ANY_METHOD_PROPERTY_IS_ENUMERABLE
        // The Annex B §B.2.2.2-5 accessor four are `Object.prototype`
        // properties too, so every family reads the one cell
        // (`Map.prototype.__defineGetter__ ===
        // Object.prototype.__defineGetter__`).
        || (torajs_rc::ANY_METHOD_DEFINE_GETTER..=torajs_rc::ANY_METHOD_LOOKUP_SETTER)
            .contains(&mid)
    {
        family == OBJ_PROTO_FAMILY
    } else {
        true
    };
    if owns { family } else { OBJ_PROTO_FAMILY }
}

/// The builtin-proto family a reified cell was minted for — -1 for
/// a family-less mint. Callers gate on
/// [`super::builtin_method_mid`] being `Some` first (this reads the
/// capture slot unconditionally).
pub unsafe fn builtin_method_family(ptr: *mut c_void) -> i64 {
    unsafe { ((*(ptr.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64) >> 32) as i64) - 1 }
}

/// The builtin-proto family a receiver's method reads resolve
/// against — the value-read twin of the per-tag dispatch ladder.
/// -1 = no builtin-proto family (the read stays family-less).
pub(crate) fn recv_proto_family(recv: AnyValue) -> i64 {
    if is_short_str(recv) {
        return STR_PROTO_FAMILY;
    }
    if is_bool(recv) {
        return 4;
    }
    if is_int32(recv) || is_double(recv) {
        return 0;
    }
    if !is_cell(recv) {
        return -1;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live heap pointer with a header.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    match tag {
        t if t == Tag::Str as u16 => STR_PROTO_FAMILY,
        t if t == Tag::Obj as u16 || t == Tag::DynObj as u16 => 1,
        t if t == Tag::Arr as u16 => 2,
        t if t == Tag::Closure as u16 => FN_PROTO_FAMILY,
        t if t == Tag::RegExp as u16 => 7,
        t if t == Tag::Date as u16 => 8,
        t if t == Tag::Symbol as u16 => 5,
        t if t == Tag::Promise as u16 => PROMISE_PROTO_FAMILY,
        t if t == Tag::BigInt as u16 => BIGINT_PROTO_FAMILY,
        t if t == Tag::Map as u16 => 11,
        t if t == Tag::Set as u16 => 12,
        t if t == Tag::WeakMap as u16 => 16,
        t if t == Tag::WeakSet as u16 => 17,
        t if t == Tag::WeakRef as u16 => 18,
        t if t == Tag::FinalizationRegistry as u16 => 39,
        // §25.1.6 — ArrayBuffer.prototype is its own singleton, so
        // its row is exact. A typed array reads through
        // %TypedArray.prototype%'s one row (RFC 20260916-typedarray-
        // intrinsic): per-kind rows would break §23.2.3.36's
        // requirement that all twelve see the SAME `values` function
        // object, and the shared parent is where the spec puts them.
        t if t == Tag::ArrayBuffer as u16 => ARRAYBUFFER_PROTO_FAMILY,
        t if t == Tag::TypedArray as u16 => TYPEDARRAY_PROTO_FAMILY,
        // §25.3.4 — DataView.prototype is its own singleton too, so
        // its row is exact for the same reason ArrayBuffer's is. Its
        // absence was not a recorded boundary like TypedArray's, just
        // a missing line: `proto_tostringtag_install` put the real
        // `@@toStringTag` on DataView.prototype, and
        // `new DataView(buf)[Symbol.toStringTag]` still answered
        // undefined because this walk had no row to reach it through
        // — the exact shape the doc above `inherited_dict` describes
        // for Map, one tag later.
        t if t == Tag::DataView as u16 => DATAVIEW_PROTO_FAMILY,
        // Iterator-protocol cells hang off their PER-FAMILY prototype
        // (%ArrayIteratorPrototype% et al), which in turn hangs off
        // %Iterator.prototype%. They used to answer 15 flatly, which
        // is where the `@@toStringTag` walk ran out of chain to look
        // at — and why each family's `next` had no prototype to be an
        // own property of.
        t if t == Tag::MapIter as u16
            || t == Tag::ArrIter as u16
            || t == Tag::IterHelper as u16 =>
        unsafe { torajs_rc::builtin_proto::iter_proto::iter_cell_proto_tag(ptr, t) },
        t if t == Tag::NumberWrapper as u16 => 0,
        t if t == Tag::StringWrapper as u16 => STR_PROTO_FAMILY,
        t if t == Tag::BooleanWrapper as u16 => 4,
        _ => -1,
    }
}
