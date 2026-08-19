//! §27.2.4 Promise static-slot monkey-patch consult — the runtime
//! half of the `Promise.{resolve,reject}` patch protocol (rotation
//! 448, promise-monkeypatch survey).
//!
//! The patch's single source of truth is the interned Promise
//! constructor cell's expando dict — `builtin_ctor_cell(10)` is what
//! the bare `Promise` ident answers in a value position, so the
//! any-lane write (`(Promise as any).resolve = fn`) and this probe
//! read one store. The static call lowering
//! ([`crate::method_value::ctor`]'s compiler-side consumer,
//! `ssa_lower_call_promise_resolve`) gates every `Promise.resolve` /
//! `Promise.reject` emit on the probe: unpatched programs pay two
//! loads and a compare, patched ones ride the any method dispatcher
//! against the ctor cell (this = the ctor, per §27.2.4.7 step 1).
//!
//! Two entries:
//! - [`__torajs_promise_ctor_patched`] — the call-site gate: 1 iff
//!   the ctor cell was ever minted AND its props dict holds an own
//!   entry under the method's name.
//! - [`__torajs_promise_patched_result`] — the typed lane's return
//!   contract: the patched call answered an owned Any; a heap
//!   %Promise% cell transfers through, anything else is a LOUD
//!   catchable TypeError. The static lane's `Type::Promise` slot
//!   cannot hold an arbitrary value, and minting a wrapper promise
//!   would silently break identity — fail-safe loud beats both
//!   (divergence from an untyped engine recorded in plan-state L3b).

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

unsafe extern "C" {
    /// torajs-dynobj — own-entry existence probe.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// The builtin-proto family tag of `Promise` (the
/// `builtin_proto_tag` row the bare ident read lowers with).
const PROMISE_PROTO_TAG: i64 = 10;

/// Lazily interned Str-cell keys, one per patchable static name —
/// indexed by the lowering's name id (0 = resolve, 1 = reject).
static PATCH_KEYS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

fn patch_key_cell(name_id: i64) -> *const c_void {
    let slot = &PATCH_KEYS[name_id as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *const c_void;
    }
    let name: &[u8] = if name_id == 0 { b"resolve" } else { b"reject" };
    let cell = crate::method_value::mint_immortal_str(name);
    slot.store(cell as u64, Ordering::Relaxed);
    cell as *const c_void
}

/// Whether `Promise.<name>` carries a user patch — see module doc.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_promise_ctor_patched(name_id: i64) -> i64 {
    let cell = crate::method_value::ctor::ctor_cell_peek(PROMISE_PROTO_TAG);
    if cell.is_null() {
        return 0;
    }
    // SAFETY: the peeked cell is a live immortal Closure-shape cell.
    let props = unsafe { crate::member_get_layout::closure_props(cell.cast()) };
    if props.is_null() {
        return 0;
    }
    // SAFETY: props is a live dynobj; the key is an immortal Str.
    unsafe { i64::from(__torajs_dynobj_has(props, patch_key_cell(name_id)) != 0) }
}

/// The typed lane's return contract over a patched call — see
/// module doc. Adopts the box's stake: a promise transfers it to
/// the returned cell, everything else releases it before the throw.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_promise_patched_result(ret: AnyValue, name_id: i64) -> *mut c_void {
    if is_cell(ret) {
        let p = as_void_ptr(ret);
        // SAFETY: a cell box points at a live heap header.
        let tag = unsafe { (p.cast::<u8>().add(4) as *const u16).read() };
        if tag == Tag::Promise as u16 {
            return p;
        }
    }
    unsafe {
        crate::nanbox_ffi::__torajs_anyv_rc_dec(ret);
        __torajs_throw_type_error(if name_id == 0 {
            c"the patched Promise.resolve answered a non-promise into a typed slot".as_ptr()
        } else {
            c"the patched Promise.reject answered a non-promise into a typed slot".as_ptr()
        });
    }
    core::ptr::null_mut()
}
