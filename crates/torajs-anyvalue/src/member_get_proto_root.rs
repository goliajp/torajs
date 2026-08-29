//! The prototype hops every non-dynobj receiver owes after its own
//! face misses (§10.1.8.1 OrdinaryGet step 4, 517-07 / 525-04).
//!
//! The dynobj lane already has them: `member_get_own::implicit_proto_parent`
//! hands back the parent cell and the walk recurses through its full
//! [[Get]]. The other lanes have no dynobj proto pair to ask — an `Arr`
//! receiver's family prototype is itself an Arr cell, a wrapper's is a
//! tag-keyed singleton — and their walk ended at the reify probe, so a
//! property the program installed on a prototype had no path to them
//! at all: `Object.prototype.foo = 5; ([] as any).foo` answered
//! undefined while `({} as any).foo` answered 5.
//!
//! Closing that with a hop straight to the root was one link too few
//! the moment a family prototype stopped hanging directly off
//! %Object.prototype%: `Iterator.prototype.zz = 9` sits between
//! `[1].values()` and the root, and so does `Map.prototype.zz`
//! between a Map and it. The walk here is the whole chain
//! ([`proto_chain_expando`]), and the spec-given methods on each
//! singleton were already offered by the caller's reify probe — what
//! is missing is only what a program put there.

use core::ffi::c_void;

use crate::member_get_own::OBJECT_PROTO_TAG;
use crate::nanbox::AnyValue;

/// §10.1.8.1 step 4, the WHOLE way up: the receiver's own family
/// prototype, then whatever that hangs off, then the root — each
/// asked for what the program installed on it.
///
/// The two shells above hop straight to the root, which was right
/// only while every builtin prototype hung directly off
/// %Object.prototype%. §23.1.5.2 puts %ArrayIteratorPrototype% under
/// %Iterator.prototype%, so `Iterator.prototype.zz = 9` sits on a
/// singleton BETWEEN `[1].values()` and the root and the root hop
/// read straight past it — and the same was true one link up for
/// every family: `Map.prototype.zz` was unreachable from a Map for
/// the same reason, just with a shorter chain to skip.
///
/// The spec-given faces are NOT this walk's business — the caller's
/// reify probe already offered them, and it consults the patch
/// bitmap, so what is left here is only what a program put there.
/// Peeks rather than materializes: a singleton nobody minted cannot
/// be carrying a user entry.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn proto_chain_expando(recv: AnyValue, key: *const c_void) -> Option<(i64, i64)> {
    if key.is_null() {
        return None;
    }
    let fam = crate::method_value::family::recv_proto_family(recv);
    let start = if fam >= 0 {
        fam
    } else {
        OBJECT_PROTO_TAG as i64
    };
    unsafe { patch_slot_chain(start, key) }
}

/// One prototype's own entry for a METHOD name, then the chain root's
/// (§10.1.9.2, 521-05). PEEKs rather than materializes, because the
/// call channel's miss exit has to stay allocation-free and a
/// singleton nobody minted cannot be carrying a patch.
///
/// The outer legs are not reachable by checking the family leg first
/// and returning early on a miss: a program that patches
/// `Object.prototype` and never touches `Array.prototype` leaves the
/// family singleton unminted, so the family peek answers NULL and an
/// early return would skip the root entirely — which is exactly the
/// shape that was broken (`Object.prototype.mm = …; arr.mm()`).
///
/// A walk, not a pair: `proto_parent_tag` is what knows how many
/// links there are, and spelling "family, else root" inline made
/// every consumer of that fact a place it could go stale.
///
/// An own entry storing `undefined` is NOT an absence —
/// `Map.prototype.get = undefined` shadows the surface with a real,
/// uncallable entry — so the membership probe decides that case on
/// both legs.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn patch_slot_chain(fam: i64, key: *const c_void) -> Option<(i64, i64)> {
    let mut tag = fam;
    while tag >= 0 {
        if let Some(hit) = unsafe { peek_own(tag, key) } {
            return Some(hit);
        }
        tag = torajs_rc::builtin_proto::proto_parent_tag(tag);
    }
    None
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
