//! One write on a receiver's OWN face — no prototype chain, no
//! inherited accessor.
//!
//! Ordinary assignment (`crate::member_set`) consults the chain
//! before it creates: that is §10.1.9's whole shape, and it is what
//! makes an inherited setter run. A few operations in the spec are
//! defined the other way round — they name the receiver's own
//! property table directly, precisely so that the chain does NOT get
//! a say. §27.1.2.1's `SetterThatIgnoresPrototypeProperties` is the
//! first of them to need this here: it IS an inherited setter, so
//! reaching for [[Set]] to do its work would find itself again.
//!
//! CreateDataPropertyOrThrow on a fresh key and OrdinarySet on an
//! existing own one are the same call against a property table — a
//! new entry gets `{W,E,C}` all true, an existing one keeps the
//! attributes it has — which is why one function answers both of the
//! setter's two branches.
//!
//! The four shapes are the four the write faces already distinguish:
//! a Proxy (define is a trap of its own), a dynobj (its own table),
//! an array reached by name (the `arrprops` side table owns that
//! domain — the index domain never gets here, the callers' keys are
//! not indices), and every other cell with an in-layout expando bag.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_set::drop_payload;
use crate::member_set_symbol::{bag_write, props_slot_off};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-arr — the non-index own-property side table.
    fn __torajs_arrprops_set(arr_ptr: *mut c_void, key: *const c_void, tag: i64, value: i64);
}

/// Write `(tag, value)` into `recv`'s own property table under
/// `key`. Answers false when the receiver has nowhere to hold a
/// property — a static-layout struct cell, which cannot grow through
/// `any` yet (the recorded boundary the symbol lane carries too).
/// The caller flavors the refusal.
///
/// The `(tag, value)` pair transfers.
///
/// # Safety
/// `recv` is a live cell AnyValue and `ptr` the cell it boxes; `key`
/// is a live Str or Symbol cell.
pub(crate) unsafe fn own_face_write(
    recv: AnyValue,
    ptr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) -> bool {
    unsafe {
        // §10.5.6 — a Proxy's define is a separate trap from its set,
        // and this operation is a define.
        if crate::proxy::is_proxy(recv) {
            return crate::proxy_define::create_data_property(recv, key, tag, value) != 0;
        }
        let cell_tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if cell_tag == Tag::DynObj as u16 {
            let mut obj = ptr;
            crate::member_set::dynobj_set_flavored(&mut obj, key, tag, value, true);
            // A table resize swaps the store inside a stable header
            // cell (RFC 20260809-dynobj-store-split), so there is no
            // fresh address to hand back.
            return true;
        }
        // An array's non-index own properties live in the side table,
        // not in the +24 bag the symbol lane uses — writing them to
        // the bag would store where no reader looks.
        if cell_tag == Tag::Arr as u16 && !crate::member_get_symbol::key_is_symbol(key) {
            __torajs_arrprops_set(ptr, key, tag as i64, value as i64);
            return true;
        }
        let Some(off) = props_slot_off(cell_tag) else {
            drop_payload(tag, value);
            return false;
        };
        bag_write(ptr, cell_tag, off, key, tag, value, true);
        true
    }
}
