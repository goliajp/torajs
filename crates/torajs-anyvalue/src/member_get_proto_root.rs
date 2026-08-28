//! The %Object.prototype% hop every non-dynobj receiver owes after its
//! own family prototype misses (§10.1.8.1 OrdinaryGet step 4, 517-07).
//!
//! The dynobj lane already has it: `member_get_own::implicit_proto_parent`
//! hands back the root cell and the walk recurses through its full
//! [[Get]]. The other lanes have no dynobj proto pair to ask — an `Arr`
//! receiver's family prototype is itself an Arr cell, a wrapper's is a
//! tag-keyed singleton — and their walk ends at the reify probe, so a
//! property the program installed on `Object.prototype` had no path to
//! them at all: `Object.prototype.foo = 5; ([] as any).foo` answered
//! undefined while `({} as any).foo` answered 5.
//!
//! Reading the root's own expando and stopping is not a shortcut here.
//! %Object.prototype% IS the chain root (its own proto pair answers
//! Null), and the spec-given methods on it were already offered by the
//! caller's reify probe — what is missing is only what a program put
//! there.

use core::ffi::c_void;

use crate::member_get_own::OBJECT_PROTO_TAG;

unsafe extern "C" {
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
}

/// The absent tag, when the root has never been materialized — a
/// program that never touched `Object.prototype` cannot have installed
/// anything on it.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn object_proto_expando_tag(key: *const c_void) -> u64 {
    let root =
        unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(OBJECT_PROTO_TAG) };
    if root.is_null() {
        return 5;
    }
    unsafe { __torajs_dynobj_get_tag(root as *const c_void, key) }
}

/// Value twin of [`object_proto_expando_tag`]; 0 is the absent value.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn object_proto_expando_value(key: *const c_void) -> u64 {
    let root =
        unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(OBJECT_PROTO_TAG) };
    if root.is_null() {
        return 0;
    }
    unsafe { __torajs_dynobj_get_value(root as *const c_void, key) }
}

/// One prototype's own entry for a METHOD name, then the chain root's
/// (§10.1.9.2, 521-05). PEEKs rather than materializes, because the
/// call channel's miss exit has to stay allocation-free and a
/// singleton nobody minted cannot be carrying a patch.
///
/// The root leg is not reachable by checking the family leg first and
/// returning early on a miss: a program that patches
/// `Object.prototype` and never touches `Array.prototype` leaves the
/// family singleton unminted, so the family peek answers NULL and an
/// early return would skip the root entirely — which is exactly the
/// shape that was broken (`Object.prototype.mm = …; arr.mm()`).
///
/// An own entry storing `undefined` is NOT an absence —
/// `Map.prototype.get = undefined` shadows the surface with a real,
/// uncallable entry — so the membership probe decides that case on
/// both legs.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn patch_slot_chain(fam: i64, key: *const c_void) -> Option<(i64, i64)> {
    if let Some(hit) = unsafe { peek_own(fam, key) } {
        return Some(hit);
    }
    if fam == OBJECT_PROTO_TAG {
        return None;
    }
    unsafe { peek_own(OBJECT_PROTO_TAG, key) }
}

/// One level: the builtin-prototype singleton for `tag`, peeked, and
/// its own entry for `key`.
///
/// # Safety
/// `key` is a live Str cell.
unsafe fn peek_own(tag: i64, key: *const c_void) -> Option<(i64, i64)> {
    let proto = unsafe { torajs_rc::builtin_proto::__torajs_peek_builtin_prototype(tag) };
    if proto.is_null() {
        return None;
    }
    let (t, v) = unsafe { crate::method_support_proto::proto_own_probe(proto, key) };
    if t == 5 && !unsafe { crate::method_support_proto::proto_own_has(proto, key) } {
        return None;
    }
    Some((t, v))
}
