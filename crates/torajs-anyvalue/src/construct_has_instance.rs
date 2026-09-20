//! §7.3.22 OrdinaryHasInstance when the target arrived as a VALUE —
//! `o instanceof C` where `C` is a binding the compile-time class
//! lane could not fold, so only the running program knows what it is.
//!
//! Split out of [`crate::construct`] under the 500-line file cap.
//! Verbatim move. The seam is the question each half answers:
//! `construct` is about `new C(...)` — making an object — while these
//! four are about whether an object is already on C's prototype
//! chain. `instanceof_dynamic` routes to both and is unchanged.

use core::ffi::c_void;

use torajs_rc::{HeapHeader, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
}

/// §7.3.22 OrdinaryHasInstance(C, O) where C is a plain-fn VALUE —
/// `o instanceof C` with a fn-expr binding behind the name (no class
/// resolves, so the compile-time tag dispatch has nothing to fold).
/// O must be an Object (step 3, non-objects answer false); P is
/// C's canonical `.prototype` (the same fnprops cell the construct
/// kernel links, so `Array.from.call(C, …)` products match); the
/// walk follows [[Prototype]] links (the proto getter answers
/// borrows / immortal singletons — nothing to release) comparing
/// cell identity per step 7. A callable with no object-shaped
/// `.prototype` throws (step 5's Get + step 6's Type check).
///
/// # Safety
/// `v` is a valid AnyValue; `cell` is a live `Tag::Closure` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_instanceof_fn_value(v: u64, cell: *mut c_void) -> bool {
    unsafe {
        // §7.3.22 step 2 — a bound function has no `.prototype` of
        // its own; the question is asked of the target it was bound
        // from, and a bound-over-bound chain recurses to the bottom
        // (the same walk [`is_constructor_cell`] does).
        if let Some((1, target, _)) = crate::method_bind::bound_cell_meta(cell) {
            return __torajs_instanceof_fn_value(v, target as *mut c_void);
        }
        let Some((_ptag, pval)) = crate::closure_proto::fn_prototype_pair(cell) else {
            __torajs_throw_type_error(
                c"Function has non-object prototype in instanceof check".as_ptr(),
            );
            return false;
        };
        proto_chain_reaches(v, pval)
    }
}

/// §7.3.22 steps 3 and 6-7 — is `pval` on O's [[Prototype]] chain?
///
/// O must be an Object (step 3, non-objects answer false); the walk
/// compares cell identity. The proto getter answers an OWNED
/// reference (`share_proto`, `dynobj_entry`), so each hop is released
/// once the next one is read: keeping them leaked one reference per
/// mortal prototype per `instanceof` that walked it, which pinned the
/// constructor behind it for good.
///
/// # Safety
/// `v` is a valid AnyValue; `pval` is a heap cell address.
pub(crate) unsafe fn proto_chain_reaches(v: AnyValue, pval: u64) -> bool {
    unsafe {
        if !is_heap_object(v) {
            return false;
        }
        let mut cur = __torajs_anyv_get_proto_of_any(v);
        while cur != crate::nanbox::VALUE_NULL && cur != 0 {
            let reached = is_cell(cur) && as_void_ptr(cur) as u64 == pval;
            let next = if reached {
                0
            } else {
                __torajs_anyv_get_proto_of_any(cur)
            };
            crate::nanbox_ffi::__torajs_anyv_rc_dec(cur);
            if reached {
                return true;
            }
            cur = next;
        }
        false
    }
}

/// §7.3.22 for a target that is one of tr's CLASS objects.
///
/// A class constructor is a function object in the spec, but tr models
/// it as a dynobj carrying `FLAG_DYNOBJ_CLASS_CTOR` (the bit that
/// makes `typeof C` answer `"function"`) — so the closure-shaped walk
/// above cannot read its `.prototype`. That property is an ordinary
/// own entry on the class dynobj itself, pointing at the very object
/// every instance's [[Prototype]] links to.
///
/// `None` when the cell is not a class object, or carries no
/// object-shaped `prototype` — the caller decides which spec step
/// that is.
///
/// # Safety
/// `cell` is a live heap cell.
pub(crate) unsafe fn class_ctor_has_instance(v: AnyValue, cell: *mut c_void) -> Option<bool> {
    unsafe {
        let hdr = &*(cell as *const HeapHeader);
        if hdr.type_tag != Tag::DynObj as u16 || hdr.flags & torajs_rc::FLAG_DYNOBJ_CLASS_CTOR == 0
        {
            return None;
        }
        let (ptag, pval) = crate::closure_proto::class_prototype_pair(cell)?;
        // Tag 4 is `AnySlotTag::Heap` in the member-pair protocol;
        // anything else is not an Object and step 6 rejects it.
        if ptag != 4 {
            return None;
        }
        Some(proto_chain_reaches(v, pval))
    }
}

/// Is this completion value an Object in the spec sense — a heap cell
/// that is not one of the heap-shaped primitives (Str / Symbol /
/// BigInt)?
pub(crate) unsafe fn is_heap_object(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    let ptr = as_void_ptr(av);
    if ptr.is_null() {
        return false;
    }
    // SAFETY: a cell-encoded AnyValue points at a live heap header.
    let hdr = unsafe { &*(ptr as *const HeapHeader) };
    hdr.type_tag != Tag::Str as u16
        && hdr.type_tag != Tag::Symbol as u16
        && hdr.type_tag != Tag::BigInt as u16
}
