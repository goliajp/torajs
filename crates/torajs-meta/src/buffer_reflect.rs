//! §25.1 / §23.2 — [[GetOwnProperty]] over a buffer-family cell,
//! split from `reflect_get_property_descriptor.rs` (file-size cap;
//! RFC 20260823-typedarray-substrate own-property knife). Only the
//! expando bag answers: `length` / `byteLength` etc. are PROTOTYPE
//! accessors, absent as own, and a typed array's canonical indices
//! as own descriptors are a recorded follow-up with the
//! numeric-index define.

use core::ffi::c_void;

use crate::reflect::{__torajs_dynobj_has, VALUE_UNDEFINED_IMM};
use crate::reflect_get_property_descriptor::{
    __torajs_anyv_get_property_descriptor, ARRAYBUFFER_PROPS_OFF, TAG_TYPEDARRAY,
    TYPEDARRAY_PROPS_OFF,
};

/// Descriptor for a string key on any receiver whose own properties
/// live in a lazy expando bag at `props_off` — the bag entry or
/// `undefined`. One body serves Promise, ArrayBuffer and TypedArray:
/// the mechanism (bag probe, then the DynObj descriptor path) is the
/// same, only the slot offset differs.
///
/// # Safety
/// `dynobj` is a live cell with a props slot at `props_off`; `key`
/// is a live Str cell.
pub(crate) unsafe fn expando_bag_descriptor(
    dynobj: *const c_void,
    props_off: usize,
    key: *const c_void,
) -> u64 {
    let props = unsafe {
        dynobj
            .cast::<u8>()
            .add(props_off)
            .cast::<*const c_void>()
            .read()
    };
    if !props.is_null() && unsafe { __torajs_dynobj_has(props, key as *const u8) } {
        return unsafe { __torajs_anyv_get_property_descriptor(props as u64, key) };
    }
    VALUE_UNDEFINED_IMM
}

/// The buffer-family entry — picks the slot for `htag`.
///
/// # Safety
/// `dynobj` is a live buffer-family cell of `htag`; `key` is a live
/// Str cell.
pub(crate) unsafe fn buffer_cell_descriptor(
    dynobj: *const c_void,
    htag: u16,
    key: *const c_void,
) -> u64 {
    let off = if htag == TAG_TYPEDARRAY {
        TYPEDARRAY_PROPS_OFF
    } else {
        // ArrayBuffer and DataView share +32 by construction.
        ARRAYBUFFER_PROPS_OFF
    };
    unsafe { expando_bag_descriptor(dynobj, off, key) }
}
