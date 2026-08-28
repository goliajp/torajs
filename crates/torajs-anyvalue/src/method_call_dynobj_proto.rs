//! Inherited `Object.prototype` surface for plain-object receivers —
//! shared tail split from `method_call_dynobj.rs` (rotation 119
//! chunk 10, file-size decomp: parent 500 → ~410, back to soft-warn
//! safe zone).
//!
//! Both `dynobj_method` (Tag::DynObj probe) and `struct_method`
//! (Tag::Obj static-layout probe) fall through here after the
//! own-property probe misses, so a user monkey-patch always wins.
//!
//! - `object_proto_fallback` — the shared prototype dispatch:
//!   builtin-proto primitive fast path, `valueOf` (§20.1.4.7),
//!   `toString` (§20.1.3.6 / Error §20.5.3.4 / badge cell),
//!   `toLocaleString` (§20.1.4.6 re-dispatch as toString).
//! - `builtin_proto_primitive` — Number.prototype / String.prototype
//!   / Boolean.prototype ARE wrapper objects carrying a spec initial
//!   value (§21.1.3 [[NumberData]] = +0 / §22.1.3 [[StringData]] =
//!   "" / §20.3.3 [[BooleanData]] = false), so a direct method call
//!   re-dispatches into the matching primitive arm with that value
//!   as the receiver (RFC 20260722 刀 5) — the whole family
//!   (`Number.prototype.toFixed(0)`, `String.prototype.charAt(0)`,
//!   …) answers wrapper semantics, not just toString/valueOf.

use core::ffi::c_void;

use torajs_rc::{
    __torajs_rc_inc, ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF,
};

use crate::method_call::not_callable;
use crate::method_call_dynobj::{dynobj_method, struct_method};
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// Inherited `Object.prototype` surface for plain-object receivers —
/// runs only AFTER the own-property probe missed, so user
/// monkey-patches always win. `valueOf` (§20.1.4.7) answers the
/// receiver itself (fresh +1 per the boxed-value convention);
/// `toString` (§20.1.3.6) answers the "[object Object]" text;
/// `toLocaleString` (§20.1.4.6 "invoke this.toString") re-dispatches
/// as a toString call so a user own `toString` wins there too.
///
/// # Safety
/// Callers hold a live receiver ptr matching the `is_struct` flag
/// (Tag::Obj vs Tag::DynObj) and a valid `argv`.
pub(crate) unsafe fn object_proto_fallback(
    obj: *mut c_void,
    mid: i64,
    name_str: *const u8,
    is_struct: bool,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // Everything below is %Object.prototype%'s surface, so a
        // receiver whose chain never reaches it inherits none of it:
        // `Object.create(null).toString()` is a TypeError, not the
        // badge. The READ channel has answered this correctly since
        // `dynobj_proto_pair` learned the null-proto flag; the call
        // channel never asked, so `typeof o.toString` was undefined
        // while `o.toString()` answered "[object Object]" — the two
        // faces of one name disagreeing again.
        //
        // Only a NAMED call asks it, because only a named call is a
        // lookup. A NULL name means the reified cell was already
        // resolved and is now being run — `Object.prototype
        // .hasOwnProperty.call(Object.create(null), "x")` is a
        // perfectly ordinary thing to write, and the chain of the
        // thisArg has nothing to do with it. Asking there took out
        // every `groups` object (§22.2.7.2) and
        // `Array.prototype[Symbol.unscopables]` (§23.1.3.35) the
        // test262 property helpers touch.
        if !is_struct && !name_str.is_null() && !chain_reaches_object_proto(obj) {
            return not_callable();
        }
        // ES §21.1.3 / §20.3.3 / §22.1.3 — `Number.prototype` IS a Number
        // object ([[NumberData]] = +0), `Boolean.prototype` a Boolean one
        // (false), `String.prototype` a String one (""). tr builds every
        // builtin prototype as an empty dynobj, so `Number.prototype
        // .toString()` fell through to here and answered "[object Object]"
        // where the spec says "0" — which is the FIRST assertion of most
        // Number/prototype/toString cases, so the whole family died on it.
        // Ordered after the own probe like the rest of this fallback: a
        // monkey-patched `Number.prototype.toString` still wins. A
        // `delete String.prototype.toString` tombstone (RFC 20260721
        // G9) retires the primitive-identity face too — the call then
        // inherits the §20.1.3.6 badge below ("[object String]").
        if !is_struct
            && !proto_family_mid_deleted(obj, mid)
            && let Some(v) = builtin_proto_primitive(obj, mid, argv, argc)
        {
            return v;
        }
        // 521-06 — what the PROGRAM put on %Object.prototype%, ahead
        // of the spec-given surface below. The three faces this
        // function serves (valueOf / toString / toLocaleString) are
        // themselves %Object.prototype% entries, so an own write to
        // the same name on the same object replaces them; answering
        // the badge for a patched `Object.prototype.toString` was
        // reading the receiver's prototype and its patch as two
        // different objects.
        //
        // The other lanes reach this consult from the dispatcher's
        // tail (`any_method_call_inner`), which the dynobj and struct
        // arms never fall out of: both claim their receiver and end
        // here instead (521-05 left them for exactly this reason).
        // `recv_proto_family` answers %Object.prototype% for Tag::Obj
        // and Tag::DynObj alike, so the same consult serves both.
        //
        // No cycle: the arms above already probed this receiver's own
        // face, and a receiver that IS %Object.prototype% would have
        // answered there — the peek below reads the same slot.
        if let Some(out) = crate::method_call_proto_patch::builtin_proto_patch_method(
            __torajs_anyv_box_pointer(obj),
            mid,
            name_str,
            argv,
            argc,
        ) {
            return out;
        }
        // 521-07 call side — everything from here down is
        // `%Object.prototype%`'s own surface, answered natively, so a
        // `delete Object.prototype.<m>` has to end the walk in a miss
        // rather than in the native answer. The Error branch inside
        // the toString arm below is `Error.prototype`'s and is asked
        // first, because the root has nothing to give up there.
        let root_gone = crate::method_call_object_proto::root_gave_up(mid);
        if mid == ANY_METHOD_VALUE_OF && !root_gone {
            __torajs_rc_inc(obj);
            return __torajs_anyv_box_pointer(obj);
        }
        if mid == ANY_METHOD_TO_LOCALE_STRING && !root_gone {
            let key = __torajs_str_alloc(b"toString".as_ptr(), 8);
            let out = if is_struct {
                struct_method(obj, ANY_METHOD_TO_STRING, key as *const u8, argv, 0)
            } else {
                dynobj_method(
                    obj,
                    ANY_METHOD_TO_STRING,
                    key as *const u8,
                    core::ptr::null_mut(),
                    argv,
                    0,
                )
            };
            __torajs_str_drop(key as *mut c_void);
            return out;
        }
        if mid == ANY_METHOD_TO_STRING {
            // Error.prototype.toString (§20.5.3.4) — a FLAG_ERROR
            // struct answers `name: message` (matching the SSA
            // typed-tier lowering), not the "[object Object]" badge.
            // After the own-property probe, so a monkey-patched own
            // `toString` still wins.
            if is_struct
                && let Some(v) = crate::method_call_error_tostring::error_struct_tostring(obj)
            {
                return v;
            }
            // §20.1.3.6 through the badge classifier rather than a
            // hardcoded "[object Object]": the five container
            // prototypes carry a well-known `Symbol.toStringTag`, so
            // `Map.prototype.toString()` is "[object Map]".
            if !root_gone {
                return crate::method_call_object_proto::cell_badge_string(obj, is_struct);
            }
        }
        // The end of THIS walk: the dynobj and struct arms claim
        // their receiver and never float the no-such sentinel the
        // dispatcher's exits answer these at, so %Object.prototype%'s
        // own three are answered here too.
        if !root_gone
            && let Some(v) = crate::method_call_object_proto::object_proto_universal(
                __torajs_anyv_box_pointer(obj),
                mid,
                argv,
                argc,
            )
        {
            return v;
        }
        not_callable()
    }
}

/// True iff the receiver is a builtin prototype singleton whose
/// family method under `mid` has been `delete`d (the torajs-rc
/// deleted-mid tombstone) — the caller then skips the
/// primitive-identity face and inherits the ordinary
/// `Object.prototype` surface.
unsafe fn proto_family_mid_deleted(obj: *mut c_void, mid: i64) -> bool {
    let ptag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(obj) };
    ptag >= 0
        && unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(ptag, mid) } != 0
}

/// Wrapper semantics for a direct method call on a primitive-wrapper
/// prototype singleton (RFC 20260722 刀 5) — the spec initial value
/// the prototype carries as its internal slot (§21.1.3 +0 / §22.1.3
/// "" / §20.3.3 false) re-dispatches as the receiver into the
/// matching primitive arm, so every family mid (`toFixed`,
/// `toExponential`, `charAt`, …) answers exactly what the same call
/// on the bare initial value would. `None` for every other receiver
/// (and for the prototypes with no primitive data —
/// `Object.prototype`, `Map.prototype`, ... — which keep the
/// ordinary Object.prototype surface) AND for a family-arm mid miss,
/// so the caller falls through unchanged (`Number.prototype
/// .getDate()` keeps the honest TypeError).
///
/// The re-dispatch entry skips the wrapper-expando / patch consults:
/// a monkey-patch lives as an own dynobj entry on this very
/// prototype, so the own probe upstream already resolved it — by the
/// time this runs, own-property resolution is over.
///
/// `Array.prototype` is deliberately absent: the spec makes it an ARRAY
/// (an empty one), not a primitive wrapper, so `Array.prototype
/// .toString()` answering "" has to come from the array lane, not here.
unsafe fn builtin_proto_primitive(
    obj: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    // Tags are ssa_lower's, fixed by `torajs_rc::builtin_proto`:
    // Number=0, String=3, Boolean=4.
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(obj) };
    // Only what the family OWNS re-dispatches on the initial value.
    // An INHERITED name has to answer on the prototype OBJECT:
    // §20.1.4.3's ToObject(this) is `Number.prototype` itself, not
    // its [[NumberData]] +0, so `Number.prototype.hasOwnProperty(
    // "toFixed")` asks about the prototype and not about zero. This
    // used to be true by accident — the universal probes dispatched
    // ahead of the walk and never got here — and reading it off the
    // re-dispatch's own miss is not enough, because the probes have
    // an answer for every receiver, including the wrong one.
    if !crate::method_support_proto::proto_tag_family_owns(tag, mid) {
        return None;
    }
    let recv: AnyValue = match tag {
        0 => crate::nanbox_encode::__torajs_anyv_box_f64(0.0),
        // "" always fits the short-str immediate encoding.
        3 => crate::nanbox::try_box_short_str(b"")?,
        4 => crate::nanbox_encode::__torajs_anyv_box_bool(0),
        _ => return None,
    };
    let out = unsafe { crate::method_call::any_method_redispatch(recv, mid, argv, argc) };
    if out == crate::method_call::ANY_METHOD_NO_SUCH {
        return None;
    }
    Some(out)
}

/// Walks the ordinary [[Prototype]] chain and answers whether
/// %Object.prototype% is on it.
///
/// `dynobj_proto_pair` is the same answer a dynamic `__proto__` read
/// gives, so the three cases it distinguishes are the three that
/// matter: an explicit null proto ends the chain, the chain root
/// answers Null for its own parent (and IS the thing being asked
/// about, so that is a yes), and everything else hands back a parent
/// to step to — implicitly the root when a dynobj carries no user
/// one.
///
/// A non-dynobj parent (`Object.create([])`, `Object.create(
/// C.prototype)`) answers yes without stepping: those shapes have no
/// null-proto spelling of their own, so their families all end at the
/// root. The depth cap is for a chain that should not exist at all —
/// answering yes there keeps the pre-existing behaviour rather than
/// inventing a new refusal.
///
/// # Safety
/// `obj` is a live `Tag::DynObj` heap pointer.
unsafe fn chain_reaches_object_proto(obj: *mut c_void) -> bool {
    let mut cur = obj as *const c_void;
    for _ in 0..64 {
        let (tag, val) = unsafe { crate::member_get_own::dynobj_proto_pair(cur) };
        if tag == torajs_rc::AnySlotTag::Undef as u64 {
            return false;
        }
        if tag != torajs_rc::AnySlotTag::Heap as u64 {
            return true;
        }
        let parent = val as *const c_void;
        // SAFETY: a Heap proto pair carries a live cell pointer.
        let ptag = unsafe { parent.cast::<u8>().add(4).cast::<u16>().read() };
        if ptag != torajs_rc::Tag::DynObj as u16 {
            return true;
        }
        cur = parent;
    }
    true
}
