//! What `%Object.prototype%`'s OWN method answers, for a walk that
//! actually reached the root because a FAMILY prototype gave the name
//! up (`delete Array.prototype.toString`).
//!
//! The sibling `method_call_object_proto.rs` answers the root's
//! surface at the END of a walk nobody claimed — a Map has no
//! `toString` of its own, so the badge is simply what is left. This
//! file answers the other arrival: the family DID own the name, the
//! program deleted it, and the walk continues past the tombstone. The
//! per-tag arm still knows how to answer that mid natively and would
//! do so, which is the whole bug — `[1,2].toString()` kept saying
//! `1,2` where `[object Array]` is due, `(5).valueOf()` kept saying 5
//! where a wrapper object is due, and `(1234567).toLocaleString()`
//! kept grouping digits where §20.1.3.5's plain `toString` is due.
//!
//! Only the three names a family can BOTH own and share with the root
//! can arrive here. The universal probes (`hasOwnProperty` /
//! `propertyIsEnumerable` / `isPrototypeOf`) and the Annex B legacy
//! accessor four are the root's alone, so no family tombstone can
//! redirect them — they are routed anyway rather than special-cased
//! out, because "what does the root's own method answer" has one
//! answer per mid regardless of who asks.

use torajs_rc::{ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF};

use crate::method_call::{ANY_METHOD_NO_SUCH, not_callable};
use crate::nanbox::AnyValue;

/// `%Object.prototype%`'s own answer for `mid` against `recv`, or
/// `None` when the root owns no method under that mid and the caller
/// keeps its own exit.
///
/// # Safety
/// Same contract as the dispatcher: `recv` is a live borrow and
/// `argv` holds `argc` live slots.
pub(crate) unsafe fn root_own_answer(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        // §20.1.3.6 — the "[object X]" badge classifier.
        if mid == ANY_METHOD_TO_STRING {
            return Some(crate::method_call_object_proto::object_proto_to_string(
                recv,
            ));
        }
        // §20.1.3.7 — ToObject(this). Identity on a cell receiver, a
        // freshly minted wrapper on a primitive, which is why
        // `typeof (5).valueOf()` turns from "number" into "object"
        // the moment `Number.prototype.valueOf` is gone.
        if mid == ANY_METHOD_VALUE_OF {
            return Some(crate::to_object::__torajs_any_to_object(recv));
        }
        // §20.1.3.5 step 2 — `Invoke(this, "toString")`, an ORDINARY
        // lookup that takes no arguments. It has to re-enter the
        // dispatcher rather than call any particular kernel: the
        // receiver's own face, a subclass method, a prototype patch
        // and a second tombstone are all live answers, and only the
        // walk knows which. `delete Number.prototype.toLocaleString`
        // alone therefore still reads "5" — the family's toString is
        // right there — and only deleting both reaches the badge.
        if mid == ANY_METHOD_TO_LOCALE_STRING {
            let out = crate::method_call::any_method_call_inner(
                recv,
                ANY_METHOD_TO_STRING,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null(),
                0,
            );
            // Nothing on the chain resolves toString either (the root
            // gave that up too) — §20.1.3.5's Invoke is a Call on a
            // non-callable, not a quiet undefined.
            if out == ANY_METHOD_NO_SUCH {
                return Some(not_callable());
            }
            return Some(out);
        }
        crate::method_call_object_proto::object_proto_universal(recv, mid, argv, argc)
    }
}
