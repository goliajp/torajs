//! 403-03 — the Any → callable-cell coercion for fn-typed RETURN
//! boundaries (`function take(f: any): (a: number) => number {
//! return f }`): `effective_ret_ty` upgrades the declared FnSig to
//! Closure when the body returns an `any` binding, and the return
//! site funnels the box through this kernel instead of handing its
//! raw bits over as a pointer.
//!
//! A Closure-tagged cell transfers the box's +1 stake to the
//! returned pointer verbatim — a cell box IS the raw pointer, made
//! explicit here rather than relied on as a bit-pattern coincidence.
//! Anything else (a number, a string, a non-callable object) answers
//! the immortal undefined sentinel, the find-miss shape: the call
//! site's undefable-heap guard turns the later CALL into a catchable
//! TypeError — the right phase (§13.3.6 the throw fires at call
//! time; returning the value itself never throws). The non-callable
//! box's own stake settles here.

use torajs_rc::{HeapHeader, Tag};

use crate::__torajs_anyv_rc_dec;
use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

/// C-ABI face for `coerce_to_ret`'s Any→Closure arm.
///
/// # Safety
///
/// `v` is a live Any box; a cell payload points at a valid heap
/// block whose header is readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_callable_cell(v: AnyValue) -> *mut u8 {
    if is_cell(v) {
        let p = as_void_ptr(v) as *mut u8;
        if !p.is_null() {
            // SAFETY: caller invariant — a cell box's payload is a
            // live heap block headed by HeapHeader.
            let hdr = unsafe { &*(p as *const HeapHeader) };
            if hdr.type_tag == Tag::Closure as u16 {
                return p;
            }
        }
    }
    // Not callable: settle the box's stake (no-op for inline
    // payloads), answer the sentinel.
    unsafe { __torajs_anyv_rc_dec(v) };
    torajs_rc::undef_cell::undef_cell_ptr()
}
