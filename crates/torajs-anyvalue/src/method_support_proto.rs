//! Builtin `<Ctor>.prototype` singleton faces — support / own /
//! alias / accessor probes + the extern method-value & gOPD cells
//! (split from `method_support.rs`, file-size limit; RFC
//! 20260713-array-proto-residual blade 2 grew the alias face past
//! 500). The per-receiver-shape `*_supports` id tables stay in the
//! parent file — extend the two together.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_HAS_OWN_PROPERTY, ANY_METHOD_KEYS, ANY_METHOD_PROPERTY_IS_ENUMERABLE,
    ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_UNKNOWN, ANY_METHOD_VALUE_OF,
    ANY_METHOD_VALUES,
};

use crate::method_support::{
    arr_supports, closure_supports, date_supports, map_supports, num_supports, regexp_supports,
    set_supports, str_supports,
};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-arr — the same pair against an array's side props
    /// (`Array.prototype` is an Arr cell, so its monkey-patches land
    /// there instead of in a dynobj).
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — invoke an accessor pair's getter face
    /// against a receiver (owned AnyValue return).
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
}

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of `method_call_dynobj.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: u64 = 6;

/// Offset of the `len` slot in an Array heap block
/// (`torajs_arr::layout::ARR_LEN_OFF`).
const ARR_LEN_OFF: usize = 8;

/// `Array.prototype` is the one builtin prototype backed by an Arr
/// cell (ES §23.1.3) rather than a dynobj.
///
/// # Safety
/// `proto` is a live heap cell.
unsafe fn proto_is_arr(proto: *mut c_void) -> bool {
    // HeapHeader: rc @ +0 (u32), type_tag @ +4 (u16).
    let cell_tag = unsafe { proto.cast::<u8>().add(4).cast::<u16>().read() };
    cell_tag == torajs_rc::Tag::Arr as u16
}

/// Own-property probe pair against a builtin prototype, whichever
/// cell shape backs it — `Array.prototype` is an Arr (ES §23.1.3),
/// the rest are dynobjs. Reading an Arr through the dynobj probe
/// would walk an entry table that isn't there.
unsafe fn proto_own_probe(proto: *mut c_void, key: *const c_void) -> (i64, i64) {
    let is_arr = unsafe { proto_is_arr(proto) };
    let (tag, value) = if is_arr {
        unsafe {
            (
                __torajs_arrprops_get_tag(proto, key),
                __torajs_arrprops_get_value(proto, key),
            )
        }
    } else {
        unsafe {
            (
                __torajs_dynobj_get_tag(proto, key),
                __torajs_dynobj_get_value(proto, key),
            )
        }
    };
    (tag as i64, value as i64)
}

/// The builtin-proto tag's method surface — tag order is locked to
/// `torajs-rc/builtin_proto.rs` (Number=0 … Function=13). Tags with
/// no any-dispatch method arm today (Object beyond the universal
/// probes / Symbol / BigInt / Error / Promise) answer false — the
/// read stays undefined rather than minting a cell that would
/// TypeError on `f.call(recv, …)`.
fn proto_tag_supports(tag: i64, mid: i64) -> bool {
    // Object.prototype's universal own-property probes resolve on
    // every receiver shape, so every builtin prototype hands them
    // out (mirrors `builtin_method_supported`'s leading arm); the
    // per-family surface is shared with the own-property variant.
    if mid == ANY_METHOD_HAS_OWN_PROPERTY
        || mid == ANY_METHOD_PROPERTY_IS_ENUMERABLE
        || mid == ANY_METHOD_VALUE_OF
    {
        return true;
    }
    proto_tag_owns(tag, mid)
}

/// Own-property variant of [`proto_tag_supports`] (RFC 20260712
/// chunk 1) — the readable surface counts inherited methods (the
/// universal `Object.prototype` probes resolve on every prototype),
/// but an own-property probe must NOT: `hasOwnProperty` /
/// `propertyIsEnumerable` are own only on `Object.prototype`
/// (tag 1). Every family method IS its prototype's own property
/// per spec.
///
/// Chunk 3 — a `delete <Ctor>.prototype.<m>` tombstone (torajs-rc
/// deleted-mid bitmask) clears the answer for every consumer in one
/// place: prop_has / gOPD synthesis / the member-read fallthrough /
/// the static `String.prototype.small` read all funnel here. A
/// dynobj own entry is probed BEFORE any of them consult this, so a
/// set / defineProperty restore revives without a clear call.
pub(crate) fn proto_tag_owns(tag: i64, mid: i64) -> bool {
    // SAFETY: pure bitmask read; range-checked inside.
    if unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(tag, mid) } != 0 {
        return false;
    }
    proto_tag_family_owns(tag, mid)
}

/// The raw family-membership half of [`proto_tag_owns`] — no
/// tombstone consultation. `prop_delete` gates its mark on this
/// (marking must not depend on the current deleted state).
pub(crate) fn proto_tag_family_owns(tag: i64, mid: i64) -> bool {
    if mid == ANY_METHOD_HAS_OWN_PROPERTY || mid == ANY_METHOD_PROPERTY_IS_ENUMERABLE {
        return tag == 1;
    }
    match tag {
        0 => num_supports(mid),
        // Object.prototype's own methods beyond the universal
        // probes handled above — valueOf / toLocaleString /
        // toString (§20.1.4.7 / §20.1.4.6 / §20.1.3.6; the alias
        // below swaps toString onto the distinct badge cell) plus
        // the Annex B §B.2.2.2-5 legacy accessor four (RFC
        // 20260713-annexb-legacy-accessor).
        1 => {
            matches!(
                mid,
                ANY_METHOD_VALUE_OF | ANY_METHOD_TO_LOCALE_STRING | ANY_METHOD_TO_STRING
            ) || (torajs_rc::ANY_METHOD_DEFINE_GETTER..=torajs_rc::ANY_METHOD_LOOKUP_SETTER)
                .contains(&mid)
        }
        2 => arr_supports(mid),
        3 => str_supports(mid),
        // Boolean.prototype owns toString + valueOf (§20.3.3);
        // toLocaleString is inherited.
        4 => matches!(mid, ANY_METHOD_TO_STRING | ANY_METHOD_VALUE_OF),
        7 => regexp_supports(mid),
        8 => date_supports(mid),
        11 => map_supports(mid),
        12 => set_supports(mid),
        13 => closure_supports(mid),
        _ => false,
    }
}

/// gOPD own-entry synthesis probe (RFC 20260712 chunk 2) — when a
/// dynobj turns out to be a builtin `<Ctor>.prototype` singleton and
/// `key` interns to a method its family owns, hand out the immortal
/// interned cell so torajs-meta can synthesize the spec method
/// descriptor `{value: cell, writable: true, enumerable: false,
/// configurable: true}`. 0 = not a builtin proto / not an owned
/// method (the caller keeps its undefined answer).
///
/// # Safety
/// `dynobj` is NULL or a live heap cell; `key` is NULL or a live
/// Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_own_method_cell(
    dynobj: *const c_void,
    key: *const c_void,
) -> u64 {
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(dynobj) };
    if tag < 0 {
        return 0;
    }
    // `constructor` is an own property of every builtin prototype
    // (§20.x.3.1 family) — one interned identity per tag, so gOPD's
    // desc.value and a member read answer the SAME cell. Probed
    // before the method-id intern (constructor has no mid).
    if unsafe { crate::prop_has::key_is(key, b"constructor") } {
        return crate::method_value::builtin_ctor_cell(tag) as u64;
    }
    let mid = unsafe { crate::method_value::key_method_id(key) };
    if mid == ANY_METHOD_UNKNOWN
        || (mid as usize) >= crate::method_value::TABLE_SIZE
        || !proto_tag_owns(tag, mid)
    {
        return 0;
    }
    // Immortal interned cell — rc traffic no-ops.
    crate::method_value::builtin_method_cell(set_keys_alias(tag, mid)) as u64
}

/// Per-prototype cell aliases — a name-interned mid that a given
/// `<Ctor>.prototype` hands out as a DIFFERENT function object:
/// - §24.2.4.8 `Set.prototype.keys` IS the values function (tag 12,
///   `torajs-rc/builtin_proto.rs` order), so the two compare `===`.
/// - §20.1.3.6 `Object.prototype.toString` (tag 1) is the
///   "[object X]" badge classifier, distinct from every
///   per-receiver `toString` (RFC 20260713-array-proto-residual
///   blade 2).
fn set_keys_alias(tag: i64, mid: i64) -> i64 {
    if tag == 12 && mid == ANY_METHOD_KEYS {
        return ANY_METHOD_VALUES;
    }
    if tag == 1 && mid == ANY_METHOD_TO_STRING {
        return torajs_rc::ANY_METHOD_OBJECT_TO_STRING;
    }
    mid
}

/// The per-family accessor probe: `Map.prototype` (tag 11) and
/// `Set.prototype` (tag 12) own a `size` accessor (§24.1.3.10 /
/// §24.2.3.9). Answers the carried accessor id for the tombstone
/// bitmask; `None` for every other (tag, key) pair.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn proto_tag_accessor_mid(tag: i64, key: *const c_void) -> Option<i64> {
    if tag != 11 && tag != 12 {
        return None;
    }
    if key.is_null() {
        return None;
    }
    // Same read shape as key_method_id — ASCII name, Latin-1 payload.
    use crate::method_value::{STR_DATA_OFF, STR_LEN_OFF};
    let len = unsafe { (key.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() } as usize;
    if len != 4 {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(STR_DATA_OFF), 4) };
    if bytes == b"size" {
        Some(torajs_rc::ANY_METHOD_GET_SIZE)
    } else {
        None
    }
}

/// gOPD accessor-entry synthesis probe (set-proto-cluster C2-size)
/// — when a dynobj is a builtin `Map.prototype` / `Set.prototype`
/// singleton and `key` is the `size` accessor its family owns (and
/// no `delete` tombstone hides it), hand out the immortal getter
/// cell so torajs-meta can synthesize the spec accessor descriptor
/// `{get: cell, set: undefined, enumerable: false, configurable:
/// true}`. 0 = not applicable (the caller keeps its answer).
///
/// # Safety
/// `dynobj` is NULL or a live heap cell; `key` is NULL or a live
/// Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_own_accessor_getter(
    dynobj: *const c_void,
    key: *const c_void,
) -> u64 {
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(dynobj) };
    if tag < 0 {
        return 0;
    }
    let Some(mid) = (unsafe { proto_tag_accessor_mid(tag, key) }) else {
        return 0;
    };
    if unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(tag, mid) } != 0 {
        return 0;
    }
    crate::method_value::size_getter_cell(tag) as u64
}

/// `<Ctor>.prototype.<m>` static member read (chunk A). Own-entry
/// probe on the builtin-proto singleton dynobj first — a user
/// monkey-patch (`String.prototype.foo = …`) wins over the interned
/// builtin cell, matching ES own-property-before-intrinsic order.
/// Data-tagged hits (null/bool/i64/f64/heap) box out under the
/// owned protocol; absent (5, 0) and accessor-sentinel entries fall
/// through to the method-cell table. Unknown / unsupported names
/// answer undefined (bun's wrong-arm shape).
///
/// # Safety
/// `key` is NULL or a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_method_value(
    tag: i64,
    key: *const c_void,
) -> AnyValue {
    if key.is_null() {
        return VALUE_UNDEFINED;
    }
    let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(tag) };
    if !proto.is_null() {
        let (dtag, dval) = unsafe { proto_own_probe(proto, key) };
        if (0..=4).contains(&dtag) {
            // The probe pair is a borrow — the returned box owns
            // its own reference.
            crate::payload_rc_inc(dtag, dval);
            return unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(dtag, dval) };
        }
        // Accessor own entry (RFC 20260718-accessor-reify 刀 1 —
        // e.g. `Object.prototype.__proto__`): §10.1.8.1 [[Get]]
        // invokes the getter with the singleton itself as receiver.
        if dtag == ANY_ACCESSOR_TAG as i64 {
            let recv = unsafe { crate::nanbox_encode::__torajs_anyv_box_pointer(proto) };
            return unsafe { __torajs_accessor_invoke_getter(dval as u64 as *const c_void, recv) };
        }
        // `Array.prototype.length` is the Arr cell's own length, not a
        // method name — and it moves (`Array.prototype[0] = false`
        // grows it to 1, which is how test262 sets up an inherited
        // index property). Every other prototype falls through to the
        // method table.
        if unsafe { proto_is_arr(proto) } && unsafe { crate::prop_has::key_is(key, b"length") } {
            let len = unsafe { proto.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
            return crate::nanbox_encode::__torajs_anyv_box_i64(len as i64);
        }
    }
    let mid = unsafe { crate::method_value::key_method_id(key) };
    if mid == ANY_METHOD_UNKNOWN
        || (mid as usize) >= crate::method_value::TABLE_SIZE
        || !proto_tag_supports(tag, mid)
    {
        return VALUE_UNDEFINED;
    }
    // Immortal interned cell — rc traffic no-ops, hand out as-is.
    crate::method_value::builtin_method_cell(set_keys_alias(tag, mid)) as AnyValue
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_rc::{ANY_METHOD_CALL, ANY_METHOD_SLICE, ANY_METHOD_SPLIT};

    #[test]
    fn proto_tags_route_to_their_family() {
        // String proto answers str methods, not date's.
        assert!(proto_tag_supports(3, ANY_METHOD_SLICE));
        assert!(!proto_tag_supports(3, torajs_rc::ANY_METHOD_GET_TIME));
        // Date proto answers annexB getYear.
        assert!(proto_tag_supports(8, torajs_rc::ANY_METHOD_GET_YEAR));
        // Function proto answers call/apply/bind only.
        assert!(proto_tag_supports(13, ANY_METHOD_CALL));
        assert!(!proto_tag_supports(13, ANY_METHOD_SLICE));
        // No-arm tags answer false for ordinary methods…
        assert!(!proto_tag_supports(9, ANY_METHOD_TO_STRING));
        // …but the universal probes resolve everywhere.
        assert!(proto_tag_supports(9, ANY_METHOD_HAS_OWN_PROPERTY));
        assert!(proto_tag_supports(1, ANY_METHOD_PROPERTY_IS_ENUMERABLE));
    }

    #[test]
    fn tombstone_hides_owned_method() {
        assert!(proto_tag_owns(3, ANY_METHOD_SLICE));
        unsafe {
            torajs_rc::builtin_proto::__torajs_builtin_proto_mark_deleted(3, ANY_METHOD_SLICE)
        };
        assert!(!proto_tag_owns(3, ANY_METHOD_SLICE));
        // …the raw family half stays true (prop_delete's mark gate)…
        assert!(proto_tag_family_owns(3, ANY_METHOD_SLICE));
        // …and the readable surface funnels through the same check.
        assert!(!proto_tag_supports(3, ANY_METHOD_SLICE));
        // Sibling tag / sibling mid unaffected.
        assert!(proto_tag_owns(2, ANY_METHOD_SLICE));
        assert!(proto_tag_owns(3, ANY_METHOD_SPLIT));
    }
}
