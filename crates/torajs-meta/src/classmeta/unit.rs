//! Per-evaluation class units (RFC 20260917-class-eval-unit K1).
//!
//! A class declared where it can be evaluated more than once (a
//! function body, a loop body) is a new class object and a new
//! prototype on every evaluation (§15.7.14). The shared by-tag
//! registry cannot hold that: it names one class object per source
//! class. A unit class therefore never registers there. Its program
//! mints the class object and the prototype at the declaration's own
//! position and hands them to [`__torajs_class_unit_wire`], which runs
//! what the registration runs for a one-shot class — minus every write
//! to the singleton tables — and each instance carries its class
//! object in a hidden layout slot, whose offset this module records
//! per tag for the one runtime question an instance asks: its
//! prototype (`obj_proto::__torajs_obj_proto_borrowed`).
//!
//! What stays shared across evaluations, and is right to: layout,
//! vtable, method bodies, and — for now — the reified method faces
//! (K1 registered deviation: `C1.prototype.m === C2.prototype.m`;
//! K2b gives each unit its own). The faces are minted once per
//! (tag, row) and reused, because a face is an immortal static cell:
//! minting one per evaluation would leak one per call.

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use super::*;

/// The class object's hidden record: its tag, the one thing an
/// evaluation's class object cannot answer through the by-tag registry
/// (printing, `toString`, `new v()`). Non-enumerable, non-writable,
/// non-configurable, and hidden from every own-key walk the way the
/// `\0proto` slot is (`obj_own_keys_key_shape::key_is_internal_slot`).
pub(crate) const UNIT_RECORD_KEY: &[u8] = b"\x00unit";
const UNIT_RECORD_ATTRS: u64 = (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6);
const ANY_I64: u64 = 2;

/// Hidden slot offset per tag; 0 = not a unit class. Written by every
/// wire (idempotent), so no prologue ordering is involved.
static UNIT_SLOT_OFF: [AtomicU32; MAX_CLASSES] = [const { AtomicU32::new(0) }; MAX_CLASSES];

/// Per tag, a lazily allocated array of reified method faces indexed
/// by method-table row (0 = not minted yet).
static UNIT_FACES: [AtomicPtr<u64>; MAX_CLASSES] =
    [const { AtomicPtr::new(ptr::null_mut()) }; MAX_CLASSES];

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const u8) -> u64;
}

/// The hidden slot offset of a unit class's instances, 0 otherwise.
pub(super) fn unit_slot_off(tag: i64) -> u32 {
    if !in_range(tag) {
        return 0;
    }
    UNIT_SLOT_OFF[tag as usize].load(Ordering::Relaxed)
}

/// The face cache of `tag`'s `n` method rows, allocated zeroed on
/// first use.
///
/// # Safety
/// `tag` is in range; `n` is the class's method-row count.
pub(super) unsafe fn unit_faces(tag: i64, n: u32) -> *mut u64 {
    let cell = &UNIT_FACES[tag as usize];
    let cur = cell.load(Ordering::Relaxed);
    if !cur.is_null() || n == 0 {
        return cur;
    }
    let bytes = n as usize * core::mem::size_of::<u64>();
    // SAFETY: a fresh block of `n` u64 slots, zeroed below.
    let fresh = unsafe { malloc(bytes) }.cast::<u64>();
    unsafe { ptr::write_bytes(fresh, 0, n as usize) };
    cell.store(fresh, Ordering::Relaxed);
    fresh
}

/// The prototype of a unit-class instance, BORROWED (0 when the hidden
/// slot does not hold a class object).
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell of class `tag`, a unit class whose
/// instances keep their class object at `off`.
pub(super) unsafe fn unit_instance_proto(cell: *const c_void, off: u32) -> u64 {
    unsafe {
        let class = cell.cast::<u8>().add(off as usize).cast::<u64>().read();
        if !is_cell_imm(class) || heap_type_tag(class as *const c_void) != TAG_DYNOBJ {
            return 0;
        }
        let key = alloc_str_key(b"prototype");
        let t = __torajs_dynobj_get_tag(class as *const c_void, key.cast_const());
        let v = __torajs_dynobj_get_value(class as *const c_void, key.cast_const());
        __torajs_str_drop(key);
        if t == ANY_HEAP as u64 { v } else { 0 }
    }
}

/// Wire one evaluation of unit class `tag`: `class_anyv` / `proto_anyv`
/// are the class object and prototype the declaration just minted, and
/// `slot_off` the offset of the hidden slot its instances carry.
/// §10.2.3 MakeConstructor and §15.7.14's method definitions, exactly
/// as `wire_first_class_links` runs them for a one-shot class, onto
/// these two objects only.
///
/// # Safety
/// Both values are live dynobj cells; `tag` names the class's layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_unit_wire(
    class_anyv: u64,
    proto_anyv: u64,
    tag: i64,
    slot_off: i64,
) {
    if !in_range(tag)
        || !is_cell_imm(class_anyv)
        || !is_cell_imm(proto_anyv)
        || unsafe { heap_type_tag(proto_anyv as *const c_void) } != TAG_DYNOBJ
    {
        return;
    }
    UNIT_SLOT_OFF[tag as usize].store(slot_off as u32, Ordering::Relaxed);
    unsafe {
        let class_cell = class_anyv as *mut c_void;
        __torajs_dynobj_lock_builtin_fn_class_slots(class_cell);
        __torajs_dynobj_mark_class_ctor(class_cell);
        let func_proto = __torajs_get_builtin_prototype(FUNCTION_PROTO_TAG);
        if !func_proto.is_null() {
            let flags = class_cell.cast::<u8>().add(6).cast::<u16>().read();
            // A fresh class object: extensible, acyclic — the link
            // cannot be refused.
            let _ = crate::reflect_proto_set::ordinary_set_prototype_of(
                class_cell,
                func_proto as u64,
                flags,
            );
        }
        let mut class_slot = class_cell;
        let record = alloc_str_key(UNIT_RECORD_KEY);
        __torajs_dynobj_define_plain(
            &mut class_slot,
            record,
            ANY_I64,
            tag as u64,
            UNIT_RECORD_ATTRS,
        );
        __torajs_str_drop(record);
        let mut slot = proto_anyv as *mut c_void;
        let key = alloc_str_key(b"constructor");
        __torajs_rc_inc(class_cell);
        __torajs_dynobj_define_plain(
            &mut slot,
            key,
            ANY_HEAP as u64,
            class_anyv,
            DEFINE_CTOR_FLAGS,
        );
        __torajs_str_drop(key);
        reify::define_unit_rows(tag, slot);
    }
}

/// The tag a unit-class object records, or -1 for any other cell.
///
/// # Safety
/// `cell` is NULL or a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_unit_tag_of(cell: *const c_void) -> i64 {
    if cell.is_null() || unsafe { heap_type_tag(cell) } != TAG_DYNOBJ {
        return -1;
    }
    unsafe {
        let key = alloc_str_key(UNIT_RECORD_KEY);
        let t = __torajs_dynobj_get_tag(cell, key.cast_const());
        let v = __torajs_dynobj_get_value(cell, key.cast_const());
        __torajs_str_drop(key);
        if t == ANY_I64 && in_range(v as i64) {
            v as i64
        } else {
            -1
        }
    }
}
