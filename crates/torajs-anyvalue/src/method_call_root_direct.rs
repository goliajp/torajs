//! The direct [[Call]] of a borrowed root toString / valueOf — RFC
//! 20260922-to-primitive-get-call slice S3.
//!
//! A builtin method read off a prototype is a mid cell with no body:
//! calling it re-dispatches the mid on `this`, through the family
//! arms. For the four cells an ordinary object, array or function
//! inherits as its `toString` / `valueOf` — Object.prototype's two,
//! Array.prototype.toString and Function.prototype.toString — the
//! answer does not depend on anything the arm knows beyond the
//! receiver's shape, so the call goes straight to the kernel here.
//! That makes the edge from a coercion to these answers a direct
//! symbol reference instead of a trip through the dispatch seam.
//!
//! The rows come from `method_value::family::intern_family`'s shape
//! (which cells the four object-world receivers inherit), not from a
//! list of kernels: a new kernel needs no row. Every other cell keeps
//! the re-dispatch.

use torajs_rc::builtin_proto::OBJECT_PROTO_TAG;
use torajs_rc::{ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF, Tag};

use crate::method_value::FN_PROTO_FAMILY;
use crate::method_value::family::ARR_PROTO_FAMILY;
use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

/// Object.prototype's family row.
const OBJ_PROTO_FAMILY: i64 = OBJECT_PROTO_TAG as i64;

/// The answer of calling root cell `(fam, mid)` with `this_arg`, or
/// `None` when the call must re-dispatch (not one of the four cells,
/// or a receiver outside the object world — a primitive `this` goes
/// through ToObject in the family's own arm).
///
/// # Safety
/// A cell payload of `this_arg` is live.
pub(crate) unsafe fn root_owned_direct(mid: i64, fam: i64, this_arg: AnyValue) -> Option<AnyValue> {
    if !is_cell(this_arg) {
        return None;
    }
    let ptr = as_void_ptr(this_arg);
    // SAFETY: a cell box points at a live heap header (tag at +4).
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    let object_world = tag == Tag::DynObj as u16
        || tag == Tag::Obj as u16
        || tag == Tag::Arr as u16
        || tag == Tag::Closure as u16;
    if !object_world {
        return None;
    }
    match (fam, mid) {
        // §20.1.3.7 — ToObject of an object is itself.
        (OBJ_PROTO_FAMILY, ANY_METHOD_VALUE_OF) => {
            unsafe { torajs_rc::__torajs_rc_inc(ptr) };
            Some(this_arg)
        }
        // §20.1.3.6 — the badge.
        (OBJ_PROTO_FAMILY, ANY_METHOD_TO_STRING) => {
            Some(unsafe { crate::method_call_object_proto::object_proto_to_string(this_arg) })
        }
        // §23.1.3.36 — Get(join) and call it.
        (ARR_PROTO_FAMILY, ANY_METHOD_TO_STRING) if tag == Tag::Arr as u16 => {
            Some(unsafe { crate::method_call_arr_copy::arr_to_string_direct(ptr) })
        }
        // §20.2.3.5 — the function's source text or native form.
        (FN_PROTO_FAMILY, ANY_METHOD_TO_STRING) if tag == Tag::Closure as u16 => {
            Some(unsafe { crate::method_call_closure::closure_to_string_direct(ptr) })
        }
        _ => None,
    }
}
