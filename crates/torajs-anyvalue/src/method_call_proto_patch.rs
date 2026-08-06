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

use crate::method_call::{closure_cell_entry, invoke_with_this, not_callable};
use crate::method_value::{
    STR_PROTO_FAMILY, builtin_method_family, builtin_method_mid, recv_proto_family,
};
use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str,
};

unsafe extern "C" {
    /// torajs-dynobj — invoke an accessor pair's getter face against
    /// a receiver (owned AnyValue return; a throw inside records
    /// pending).
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    /// torajs-throw — pending-throw probe (non-consuming).
    fn __torajs_throw_check() -> i64;
    /// Universal NaN-box-safe heap dropper (getter answer release).
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of `method_support_proto.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: i64 = 6;

/// `ANY_HEAP` slot tag (torajs-dynobj `layout.rs` mirror) — the one
/// tag whose value channel carries a pointer. Every other tag is an
/// immediate riding in the same 64 bits.
const ANY_HEAP: i64 = 4;

/// Primitive fast-arm pre-gate (RFC 20260721 刀 11 G13) — the
/// short-str / bool / num arms and the heap-Str cell arm answer
/// their mids natively, so a monkey-patch installed on the
/// receiver's builtin prototype (data or accessor shape) must
/// consult BEFORE they run. The (tag, mid) patch bitmap keeps the
/// no-patch program at one relaxed load per primitive method call.
///
/// The §20.1.3.5 leg: bool / num have no own `toLocaleString` — the
/// inherited `Object.prototype.toLocaleString` is `Invoke(this,
/// "toString")`, so their toLocaleString call also consults a
/// TO_STRING patch (String has its own §22.1.3.26 and skips the leg).
pub(crate) unsafe fn primitive_patch_pregate(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    let fam = if is_short_str(recv) {
        STR_PROTO_FAMILY
    } else if is_bool(recv) {
        4
    } else if is_int32(recv) || is_double(recv) {
        0
    } else if is_cell(recv)
        && unsafe {
            (as_void_ptr(recv).cast::<u8>().add(4) as *const u16).read() == Tag::Str as u16
        }
    {
        STR_PROTO_FAMILY
    } else {
        return None;
    };
    unsafe {
        if torajs_rc::builtin_proto::__torajs_builtin_proto_has_patch(fam, mid) != 0
            && let Some(out) = builtin_proto_patch_method(recv, mid, name_str, argv, argc)
        {
            return Some(out);
        }
        if mid == ANY_METHOD_TO_LOCALE_STRING
            && fam != STR_PROTO_FAMILY
            && torajs_rc::builtin_proto::__torajs_builtin_proto_has_patch(fam, ANY_METHOD_TO_STRING)
                != 0
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
        // Peek only — a singleton nobody minted cannot carry a
        // patch, and the miss exit must stay alloc-free.
        let proto = torajs_rc::builtin_proto::__torajs_peek_builtin_prototype(fam);
        if proto.is_null() {
            return None;
        }
        // A known-mid call site carries no name bytes — mint the
        // canonical name from the meta row for the own probe (cold
        // path; the patch table is a monkey-patch surface).
        let mut minted: *mut u8 = core::ptr::null_mut();
        let key: *const u8 = if !name_str.is_null() {
            name_str
        } else if let Some((nm, _)) = torajs_rc::any_method_meta(mid) {
            let s = crate::__torajs_str_alloc_pooled(nm.len() as u64);
            core::ptr::copy_nonoverlapping(nm.as_ptr(), s.add(16), nm.len());
            minted = s;
            s
        } else {
            return None;
        };
        let (tag, value) =
            crate::method_support_proto::proto_own_probe(proto, key as *const c_void);
        if !minted.is_null() {
            crate::__torajs_str_drop(minted as *mut c_void);
        }
        if tag == 5 { None } else { Some((tag, value)) }
    }
}

/// Resolve the receiver's builtin-prototype patch for `mid` to the
/// callee it names, WITHOUT calling it — the `Get(O, P)` half of a
/// method call, split off for the callers that must perform it once
/// and call the answer many times (§24.1.1.1 step 7.a resolves the
/// collection adder once, step 9.f calls it per item).
///
/// `None` is "no patch" — the native arm is the callee. `Some` is an
/// OWNED value that may or may not be callable; the caller decides,
/// because what a non-callable one means differs per site. An
/// accessor-shaped patch runs its getter here, which is exactly the
/// one `Get` the spec asks for.
pub(crate) unsafe fn resolve_proto_patch(recv: AnyValue, mid: i64) -> Option<AnyValue> {
    unsafe {
        let (tag, value) = proto_patch_slot(recv, mid, core::ptr::null())?;
        if tag == ANY_ACCESSOR_TAG {
            // §10.1.9.2 step 3 — the getter runs with the ORIGINAL
            // receiver as `this`, and its answer is the callee. A
            // throw inside leaves the pending record for the caller.
            return Some(__torajs_accessor_invoke_getter(
                value as *const c_void,
                recv,
            ));
        }
        if tag != ANY_HEAP {
            // An immediate patch (`Map.prototype.set = null`) — a
            // real answer, just not a callable one.
            return Some(crate::nanbox_encode::__torajs_anyv_box_from_pair(
                tag, value,
            ));
        }
        crate::payload_rc_inc(tag, value);
        Some(crate::nanbox_encode::__torajs_anyv_box_from_pair(
            tag, value,
        ))
    }
}

/// Probe the receiver's builtin prototype singleton for a live own
/// entry under the method name — `Some(result)` when a patch
/// resolved (invoked / coerced / not-callable throw), `None` when
/// there is no patch and the caller keeps its miss exit.
pub(crate) unsafe fn builtin_proto_patch_method(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let (tag, value) = proto_patch_slot(recv, mid, name_str)?;
        // Accessor-shaped patch (`defineProperty(proto, m, {get})`) —
        // §10.1.9.2 step 3: the getter runs with the ORIGINAL
        // receiver as `this`, and its answer is the callee.
        if tag == ANY_ACCESSOR_TAG {
            let f = __torajs_accessor_invoke_getter(value as *const c_void, recv);
            if __torajs_throw_check() != 0 {
                if is_cell(f) {
                    __torajs_value_drop_heap(as_void_ptr(f));
                }
                return Some(VALUE_UNDEFINED);
            }
            if is_cell(f) {
                let fptr = as_void_ptr(f);
                if let Some((env, entry)) = closure_cell_entry(fptr) {
                    let out = invoke_with_this(env, entry, recv, argv, argc);
                    __torajs_value_drop_heap(fptr);
                    return Some(out);
                }
                __torajs_value_drop_heap(fptr);
            }
            return Some(not_callable());
        }
        // §13.3.6.1 step 5 — a patch that resolved to something that
        // is not an object is simply not callable, and saying so is
        // the whole of what this branch may do with it. The probe's
        // value channel only carries a pointer under `ANY_HEAP`;
        // under every other tag it is an immediate riding in the
        // same 64 bits, so reading it as a cell (`Map.prototype.set
        // = null`, `String.prototype.toUpperCase = 42`) walked into
        // whatever those bits addressed and took the process down
        // with it. The accessor branch above already guards its own
        // callee this way; this one did not.
        if tag != ANY_HEAP {
            return Some(not_callable());
        }
        let cell = value as *mut c_void;
        // A borrowed builtin cell (`Number.prototype.split =
        // String.prototype.split`) — String-family cells run the
        // §22.1.3 generic ToString(this) coerce; any other family
        // stays on the miss exit (see module doc).
        if let Some(patch_mid) = builtin_method_mid(cell) {
            let fam = builtin_method_family(cell);
            return crate::method_call_closure::generic_builtin_this(
                patch_mid, recv, argv, argc, fam,
            );
        }
        if let Some((env, entry)) = closure_cell_entry(cell) {
            return Some(invoke_with_this(env, entry, recv, argv, argc));
        }
        Some(not_callable())
    }
}
