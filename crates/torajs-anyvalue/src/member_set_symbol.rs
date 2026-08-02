//! §6.1.7 symbol-keyed member writes — the `o[sym] = v` lane.
//!
//! Twin of the read lane in [`crate::member_get_symbol`], and split
//! from [`crate::member_set`] for the same reason: every receiver arm
//! there is guarded by NAME tests — `key_is(key, "lastIndex")`,
//! `"name"` / `"length"` readonly refusals, a builtin ctor's
//! non-writable `prototype`, the Symbol constructor's well-known
//! statics, `length` resize, canonical-index element routing, the
//! Annex B `__proto__` setter. None of them can apply to a symbol key,
//! and each would read Str payload offsets off a 16-byte Symbol cell.
//!
//! What IS shared is the part that does not care how a key is spelled:
//! §10.1.9.2's prototype-chain consult on an own miss, via
//! [`crate::member_set::inherited_set_handled`]. An own hit needs no
//! special handling either — `__torajs_dynobj_set` already dispatches
//! an accessor entry's setter and refuses a non-writable data entry.
//!
//! So the whole lane is: find the receiver's property dict (allocating
//! the in-layout expando on first write), consult the chain on a miss,
//! then write. A receiver with nowhere to put a property — a
//! primitive, or a static-layout struct cell, which cannot grow
//! through `any` yet — rejects exactly as the string lane does.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_set::{drop_payload, dynobj_set_flavored, inherited_set_handled};
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `torajs_dynobj::layout::CELL_PROPS_OFF` mirror — Arr / Closure keep
/// their expando dict in the same `+24` slot.
const CELL_PROPS_OFF: usize = 24;

/// `torajs-wrapper::WRAPPER_PROPS_OFF` mirror — the primitive
/// wrappers' expando slot.
const WRAPPER_PROPS_OFF: usize = 16;

/// The in-layout expando-dict slot for a shape that carries one, or
/// `None` for a shape that cannot hold a symbol-keyed property.
fn props_slot_off(cell_tag: u16) -> Option<usize> {
    // Blade 2 (RFC 20260714-struct-dynamic-props) — a struct cell
    // carries the same +24 expando slot as Arr / Closure: a symbol
    // key never collides with the string-keyed layout fields, so no
    // layout probe is needed on this lane.
    if cell_tag == Tag::Arr as u16 || cell_tag == Tag::Closure as u16 || cell_tag == Tag::Obj as u16
    {
        return Some(CELL_PROPS_OFF);
    }
    if cell_tag == Tag::NumberWrapper as u16
        || cell_tag == Tag::StringWrapper as u16
        || cell_tag == Tag::BooleanWrapper as u16
    {
        return Some(WRAPPER_PROPS_OFF);
    }
    None
}

/// `o[sym] = v`. Returns without writing (releasing the value stake and
/// recording a TypeError) when the receiver has nowhere to hold a
/// property.
///
/// # Safety
/// `recv_slot` points at a live AnyValue slot the caller owns; `ptr` is
/// the live cell `recv` boxes and `cell_tag` its `type_tag`; `key` is a
/// live Symbol cell; `(tag, value)` carries the caller's +1 on heap
/// payloads.
pub(crate) unsafe fn symbol_member_set(
    recv_slot: *mut AnyValue,
    recv: AnyValue,
    ptr: *mut c_void,
    cell_tag: u16,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        if cell_tag == Tag::DynObj as u16 {
            if __torajs_dynobj_has(ptr, key as *const c_void) == 0
                && let Some(handled) =
                    inherited_set_handled(ptr, recv, key, tag, value, throw_on_refusal)
            {
                return handled;
            }
            let mut obj = ptr;
            let wrote = dynobj_set_flavored(&mut obj, key, tag, value, throw_on_refusal);
            if obj != ptr {
                // Relocated block — the NaN-box cell encoding is the
                // pointer bits; transfer, no rc traffic (same object
                // identity, moved storage).
                *recv_slot = __torajs_anyv_box_from_pair(4, obj as i64);
            }
            return wrote;
        }
        let Some(off) = props_slot_off(cell_tag) else {
            // A primitive receiver's write is discarded per §10.1.9,
            // and a static-layout struct cell cannot grow through
            // `any` yet — the string lane's boundary TypeError.
            drop_payload(tag, value);
            if throw_on_refusal {
                __torajs_throw_type_error(
                    c"cannot set a symbol-keyed property on this value".as_ptr(),
                );
            }
            return 0;
        };
        let props_slot = ptr.cast::<u8>().add(off) as *mut u64;
        let mut props = *props_slot as *mut c_void;
        if props.is_null() {
            props = __torajs_dynobj_alloc();
        }
        let wrote = dynobj_set_flavored(&mut props, key, tag, value, throw_on_refusal);
        // First-write alloc and resize relocation both land the fresh
        // table back in the slot; the host cell itself never moves.
        *props_slot = props as u64;
        wrote
    }
}
