//! `__torajs_obj_drop_rc` — release-one-reference drop for a
//! `Tag::Obj` block reached through the runtime dispatch
//! (`__torajs_value_drop_heap`), L3b #4.
//!
//! Mirrors the lower-emitted typed drop
//! (`torajs-core::ssa_lower_emit_drop_value::emit_drop_obj`) using the
//! codegen-emitted `__torajs_class_layouts` table instead of the
//! compile-time struct layout:
//!
//! - rc-dec; while other refs remain, a **named-class** instance
//!   registers as a potential cycle root (`cycle_buffer` — the typed
//!   path's T-26.C). Anonymous structs are excluded on both paths:
//!   the typed anon drop skips the buffer scrub for speed, so a
//!   runtime-buffered anon struct would leave a dangling buffer
//!   entry when the typed side frees it.
//! - on hit-zero: named-class instances clear their WeakRefs
//!   (T-26), then every layout `child_offsets` slot releases one
//!   reference through `value_drop_heap` (NaN-box immediates in
//!   `any`-typed fields are filtered by its cell gate), then the
//!   +24 props dynobj (fixed offset — released for anonymous
//!   structs too), then `free` — unless the block sits in the cycle
//!   root buffer, where it stays allocated, coloured black, for the
//!   next collect's root walk to free (a buffer entry leaves in O(1)).
//!   The free is sized from the layout row (`cell_free`).
//!
//! Every `Tag::Obj` cell carries a class tag with a layout row — the
//! compiler stamps each literal out of the anon pool, the runtime's
//! only `Tag::Obj` mint (an `allSettled` record) builds a dynobj where
//! the call site named no tag — so past the hit-zero test the layout
//! walk is unconditional.

use core::ffi::c_void;

use crate::buffer::__torajs_cycle_buffer;
use crate::cell_free::free_class_cell;
use crate::layout::{
    CLASS_LAYOUT_FLAG_NAMED, COLOR_BLACK, FLAG_BUFFERED, HeapHeader, OBJ_PROPS_OFF, is_class_obj,
    layout_for_class_obj, set_color,
};

unsafe extern "C" {
    /// torajs-rc refcount decrement — returns 1 on hit-zero (Free),
    /// 0 while refs remain / for static literals (Keep).
    fn __torajs_rc_dec(p: *mut c_void) -> i32;

    /// Universal drop dispatch (torajs-value-drop) — releases one
    /// reference per child slot, tag-routed.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Release one reference on a `Tag::Obj` block. NULL is a no-op.
///
/// # Safety
/// `p` is NULL or a live heap block with the universal `HeapHeader`
/// and, for class instances, the `class_tag` u32 at +8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_drop_rc(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } == 0 {
        // Still referenced — named-class instances are potential
        // cycle roots (typed path buffers here too). A static literal
        // is never one, and its dec is a no-op. Asked only here, so a
        // release that frees the block does not pay for the question.
        let named = unsafe { is_class_obj(p) }
            && unsafe { (*layout_for_class_obj(p)).flags } & CLASS_LAYOUT_FLAG_NAMED != 0;
        if named {
            unsafe { __torajs_cycle_buffer(p) };
        }
        return;
    }
    // weak observers were already told by dec_ref on the way to zero
    let lay = unsafe { layout_for_class_obj(p) };
    let n = unsafe { (*lay).n_children } as usize;
    for i in 0..n {
        let off = unsafe { *(*lay).child_offsets.add(i) } as usize;
        let child = unsafe { *((p as *mut u8).add(off) as *mut *mut c_void) };
        unsafe { __torajs_value_drop_heap(child) };
    }
    // +24 expando props dict (RFC 20260714-struct-dynamic-props
    // blade 1), a fixed slot outside the layout. NULL-safe kernel.
    let props = unsafe { *((p as *mut u8).add(OBJ_PROPS_OFF) as *mut *mut c_void) };
    unsafe { __torajs_value_drop_heap(props) };
    let h = p as *mut HeapHeader;
    if unsafe { (*h).flags } & FLAG_BUFFERED != 0 {
        unsafe { set_color(h, COLOR_BLACK) };
    } else {
        unsafe { free_class_cell(p) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real per-tag walk verification happens via the conformance
    // gate (the class_layouts table only exists in `tr build`
    // binaries); the unit-testable contract is the NULL no-op.
    #[test]
    fn null_input_is_no_op() {
        unsafe { __torajs_obj_drop_rc(core::ptr::null_mut()) };
    }
}
