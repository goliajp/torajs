//! Builtin-prototype monkey-patch consult — the proto-chain step
//! for primitive / builtin-shaped receivers. When every dispatch
//! arm misses, a patch installed on the receiver's builtin
//! prototype singleton (`Number.prototype.split =
//! String.prototype.split`, `String.prototype.myFn = () => …`)
//! resolves before the final not-a-function TypeError (ES
//! §10.1.9.2 OrdinaryGet step 2 — the receiver's own faces are the
//! per-tag arms, the singleton's own entries are its prototype
//! level). RFC 20260721-string-proto-cluster 刀 3.
//!
//! Cycle posture: the consult runs only on first-level dispatch
//! (`skip_wrapper_expando == false` — a reified-builtin
//! re-dispatch is method-body execution, lookup is over), and a
//! borrowed builtin cell only proceeds through the String-family
//! generic coerce; a non-string-family borrow answers `None`
//! (recorded L3b — an array-generic lane would re-enter the
//! dispatcher and re-consult this table).

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, Tag};

use crate::method_call::not_callable;
use crate::method_value::{STR_PROTO_FAMILY, recv_proto_family};
use crate::nanbox::{AnyValue, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str};

unsafe extern "C" {
    /// torajs-dynobj — invoke an accessor pair's getter face against
    /// a receiver (owned AnyValue return; a throw inside records
    /// pending).
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    /// torajs-throw — pending-throw probe (non-consuming).
    fn __torajs_throw_check() -> i64;
    /// Universal NaN-box-safe heap dropper (getter answer release).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-arr / torajs-dynobj — own-key membership on a
    /// RECEIVER's own expando storage: the side-props table for an
    /// Arr, the props dynobj for a Closure. The pre-gate's
    /// stand-down probe.
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
}

// The invoke half — `Get` on the prototype singleton and the
// [[Call]] that follows; re-exported so the consumer face is
// unchanged by the split.
mod invoke;
pub(crate) use invoke::{builtin_proto_patch_method, resolve_proto_patch};

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of `method_support_proto.rs::ANY_ACCESSOR_TAG`.
pub(super) const ANY_ACCESSOR_TAG: i64 = 6;

/// `ANY_HEAP` slot tag (torajs-dynobj `layout.rs` mirror) — the one
/// tag whose value channel carries a pointer. Every other tag is an
/// immediate riding in the same 64 bits.
pub(super) const ANY_HEAP: i64 = 4;

/// `Object.prototype`'s builtin-proto family — the end of every
/// builtin prototype's [[Prototype]] chain, so asking whether it
/// still owns a name is the whole of "is this inherited from
/// anywhere above".
const OBJECT_PROTO_FAMILY: i64 = 1;

/// Receivers whose ONLY source for a method is their prototype —
/// they carry no own properties at all, so nothing can sit in front
/// of the prototype for the pre-gate below to shadow.
///
/// The list is what it is because of where torajs stands today, not
/// because the spec says these objects are special: an ordinary
/// property write to a Map / Set / Date / RegExp / Promise / weak
/// instance is refused (only Arr and Closure carry a side-props
/// table), and Symbol refuses it in every engine. **When a family
/// grows own properties it must come off this list**, or its
/// pre-gate consult would start shadowing them — the ordering the
/// exclusion of Arr and Closure below is protecting.
///
/// BigInt was off this list until rotation 320, and the reason is
/// worth keeping: torajs used to reach the method dispatcher for the
/// INTERNAL ToString of a bigint, where §7.1.17 step 7 converts the
/// value directly and looks up nothing. Pre-gating it therefore made
/// a patched `BigInt.prototype.toString` observable from every
/// implicit coercion — `String.prototype.isWellFormed.call(1n)` is
/// the one test262 checks explicitly — and cost a pass regression to
/// learn. What unblocked it was fixing the coercion rather than the
/// gate: `coerce.rs`'s ToString now answers a BigInt cell directly,
/// so nothing internal consults this table and only an explicit
/// `.toString()` does. Boolean and Number never routed their
/// internal coercion this way, and Symbol has no ToString conversion
/// to route at all — measured, not assumed.
fn proto_is_only_method_source(tag: u16) -> bool {
    matches!(
        tag,
        t if t == Tag::Map as u16
            || t == Tag::Set as u16
            || t == Tag::WeakMap as u16
            || t == Tag::WeakSet as u16
            || t == Tag::WeakRef as u16
            || t == Tag::Date as u16
            || t == Tag::RegExp as u16
            || t == Tag::Promise as u16
            || t == Tag::MapIter as u16
            || t == Tag::ArrIter as u16
            || t == Tag::IterHelper as u16
            || t == Tag::Symbol as u16
            || t == Tag::BigInt as u16
    )
}

/// The receivers that DO carry own properties and are pre-gated
/// anyway, by answering the ordering question instead of dodging it
/// (see [`own_face_shadows`]). Arr keeps a side-props table, Closure
/// a props dynobj; both also route a `FLAG_SUBCLASSED` class-method
/// probe below the builtin prototype.
///
/// A family joins this list rather than
/// [`proto_is_only_method_source`] the moment it grows a place to
/// put an own property — the two lists are the same question asked
/// of receivers that can and cannot.
fn tag_carries_own_face(tag: u16) -> bool {
    tag == Tag::Arr as u16 || tag == Tag::Closure as u16
}

/// Fast-arm pre-gate (RFC 20260721 刀 11 G13) — the short-str / bool
/// / num arms, the heap-Str cell arm and the per-tag collection arms
/// answer their mids natively, so a monkey-patch installed on the
/// receiver's builtin prototype (data or accessor shape) must consult
/// BEFORE they run. The (tag, mid) patch bitmap keeps the no-patch
/// program at one relaxed load per method call.
///
/// Which receivers may be pre-gated is an ORDERING question, not a
/// performance one: §10.1.8.1 resolves a receiver's own properties
/// before its prototype's, so consulting a patch early is only sound
/// where the receiver has no own face to be resolved first. That is
/// true of the primitives, and of the cell shapes
/// `proto_is_only_method_source` names, for free — none of them can
/// hold an own property at all.
///
/// Arr and Closure can, so they answer the ordering question rather
/// than being excused from it: [`own_face_shadows`] asks whether the
/// receiver's own face — its expando entry, or a subclass method,
/// which is the other link the spec chain puts below the builtin
/// prototype — would resolve this name, and the pre-gate stands down
/// where it would. `a.push = f` keeps beating `Array.prototype.push
/// = g` because the receiver is asked first, not because the patch
/// is never consulted.
///
/// The §20.1.3.5 leg: a family with no own `toLocaleString` inherits
/// `Object.prototype`'s, which is `Invoke(this, "toString")` — so its
/// toLocaleString call must consult a TO_STRING patch too. Only the
/// families that redefine the property skip the leg, and which ones
/// those are is a question `proto_tag_owns` answers. Standing in for
/// it with "everyone except String" had all three interesting
/// families backwards: `String.prototype` owns no toLocaleString (it
/// was skipping the leg it needed), while `Number.prototype` and
/// `Date.prototype` own theirs and were taking a leg that must not
/// run — a patched `Number.prototype.toString` showed through
/// `(5).toLocaleString()`, which no engine does.
pub(crate) unsafe fn primitive_patch_pregate(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    // Set for the receivers that carry an own property face, so the
    // consults below can ask it before answering (None for everyone
    // whose prototype is their only source).
    let mut recv_face: Option<(*mut c_void, u16)> = None;
    let fam = if is_short_str(recv) {
        STR_PROTO_FAMILY
    } else if is_bool(recv) {
        4
    } else if is_int32(recv) || is_double(recv) {
        0
    } else if is_cell(recv) {
        let ptr = as_void_ptr(recv);
        // SAFETY: is_cell guarantees a live header.
        let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
        if tag == Tag::Str as u16 {
            STR_PROTO_FAMILY
        } else if proto_is_only_method_source(tag) || tag_carries_own_face(tag) {
            if tag_carries_own_face(tag) {
                recv_face = Some((ptr, tag));
            }
            match recv_proto_family(recv) {
                f if f >= 0 => f,
                _ => return None,
            }
        } else {
            return None;
        }
    } else {
        return None;
    };
    unsafe {
        if (torajs_rc::builtin_proto::__torajs_builtin_proto_is_shadowed(fam, mid) != 0
            || root_shadows_inherited(fam, mid))
            && !own_face_shadows(recv_face, mid, name_str)
        {
            if let Some(out) = builtin_proto_patch_method(recv, mid, name_str, argv, argc) {
                return Some(out);
            }
            // No own entry resolved, and the method was deleted — the
            // builtin surface is gone rather than showing through.
            // `delete Map.prototype.get` leaves nothing for `m.get`
            // to find, which is what both bun and V8 answer; the
            // value-read face has consulted this same tombstone since
            // it was introduced.
            //
            // A later set / defineProperty revives without a clear
            // call, and the probe below is what makes that true. It
            // used to be left to "the own probe above runs first",
            // which only holds when the write was a user function —
            // put the ORIGINAL method back and the consult declines
            // (that is a restore, handled above), leaving the
            // tombstone to answer for an entry that is standing right
            // there. The question the tombstone may speak to is
            // "deleted AND nothing written since", so it is asked
            // that way.
            // ...unless the chain still has it. Deleting
            // `Boolean.prototype.toString` does not leave nothing —
            // the walk continues to `Object.prototype`, which owns
            // its own toString, and both bun and V8 answer the badge
            // rather than throwing. Only a method with nothing above
            // it (`delete Map.prototype.get`) is really gone.
            if torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(fam, mid) != 0
                && proto_patch_slot(recv, mid, name_str).is_none()
                && !crate::method_support_proto::proto_tag_owns(OBJECT_PROTO_FAMILY, mid)
            {
                return Some(not_callable());
            }
        }
        if mid == ANY_METHOD_TO_LOCALE_STRING
            && !crate::method_support_proto::proto_tag_owns(fam, ANY_METHOD_TO_LOCALE_STRING)
            && torajs_rc::builtin_proto::__torajs_builtin_proto_has_patch(fam, ANY_METHOD_TO_STRING)
                != 0
            // The leg's Invoke is an ordinary lookup of `toString` on
            // the receiver, so it stands down on an own one for the
            // same reason the consult above does — just under the
            // name it is about to resolve, not the one called.
            && !own_face_shadows(recv_face, ANY_METHOD_TO_STRING, core::ptr::null())
        {
            // §20.1.3.5 step 2 Invoke takes no arguments.
            return builtin_proto_patch_method(
                recv,
                ANY_METHOD_TO_STRING,
                core::ptr::null(),
                core::ptr::null(),
                0,
            );
        }
    }
    None
}

/// Does the chain ROOT carry a write under a name this family does
/// NOT own?
///
/// The bitmap the gate above reads is per (prototype, mid), so a
/// write to %Object.prototype% is invisible to every other family's
/// gate — and 521-05's hop to the root, which happens inside the slot
/// lookup, never ran because the gate had already declined. The arms
/// that answer an INHERITED name natively therefore kept answering it:
/// `Object.prototype.valueOf = f` left `[1].valueOf()` at the array
/// identity, `new Map().valueOf()` at the map, and the same write
/// under `toString` left a Map at its badge.
///
/// Ownership is the whole of the condition. `String.prototype` owns
/// its own `valueOf` (§22.1.3.35) and `Array.prototype` its own
/// `toString` (§23.1.3.36), so for those the root is not what the
/// walk reaches — and `proto_tag_owns` reads the delete tombstone, so
/// a family method the program removed stops being an answer and the
/// root's shows through.
fn root_shadows_inherited(fam: i64, mid: i64) -> bool {
    fam != OBJECT_PROTO_FAMILY
        // SAFETY: pure bitmask read, range-checked inside.
        && unsafe {
            torajs_rc::builtin_proto::__torajs_builtin_proto_is_shadowed(OBJECT_PROTO_FAMILY, mid)
        } != 0
        && !crate::method_support_proto::proto_tag_owns(fam, mid)
}

/// The Str cell to probe a property under for `mid`: the call site's
/// own name bytes when it carries them, otherwise the canonical name
/// minted from the meta row (a known-mid site carries none). `None`
/// when the mid has no name to probe under.
///
/// Answers `(key, minted)` — `minted` is NULL when the key is the
/// caller's borrow, and otherwise the caller's to drop. Cold path by
/// construction: every caller is behind the patch bitmap.
unsafe fn method_name_key(mid: i64, name_str: *const u8) -> Option<(*const u8, *mut u8)> {
    if !name_str.is_null() {
        return Some((name_str, core::ptr::null_mut()));
    }
    let (nm, _) = torajs_rc::any_method_meta(mid)?;
    unsafe {
        let s = crate::__torajs_str_alloc_pooled(nm.len() as u64);
        core::ptr::copy_nonoverlapping(nm.as_ptr(), s.add(16), nm.len());
        Some((s as *const u8, s))
    }
}

/// Would the receiver's OWN resolution answer a call under `mid`
/// before the builtin prototype is ever reached?
///
/// Only the two cell shapes that carry own properties pass a
/// `recv_face` here (`None` answers false for everyone else). Their
/// arms resolve an expando entry first, and then — behind
/// `FLAG_SUBCLASSED` — a class method, because on the spec chain
/// `C.prototype` sits between own properties and the builtin
/// prototype. A `true` is the pre-gate standing down: §10.1.8.1
/// resolves the receiver's own face first, so consulting a prototype
/// patch early would shadow it.
///
/// Membership, not the probe's tag channel, is the question — an own
/// `undefined` is still an own property, on the receiver side as much
/// as on the prototype's.
unsafe fn own_face_shadows(
    recv_face: Option<(*mut c_void, u16)>,
    mid: i64,
    name_str: *const u8,
) -> bool {
    let Some((ptr, tag)) = recv_face else {
        return false;
    };
    unsafe {
        let Some((key, minted)) = method_name_key(mid, name_str) else {
            return false;
        };
        let expando = if tag == Tag::Arr as u16 {
            __torajs_arrprops_has(ptr, key as *const c_void) != 0
        } else {
            let props = crate::member_get::closure_props(ptr);
            !props.is_null() && __torajs_dynobj_has(props, key as *const c_void) != 0
        };
        let owns = expando || {
            let flags = (ptr.cast::<u8>().add(6) as *const u16).read();
            flags & torajs_rc::FLAG_SUBCLASSED != 0
                && crate::method_call_subclass::subclass_owns(ptr, key)
        };
        if !minted.is_null() {
            crate::__torajs_str_drop(minted as *mut c_void);
        }
        owns
    }
}

/// The receiver's live builtin-prototype own entry for `mid` as the
/// probe's raw `(tag, value)` pair — `None` when the family has no
/// singleton, no entry, or no name to probe under, which is every
/// caller's "there is no patch here" exit.
///
/// The pair is a BORROW into the singleton's slot; a caller keeping
/// the value past the probe takes its own stake.
pub(crate) unsafe fn proto_patch_slot(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
) -> Option<(i64, i64)> {
    unsafe {
        let fam = recv_proto_family(recv);
        if fam < 0 {
            return None;
        }
        let (key, minted) = method_name_key(mid, name_str)?;
        let out = crate::member_get_proto_root::patch_slot_chain(fam, key as *const c_void);
        if !minted.is_null() {
            crate::__torajs_str_drop(minted as *mut c_void);
        }
        out
    }
}
