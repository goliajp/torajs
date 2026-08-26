//! r502 (RFC 20260824-s2-5 刀 4, the A5 shape on struct cells) — the
//! one entry through which a `Tag::Obj` cell (a class instance or an
//! anonymous struct shape) first grows its expando props bag at
//! `+24`.
//!
//! The typed drop the compiler synthesizes for a struct instance has
//! three legs that only matter when a bag exists: a fields-all-
//! scalar instance can sit on a cycle only through the bag, so its
//! rc-nonzero path buffers it as a cycle root (`__torajs_cycle_
//! buffer`); its rc-zero path releases the bag through the generic
//! value drop, and scrubs `FLAG_BUFFERED`. Each leg roots a runtime
//! world (the cycle collector, the weak registry, the whole drop
//! dispatch — 14 KB of text on a class program that never lets an
//! instance into the any world). The legs are link seams guarded on
//! THIS entry's text liveness: every first attach — the any-world
//! member set, the symbol-keyed set, `Object.defineProperty` on an
//! instance, an Error's `cause` install — comes here, and a grown
//! bag written back through the slot by a resize is not an attach.
//! A writer added later that bypasses it would surface as a named
//! TypeError on the instance's drop (the seam's loud-reject stub),
//! never as a silent leak.

use core::ffi::c_void;

/// `OBJ_PROPS_OFF` — the struct cell's expando slot (mirrors
/// torajs-anyvalue `member_get_layout::OBJ_PROPS_OFF` / torajs-dynobj
/// `layout::CELL_PROPS_OFF`).
const OBJ_PROPS_OFF: usize = 24;

/// Store `props` as the cell's expando bag — the cell takes
/// ownership. Not inlined: its text liveness is the link's evidence
/// that some instance may carry a bag.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap cell whose props slot is NULL;
/// `props` is a live dynobj.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_props_attach(cell: *mut u8, props: *mut c_void) {
    unsafe { *(cell.add(OBJ_PROPS_OFF) as *mut *mut c_void) = props }
}
