//! WeakMap/WeakSet-subclass instance allocation (rotation 373 —
//! extends RFC 20260730-exotic-backed-class-instance blade 2 to the
//! weak collections, the torajs-collections `subclass_alloc` twin).
//!
//! `class C extends WeakMap | WeakSet` mints a REAL weak-collection
//! cell — the whole surface (set/get/has/add/delete) rides the
//! existing arms because the instance IS a weak collection. Class
//! identity rides blade 0 (`FLAG_SUBCLASSED` + torajs-meta side
//! table), scrubbed by the weakmap/weakset drop paths.
//!
//! `super()` contributes nothing beyond the mint; the iterable form
//! (`super(entries)`, §24.3.1.1 / §24.4.1.1) rides
//! `__torajs_collection_init_from_iterable` with the WEAK kinds —
//! the same kernel the plain `new WeakMap(iter)` ctor uses — via the
//! torajs-anyvalue super twins.

use core::ffi::c_void;

use crate::weakmap::__torajs_weakmap_create;
use crate::weakset::__torajs_weakset_create;

/// `torajs_rc::FLAG_SUBCLASSED` mirror (flags bit 0, RFC 20260730
/// blade 0 — same mirror the collections twin carries).
const FLAG_SUBCLASSED: u16 = 1;

/// `torajs_rc::AnySlotTag::Heap` mirror.
const ANY_HEAP: i64 = 4;

/// Universal heap header: flags u16 at byte +6 (the layout every
/// runtime crate mirrors).
const FLAGS_OFF: usize = 6;

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

/// Mark + register the fresh cell's class identity and answer it
/// boxed — subclass instances live in the any world.
unsafe fn mint_common(p: *mut c_void, class_tag: i64) -> u64 {
    unsafe {
        let flags = p.cast::<u8>().add(FLAGS_OFF) as *mut u16;
        *flags |= FLAG_SUBCLASSED;
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(p, class_tag, proto_cell);
        __torajs_anyv_box_from_pair(ANY_HEAP, p as i64)
    }
}

/// Mint a WeakMap-subclass instance (fresh empty WeakMap).
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_subclass_alloc(class_tag: i64) -> u64 {
    unsafe { mint_common(__torajs_weakmap_create(), class_tag) }
}

/// Mint a WeakSet-subclass instance (fresh empty WeakSet).
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakset_subclass_alloc(class_tag: i64) -> u64 {
    unsafe { mint_common(__torajs_weakset_create(), class_tag) }
}
