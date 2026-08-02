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
//!   structs too), then the buffer scrub + `free`.
//! - `class_tag == 0` (anonymous struct interned too late for a
//!   layout — see `torajs-structmeta`'s graceful-degradation note)
//!   has no walkable metadata: outer block frees, field refs leak
//!   (documented residue, same as the pre-L3b-#4 fallback).

use core::ffi::c_void;

use crate::buffer::{__torajs_cycle_buffer, __torajs_cycle_unbuffer};
use crate::layout::{CLASS_LAYOUT_FLAG_NAMED, OBJ_PROPS_OFF, is_class_obj, layout_for_class_obj};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat free (the Obj allocator is mmalloc
    /// via `obj_alloc`'s `___torajs_libc_malloc` tail call).
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);

    /// torajs-rc refcount decrement — returns 1 on hit-zero (Free),
    /// 0 while refs remain / for static literals (Keep).
    fn __torajs_rc_dec(p: *mut c_void) -> i32;

    /// torajs-weak — clear WeakRefs pointing at a dying target
    /// (typed path's T-26 class-instance hook).
    fn __torajs_weakref_target_dying(target: *mut c_void);

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
    let classy = unsafe { is_class_obj(p) };
    let named =
        classy && unsafe { (*layout_for_class_obj(p)).flags } & CLASS_LAYOUT_FLAG_NAMED != 0;
    if unsafe { __torajs_rc_dec(p) } == 0 {
        // Still referenced — named-class instances are potential
        // cycle roots (typed path buffers here too).
        if named {
            unsafe { __torajs_cycle_buffer(p) };
        }
        return;
    }
    if classy {
        if named {
            unsafe { __torajs_weakref_target_dying(p) };
        }
        let lay = unsafe { layout_for_class_obj(p) };
        let n = unsafe { (*lay).n_children } as usize;
        for i in 0..n {
            let off = unsafe { *(*lay).child_offsets.add(i) } as usize;
            let child = unsafe { *((p as *mut u8).add(off) as *mut *mut c_void) };
            unsafe { __torajs_value_drop_heap(child) };
        }
    }
    // +24 expando props dict (RFC 20260714-struct-dynamic-props
    // blade 1) — fixed offset, so it is released OUTSIDE the layout
    // gate: an anonymous struct (class_tag 0, no walkable layout)
    // still frees its props even though its layout fields cannot be
    // walked (that residue is documented above). NULL-safe kernel.
    let props = unsafe { *((p as *mut u8).add(OBJ_PROPS_OFF) as *mut *mut c_void) };
    unsafe { __torajs_value_drop_heap(props) };
    // Cheap no-op when FLAG_BUFFERED is clear; scrubs a buffered
    // class Obj so the root buffer never holds a freed pointer.
    unsafe { __torajs_cycle_unbuffer(p) };
    unsafe { free(p) };
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
