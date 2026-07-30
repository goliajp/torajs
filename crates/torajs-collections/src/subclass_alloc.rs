//! Map/Set-subclass instance allocation (RFC
//! 20260730-exotic-backed-class-instance blade 2).
//!
//! `class C extends Map | Set` mints a REAL collection cell — the
//! whole Map/Set surface (set/get/has/add/size/iteration) rides the
//! existing arms because the instance IS a collection. Class identity
//! rides blade 0 (`FLAG_SUBCLASSED` + torajs-meta side table),
//! scrubbed by the map/set drop path (already wired).
//!
//! `super()` contributes nothing beyond the mint (a fresh collection
//! is the no-argument ctor's answer); the iterable-seeding form
//! (`super(pairs)`, §24.1.1.1 AddEntriesFromIterable) is a later
//! seam, loud at the desugar boundary.

use core::ffi::c_void;

use crate::create::{__torajs_map_create, __torajs_set_create};
use crate::layout::Map;

/// `torajs_rc::FLAG_SUBCLASSED` mirror (flags bit 0, RFC 20260730
/// blade 0 — same mirror drop.rs carries).
const FLAG_SUBCLASSED: u16 = 1;

/// `torajs_rc::AnySlotTag::Heap` mirror.
const ANY_HEAP: i64 = 4;

unsafe extern "C" {
    /// torajs-meta — record the fresh instance's class identity
    /// (blade 0). Takes no reference on the proto cell.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
    /// torajs-meta classmeta — the class's registered `__proto_<C>`
    /// AnyValue immediate (0 when unregistered).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    /// torajs-anyvalue — NaN-box encode.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// Mark + register the fresh collection's class identity and answer
/// it boxed — subclass instances live in the any world.
unsafe fn mint_common(p: *mut c_void, class_tag: i64) -> u64 {
    unsafe {
        (*(p as *mut Map)).header.flags |= FLAG_SUBCLASSED;
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(p, class_tag, proto_cell);
        __torajs_anyv_box_from_pair(ANY_HEAP, p as i64)
    }
}

/// Mint a Map-subclass instance (fresh empty Map).
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_subclass_alloc(class_tag: i64) -> u64 {
    unsafe { mint_common(__torajs_map_create(), class_tag) }
}

/// Mint a Set-subclass instance (fresh empty Set).
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_subclass_alloc(class_tag: i64) -> u64 {
    unsafe { mint_common(__torajs_set_create(), class_tag) }
}
