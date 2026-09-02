//! `__torajs_dynobj_proto_next` — the next USER prototype above a
//! dynobj (rotation 562).
//!
//! bun's inspect walks up to five [[Prototype]] hops and prints what
//! it finds there (`Object.create(base)` shows `base`'s properties,
//! `Sub.prototype` shows the methods `Base.prototype` declares), so
//! the walkers need one rule for "what is above this object". A
//! dynobj's link is its internal `\0proto` entry — the slot
//! `torajs-meta::reflect::PROTO_SLOT_KEY` writes; an object with no
//! entry has the implicit %Object.prototype%, which is where the
//! walk stops anyway.
//!
//! A prototype may also be a TYPED object literal — a `Tag::Obj`
//! struct cell whose rows live in `torajs-meta::struct_print_rows`
//! rather than in a dynobj entry array (562-10). The link is
//! returned all the same; the caller dispatches on the cell's tag.
//! The walk ENDS at such a cell: a struct's own [[Prototype]] link
//! is not a `\0proto` entry, so there is nothing further to read
//! here.
//!
//! A BUILTIN prototype ends the walk here. bun does print what it
//! finds on one (`Object.create(Array.prototype)` lists every array
//! method); tr's builtin prototypes are synthesized name lists
//! rather than dynobj entries, so walking into one would find
//! nothing to print. Registered as its own gap.

use core::ffi::c_void;

use crate::iter::{__torajs_dynobj_iter_key, __torajs_dynobj_iter_len, __torajs_dynobj_iter_value};
use crate::layout::{DYNOBJ_HDR_FLAG_NULL_PROTO, TAG_DYNOBJ, TAG_OBJ};
use crate::probe::key_str_bytes;

/// `torajs-meta::reflect::PROTO_SLOT_KEY` — the internal
/// [[Prototype]] slot's key, unspellable from the program.
const PROTO_SLOT_KEY: &[u8] = b"\x00proto";

/// bun walks at most five prototypes (`prototypeCount < 5`).
pub const MAX_PROTO_HOPS: usize = 5;

/// Universal heap-header `type_tag u16 @4` / `flags u16 @6`.
const HDR_TYPE_OFF: usize = 4;
const HDR_FLAGS_OFF: usize = 6;

unsafe extern "C" {
    fn __torajs_anyv_cell_ptr(v: u64) -> i64;
    /// torajs-rc — `>= 0` when the cell IS a builtin prototype.
    fn __torajs_builtin_proto_tag_of(obj: *const c_void) -> i64;
}

/// The user prototype above `obj` — a dynobj or a `Tag::Obj` struct
/// cell — or null when the walk ends (no link, a null prototype, the
/// implicit %Object.prototype%, or a builtin prototype).
///
/// # Safety
/// `obj` is NULL or a live heap cell with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_proto_next(obj: *const c_void) -> *const c_void {
    if obj.is_null() {
        return core::ptr::null();
    }
    unsafe {
        if *((obj as *const u8).add(HDR_TYPE_OFF) as *const u16) != TAG_DYNOBJ {
            return core::ptr::null();
        }
        if *((obj as *const u8).add(HDR_FLAGS_OFF) as *const u16) & DYNOBJ_HDR_FLAG_NULL_PROTO != 0
        {
            return core::ptr::null();
        }
        // The slot is one entry among a handful; the walkers scan
        // every entry anyway, so a linear find costs nothing and
        // needs no key cell minted to probe with.
        let len = __torajs_dynobj_iter_len(obj);
        for i in 0..len {
            let key = __torajs_dynobj_iter_key(obj, i);
            if key.is_null() {
                continue;
            }
            let Some((p, n, latin1)) = key_str_bytes(key) else {
                continue;
            };
            if !latin1 || n as usize != PROTO_SLOT_KEY.len() {
                continue;
            }
            if core::slice::from_raw_parts(p, n as usize) != PROTO_SLOT_KEY {
                continue;
            }
            let cell = __torajs_anyv_cell_ptr(__torajs_dynobj_iter_value(obj, i)) as *const c_void;
            if cell.is_null() {
                return core::ptr::null();
            }
            let tag = *((cell as *const u8).add(HDR_TYPE_OFF) as *const u16);
            if (tag != TAG_DYNOBJ && tag != TAG_OBJ) || __torajs_builtin_proto_tag_of(cell) >= 0 {
                return core::ptr::null();
            }
            return cell;
        }
        core::ptr::null()
    }
}
