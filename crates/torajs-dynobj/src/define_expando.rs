//! Where a define LANDS when the receiver keeps its own properties in
//! a side dynobj — the lazy-expando arm.
//!
//! Split out of `define.rs` (file-size hard limit; §10.4.6.6's
//! namespace refusal needs room in the validate/apply kernel). The
//! seam is the one the module already had: this file answers *which
//! table the property goes into*, `define.rs` answers *what
//! §10.1.6.3 does once it is there* — the recursion at the end of
//! `define_into_expando` is exactly that handoff.

use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-rc — a closure cell's first expando attach (A5): the
    /// closure's env-drop seams are guarded on this entry's liveness.
    fn __torajs_closure_props_attach(cell: *mut u8, props: *mut c_void);
}

/// Lazy-expando receiver arm (Closure / Promise) — the cell's own
/// defines land in the props dynobj at `props_off`, allocated on
/// first touch; recursing with that slot runs the full §10.1.6.3
/// validate/apply against the entry table. `seed_virtual` is the
/// Closure receiver's reflected own `name`/`length` seeding — a
/// Promise cell has none.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn define_into_expando(
    obj: *mut c_void,
    props_off: usize,
    seed_virtual: bool,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
    attach: Option<unsafe extern "C" fn(*mut u8, *mut c_void)>,
) -> i64 {
    let props_slot = unsafe { obj.cast::<u8>().add(props_off) } as *mut *mut c_void;
    unsafe {
        if (*props_slot).is_null() {
            // r502 — a receiver whose drop legs sit behind link seams
            // (closure: A5) attaches through the rc entry the seams
            // are guarded on; the others write the slot directly.
            let fresh = crate::alloc::__torajs_dynobj_alloc();
            match attach {
                Some(attach) => attach(obj.cast::<u8>(), fresh),
                None => *props_slot = fresh,
            }
        }
        if seed_virtual {
            crate::define_entry::seed_virtual_fn_prop(obj, props_slot, key);
        }
        crate::define::define_apply(props_slot, key, tag, value, flags_byte, throw_on_refusal)
    }
}

/// The closure receiver's expando define: virtual `name` / `length`
/// seeded, the first attach through torajs-rc's
/// `__torajs_closure_props_attach` (the env-drop seams' guard, A5).
pub(crate) unsafe fn define_into_closure_expando(
    obj: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        define_into_expando(
            obj,
            crate::layout::CELL_PROPS_OFF,
            true,
            key,
            tag,
            value,
            flags_byte,
            throw_on_refusal,
            Some(__torajs_closure_props_attach),
        )
    }
}
