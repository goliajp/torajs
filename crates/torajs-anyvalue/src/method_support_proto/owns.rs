//! Which builtin prototype owns which method id.
//!
//! Split from the parent at the 500-line limit. The parent reads
//! these to answer a property; this asks the prior question — does
//! the surface HAVE it — off the tag tables locked to
//! `torajs-rc/builtin_proto.rs`. A child module reaches the parent's
//! private items directly, so nothing changed visibility.

use super::*;

/// The builtin-proto tag's method surface — tag order is locked to
/// `torajs-rc/builtin_proto.rs` (Number=0 … Function=13). Tags with
/// no any-dispatch method arm today (Object beyond the universal
/// probes / Symbol / BigInt / Error / Promise) answer false — the
/// read stays undefined rather than minting a cell that would
/// TypeError on `f.call(recv, …)`.
pub(super) fn proto_tag_supports(tag: i64, mid: i64) -> bool {
    // A prototype's READABLE surface is its own methods plus
    // everything it inherits — and every builtin prototype's
    // [[Prototype]] chain ends at `Object.prototype`, so asking
    // that row is the whole of the inherited half. Asking it as a
    // chain link rather than as a hard-coded list of names is what
    // makes `Map.prototype.toString` resolve at all: the list had
    // four of Object.prototype's six methods on it and had silently
    // left `toString` / `toLocaleString` (plus the Annex B
    // §B.2.2.2-5 accessor four) unreadable on every family that
    // inherits rather than owns them, while `"toString" in
    // Map.prototype` — walking the same chain through
    // `__torajs_proto_chain_key_owned` — answered true all along.
    //
    // Routing through `proto_tag_owns` on both sides also keeps the
    // delete tombstone honest in the inherited direction: after
    // `delete Object.prototype.toString`, every family's read goes
    // undefined with it, which the old early-return could not have
    // expressed.
    proto_tag_owns(tag, mid)
        || proto_tag_owns(torajs_rc::builtin_proto::OBJECT_PROTO_TAG as i64, mid)
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
    // Same shape of exception, same reason: `toLocaleString` is an
    // `Object.prototype` property (§20.1.3.5) that only Number /
    // Array / Date / BigInt redefine (ECMA-402 §18-20). The per-arm
    // `*_supports` tables answer "this ARM resolves this mid", which
    // is a different question — `String.prototype` and
    // `Function.prototype` own no toLocaleString in any engine, yet
    // their arms answer the inherited call, so reading ownership off
    // the dispatch table said true for both. The family list here is
    // the one `intern_family` already carries for this mid; asking
    // one question in two places had let the two drift.
    if mid == ANY_METHOD_TO_LOCALE_STRING {
        return matches!(tag, 0 | 1 | 2 | 6 | 8);
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
                ANY_METHOD_VALUE_OF
                    | ANY_METHOD_TO_LOCALE_STRING
                    | ANY_METHOD_TO_STRING
                    | ANY_METHOD_IS_PROTOTYPE_OF
            ) || (torajs_rc::ANY_METHOD_DEFINE_GETTER..=torajs_rc::ANY_METHOD_LOOKUP_SETTER)
                .contains(&mid)
        }
        2 => arr_supports(mid),
        3 => str_supports(mid),
        // Boolean.prototype owns toString + valueOf (§20.3.3);
        // toLocaleString is inherited.
        4 => matches!(mid, ANY_METHOD_TO_STRING | ANY_METHOD_VALUE_OF),
        // Symbol.prototype owns toString + valueOf (§20.4.3);
        // description is an accessor, not a method.
        5 => matches!(mid, ANY_METHOD_TO_STRING | ANY_METHOD_VALUE_OF),
        // BigInt.prototype owns toString + toLocaleString + valueOf
        // (§21.2.3).
        6 => matches!(
            mid,
            ANY_METHOD_TO_STRING | ANY_METHOD_TO_LOCALE_STRING | ANY_METHOD_VALUE_OF
        ),
        7 => regexp_supports(mid),
        8 => date_supports(mid),
        // Promise.prototype owns then / catch / finally (§27.2.5).
        10 => matches!(
            mid,
            torajs_rc::ANY_METHOD_THEN
                | torajs_rc::ANY_METHOD_CATCH
                | torajs_rc::ANY_METHOD_FINALLY
        ),
        11 => map_supports(mid),
        12 => set_supports(mid),
        13 => closure_supports(mid),
        // %WeakMap.prototype% / %WeakSet.prototype% (§24.3.3 /
        // §24.4.3) read off the SAME per-arm tables the instance
        // dispatch uses — the pair the rotation-148 drift lesson
        // says must move together.
        16 => weakmap_supports(mid),
        17 => weakset_supports(mid),
        18 => weakref_supports(mid),
        _ => false,
    }
}
