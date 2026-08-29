//! Which prototype an iterator cell answers.
//!
//! §23.1.5.1 CreateArrayIterator, §22.1.5.1 CreateStringIterator,
//! §24.1.5.1 / §24.2.5.1 CreateMapIterator / CreateSetIterator and
//! the §27.1.5 helper mint each give their object a DIFFERENT
//! [[Prototype]], and tr backs those five with three cell tags. So
//! the tag alone cannot name the prototype, and this is the one place
//! that knows how to get from a cell to its slot:
//!
//! - `MapIter` — the SOURCE decides. A Set is its own heap tag
//!   sharing the Map layout, so a MapIter over a `Tag::Set` cell is a
//!   set iterator by construction and nothing can drift.
//! - `ArrIter` — the source cannot decide, because a string iteration
//!   is materialized as an ArrIter over a character array. The cell
//!   carries a family word saying which mint made it
//!   (`torajs_arr::iter`).
//! - `IterHelper` — one family, no question to ask.
//!
//! Three consumers read this: `getPrototypeOf`
//! (`torajs_meta::reflect_proto`), the property-chain walk
//! (`torajs_anyvalue::method_value::family`), and the badge that
//! falls out of it. They used to answer %Iterator.prototype% flatly —
//! a chain one link short, and a chain one link short has nowhere to
//! keep the `@@toStringTag` that names the badge.

use core::ffi::c_void;

use super::{
    ARRAY_ITER_PROTO_TAG, ITER_HELPER_PROTO_TAG, MAP_ITER_PROTO_TAG, SET_ITER_PROTO_TAG,
    STRING_ITER_PROTO_TAG,
};
use crate::Tag;

#[cfg(not(test))]
unsafe extern "C" {
    /// torajs-arr — `ARR_ITER_FAMILY_ARRAY` / `_STRING`.
    fn __torajs_arr_iter_family(iter: *const c_void) -> u32;
}

// torajs-rc's own `cargo test` build has no torajs-arr staticlib to
// link; downstream crates that pull this in as an rlib provide the
// symbol through their own link stubs.
#[cfg(test)]
unsafe fn __torajs_arr_iter_family(_iter: *const c_void) -> u32 {
    0
}

/// `torajs_arr::iter::ARR_ITER_FAMILY_STRING` mirror.
const FAMILY_STRING: u32 = 1;

/// The MapIter / ArrIter source cell — both layouts put it at +8.
const SOURCE_OFF: usize = 8;

/// The builtin-proto slot an iterator cell inherits from, or `-1`
/// when `tag` is not an iterator tag.
///
/// # Safety
/// `ptr` is a live heap cell whose header tag is `tag`.
pub unsafe fn iter_cell_proto_tag(ptr: *const c_void, tag: u16) -> i64 {
    if tag == Tag::MapIter as u16 {
        let src = unsafe {
            ptr.cast::<u8>()
                .add(SOURCE_OFF)
                .cast::<*const c_void>()
                .read()
        };
        if src.is_null() {
            return MAP_ITER_PROTO_TAG as i64;
        }
        let src_tag = unsafe { src.cast::<u8>().add(4).cast::<u16>().read() };
        return if src_tag == Tag::Set as u16 {
            SET_ITER_PROTO_TAG as i64
        } else {
            MAP_ITER_PROTO_TAG as i64
        };
    }
    if tag == Tag::ArrIter as u16 {
        return if unsafe { __torajs_arr_iter_family(ptr) } == FAMILY_STRING {
            STRING_ITER_PROTO_TAG as i64
        } else {
            ARRAY_ITER_PROTO_TAG as i64
        };
    }
    if tag == Tag::IterHelper as u16 {
        return ITER_HELPER_PROTO_TAG as i64;
    }
    -1
}

/// [`iter_cell_proto_tag`]'s extern face, for the runtime crates that
/// reach torajs-rc across the staticlib boundary rather than through
/// Cargo (torajs-meta keeps zero Cargo deps — vision §2).
///
/// # Safety
/// Same as [`iter_cell_proto_tag`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iter_cell_proto_tag(ptr: *const c_void, tag: i64) -> i64 {
    unsafe { iter_cell_proto_tag(ptr, tag as u16) }
}
