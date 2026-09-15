//! A closure cell's own keys — §10.2.9 / §10.2.10 the virtual `length`
//! / `name` pair, §20.2.4 a plain function's `prototype`, then the
//! synthesized %Function.prototype% names and the expando dict. Moved
//! out of [`crate::obj_own_keys`] (file-size: that dispatcher sits at
//! its cap).

use core::ffi::c_void;

use crate::obj_own_keys::{CLOSURE_PROPS_OFF, dynobj_keys_append};
use crate::obj_own_keys_proto_names::push_synthesized_proto_names;

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    /// torajs-anyvalue — a builtin constructor cell's own string keys
    /// after `length` / `name` (immortal interned cells, borrowed); a
    /// `cap == 0` probe answers the count, 0 for every other closure.
    fn __torajs_builtin_ctor_own_names(cell: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const u8) -> bool;
}

/// `torajs_rc::flags` mirrors — `delete fn.name` / `delete fn.length`
/// tombstones on a Closure header (bits 10/11).
const FLAG_FN_NAME_DELETED: u16 = 1 << 10;
const FLAG_FN_LENGTH_DELETED: u16 = 1 << 11;
/// `torajs_rc::FLAG_FN_PROTO` mirror (bit 15, Closure-private —
/// disjoint-by-tag with Arr's exotic-index bit). Set by the compiler on
/// a plain `function` cell; clear for arrow / method forms, which own
/// no `prototype` at all (§20.2.4 vs §10.2.5).
const FLAG_FN_PROTO: u16 = 1 << 15;

/// The closure arm of `__torajs_anyv_own_keys`.
///
/// # Safety
/// `cell` is a live Closure cell.
pub(crate) unsafe fn closure_cell_keys(cell: *const c_void, include_nonenum: i64) -> *mut c_void {
    let props =
        unsafe { (cell.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64).read() } as *const c_void;
    let mut out = unsafe { __torajs_arr_alloc(0) };
    // §10.2.9/§10.2.10 (via §20.2.4) — every function's
    // virtual own `length` / `name` pair surfaces on gOPN
    // in creation order (length first; test262
    // property-order family asserts the adjacency).
    // Non-enumerable, so `Object.keys` stays exact.
    // Delete tombstones live on the header flags.
    if include_nonenum != 0 {
        let flags = unsafe { (cell.cast::<u8>().add(6) as *const u16).read() };
        for (name, deleted) in [
            (&b"length"[..], flags & FLAG_FN_LENGTH_DELETED != 0),
            (&b"name"[..], flags & FLAG_FN_NAME_DELETED != 0),
            // §20.2.4 — a plain `function` also owns
            // `prototype` {writable: true, enumerable:
            // false, configurable: false}, created WITH the
            // function, so it sits right after the
            // name/length pair. tr materializes it lazily
            // into the props dict on first read (RFC
            // 20260721 刀 9), so emitting it here (and
            // filtering it out of the props append below)
            // makes the order independent of whether
            // anything has read it yet. An arrow / method
            // form has no `prototype` at all — the header
            // flag is exactly that distinction. Not
            // deletable (configurable: false), so no
            // tombstone.
            (&b"prototype"[..], flags & FLAG_FN_PROTO == 0),
        ] {
            if !deleted {
                let k = unsafe { crate::reflect::alloc_str_key(name) };
                out = unsafe { __torajs_arr_push(out, k as i64) };
            }
        }
    }
    // §20.2.3 makes `Function.prototype` a built-in
    // FUNCTION object rather than an ordinary one, so its
    // synthesized method names come through this arm
    // instead of the dynobj walk — the same "other cell
    // shape, same surface" hop `arr_cell_keys` makes for
    // the Array-exotic `Array.prototype`.
    if include_nonenum != 0 {
        out = unsafe { push_synthesized_proto_names(cell, out, true) };
        out = unsafe { push_builtin_ctor_names(cell, props, out) };
    }
    if props.is_null() {
        out as *mut c_void
    } else {
        unsafe { dynobj_keys_append(props, include_nonenum, out, false, true) as *mut c_void }
    }
}

/// A builtin constructor's `prototype`, table statics and data
/// constants — all non-enumerable, so only the gOPN / ownKeys surface
/// lists them. A key the expando dict also holds (a program's
/// `defineProperty(Array, "from", …)`) is the same property, which
/// the dict walk lists, so it is skipped here.
///
/// # Safety
/// `cell` is a live Closure cell; `props` is its expando dict or null.
unsafe fn push_builtin_ctor_names(
    cell: *const c_void,
    props: *const c_void,
    mut out: *mut u8,
) -> *mut u8 {
    let n = unsafe { __torajs_builtin_ctor_own_names(cell, core::ptr::null_mut(), 0) };
    if n == 0 {
        return out;
    }
    let mut names = vec![0u64; n as usize];
    unsafe { __torajs_builtin_ctor_own_names(cell, names.as_mut_ptr(), n) };
    for key in names {
        if !props.is_null() && unsafe { __torajs_dynobj_has(props, key as *const u8) } {
            continue;
        }
        // Immortal interned cells: the array slot borrows.
        out = unsafe { __torajs_arr_push(out, key as i64) };
    }
    out
}
