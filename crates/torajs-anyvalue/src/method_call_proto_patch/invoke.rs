//! The invoke half of the builtin-prototype patch consult — `Get(O,
//! P)` on the prototype singleton and, for the call sites, the
//! [[Call]] that follows.
//!
//! Split from the parent at the 500-line limit, along the seam the
//! two halves already had: the parent decides WHETHER the walk may
//! consult a patch at all (which receivers may be pre-gated, whether
//! an own face outranks the prototype, whether any prototype still
//! supplies the name), and this decides what the consult FOUND. The
//! entry probe itself (`proto_patch_slot`) stays with the parent
//! because both halves ask it.

use core::ffi::c_void;

use super::{
    __torajs_accessor_invoke_getter, __torajs_throw_check, __torajs_value_drop_heap,
    ANY_ACCESSOR_TAG, ANY_HEAP, proto_patch_slot,
};
use crate::method_call::{closure_cell_entry, invoke_with_this, not_callable};
use crate::method_value::{builtin_method_family, builtin_method_mid, recv_proto_family};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};

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
        // Putting the ORIGINAL back is a restore, not a patch:
        // `delete Map.prototype.get; Map.prototype.get = orig` names
        // the native arm, so the answer is "there is no patch here"
        // and the caller's own arm is what runs. Reading it as a
        // patch sent it down the borrowed-cell lane below, which only
        // the String family implements — every other family then
        // answered "not a function" for a method standing right
        // there, and the delete tombstone this write was undoing got
        // the last word. Map / String / Array all did it.
        if builtin_method_mid(cell) == Some(mid)
            && builtin_method_family(cell)
                == crate::method_value::family::intern_family(recv_proto_family(recv), mid)
        {
            return None;
        }
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
