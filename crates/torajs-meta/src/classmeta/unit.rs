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
//! vtable and method bodies. The reified method faces are NOT shared
//! since K2b — §15.7.14 gives every evaluation its own method
//! function objects, so each evaluation mints its own, MORTAL, faces
//! (they hold the class object, and the prototype entry that owns
//! them releases them).

use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

/// The class object's hidden record, an `Array<Any>` fixed at wire
/// time: `[tag, unit₀, …, unitₙ₋₁]`.
///
/// - `tag` is the one thing an evaluation's class object cannot answer
///   through the by-tag registry (printing, `toString`, `new v()`).
/// - `unit₀ … unitₙ₋₁` are the class objects of its unit ancestors,
///   root first (K5-2); `n` is the class's DEPTH in its unit chain and
///   the class object itself stands at that depth. §7.3.31 binds a
///   private element of class `C` to one evaluation of `C`, and an
///   instance carries only its LEAF class's evaluation — this array is
///   how the leaf answers "which evaluation of `C` am I under" in one
///   indexed read, whatever the program later does to either
///   [[Prototype]] chain.
///
/// The record holds its ancestors strongly. The class object already
/// does through its [[Prototype]], so no new kind of edge (and no
/// cycle: an ancestor never reaches a descendant).
///
/// Non-enumerable, non-writable, non-configurable, and hidden from
/// every own-key walk the way the `\0proto` slot is
/// (`obj_own_keys_key_shape::key_is_hidden_own`).
pub(crate) const UNIT_RECORD_KEY: &[u8] = b"\x00unit";
const UNIT_RECORD_ATTRS: u64 = (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6);
const ANY_I64: u64 = 2;
const ARR_LEN_OFF: usize = 8;

/// Hidden slot offset per tag; 0 = not a unit class. Written by every
/// wire (idempotent), so no prologue ordering is involved.
static UNIT_SLOT_OFF: [AtomicU32; MAX_CLASSES] = [const { AtomicU32::new(0) }; MAX_CLASSES];

unsafe extern "C" {
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64;
}

/// The record array of a unit class object, BORROWED with its length
/// (`None` for any other cell).
///
/// # Safety
/// `cell` is NULL or a live heap cell.
unsafe fn unit_record(cell: *const c_void) -> Option<(*const c_void, u64)> {
    if cell.is_null() || unsafe { heap_type_tag(cell) } != TAG_DYNOBJ {
        return None;
    }
    unsafe {
        let key = alloc_str_key(UNIT_RECORD_KEY);
        let t = __torajs_dynobj_get_tag(cell, key.cast_const());
        let v = __torajs_dynobj_get_value(cell, key.cast_const());
        __torajs_str_drop(key);
        if t != ANY_HEAP as u64 || heap_type_tag(v as *const c_void) != crate::reflect::TAG_ARR {
            return None;
        }
        let arr = v as *const c_void;
        let len = arr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read();
        (len != 0).then_some((arr, len))
    }
}

/// A fresh record for a class of `tag` under `parent_anyv`: the
/// parent's ancestors, then the parent. Anything but a unit class
/// object as the parent means depth 0.
///
/// # Safety
/// `parent_anyv` is any AnyValue word.
unsafe fn mint_record(tag: i64, parent_anyv: u64) -> *mut u8 {
    unsafe {
        let mut arr = __torajs_arr_alloc_any(4);
        arr = __torajs_arr_push_any(arr.cast(), ANY_I64, tag as u64);
        if !is_cell_imm(parent_anyv) {
            return arr;
        }
        let Some((prec, plen)) = unit_record(parent_anyv as *const c_void) else {
            return arr;
        };
        let inherited = (1..plen).map(|i| __torajs_arr_get_any_value(prec, i));
        for unit in inherited.chain(core::iter::once(parent_anyv)) {
            __torajs_rc_inc(unit as *mut c_void);
            arr = __torajs_arr_push_any(arr.cast(), ANY_HEAP as u64, unit);
        }
        arr
    }
}

/// The hidden slot offset of a unit class's instances, 0 otherwise.
pub(super) fn unit_slot_off(tag: i64) -> u32 {
    if !in_range(tag) {
        return 0;
    }
    UNIT_SLOT_OFF[tag as usize].load(Ordering::Relaxed)
}

/// The prototype of a unit-class instance, BORROWED (0 when the hidden
/// slot does not hold a class object).
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell of class `tag`, a unit class whose
/// instances keep their class object at `off`.
pub(super) unsafe fn unit_instance_proto(cell: *const c_void, off: u32) -> u64 {
    unsafe { unit_class_prototype(cell.cast::<u8>().add(off as usize).cast::<u64>().read()) }
}

/// The `prototype` own value of a class object, BORROWED (0 when
/// `class` is not a dynobj cell holding one).
///
/// # Safety
/// `class` is any AnyValue word.
unsafe fn unit_class_prototype(class: u64) -> u64 {
    unsafe {
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
/// `slot_off` the offset of the hidden slot its instances carry;
/// `parent_anyv` is the parent's class object for a derived class
/// (K5-1), `undefined` for a base.
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
    parent_anyv: u64,
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
        // K5-1 — §15.7.14 steps 5–8: a derived class's constructor
        // inherits from the parent CONSTRUCTOR and its prototype from
        // the parent's `prototype`, here the class object of the same
        // evaluation (`parent_anyv`; anything but a class object means
        // a base class).
        let parent_proto = unit_class_prototype(parent_anyv);
        let func_proto = if parent_proto != 0 {
            crate::reflect_proto_set::__torajs_proto_link_fresh(proto_anyv, parent_proto);
            parent_anyv as *mut c_void
        } else {
            __torajs_get_builtin_prototype(FUNCTION_PROTO_TAG)
        };
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
            ANY_HEAP as u64,
            mint_record(tag, parent_anyv) as u64,
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
        reify::define_unit_rows(tag, slot, class_anyv);
    }
}

/// K2b — the class object held in `cell`'s hidden unit slot, or 0
/// when `cell` is not an instance of a unit class. The guard at a
/// method-call boundary compares this with the face's own unit: a
/// face minted by one evaluation running against another
/// evaluation's instance is a §7.3.31 brand failure for every
/// private element it touches.
///
/// # Safety
/// `cell` is NULL or a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_unit_of(cell: *const c_void) -> u64 {
    if cell.is_null() {
        return 0;
    }
    unsafe {
        if heap_type_tag(cell) != crate::reflect::TAG_OBJ {
            return 0;
        }
        let tag = i64::from(cell.cast::<u8>().add(8).cast::<u32>().read());
        let off = unit_slot_off(tag);
        if off == 0 {
            return 0;
        }
        let class = cell.cast::<u8>().add(off as usize).cast::<u64>().read();
        if is_cell_imm(class) && heap_type_tag(class as *const c_void) == TAG_DYNOBJ {
            class
        } else {
            0
        }
    }
}

/// The tag a unit-class object records, or -1 for any other cell.
///
/// # Safety
/// `cell` is NULL or a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_unit_tag_of(cell: *const c_void) -> i64 {
    unsafe {
        let Some((rec, _)) = unit_record(cell) else {
            return -1;
        };
        let tag = __torajs_arr_get_any_value(rec, 0) as i64;
        if in_range(tag) { tag } else { -1 }
    }
}

/// The evaluation of the depth-`depth` class of `unit`'s chain: `unit`
/// itself at its own depth, a recorded ancestor below it, 0 for
/// anything else (a word that is no unit class object, a depth the
/// chain does not reach).
///
/// # Safety
/// `unit` is any word a hidden evaluation slot or a static `this` can
/// hold.
unsafe fn unit_at(unit: u64, depth: u64) -> u64 {
    unsafe {
        if !is_cell_imm(unit) {
            return 0;
        }
        let Some((rec, len)) = unit_record(unit as *const c_void) else {
            return 0;
        };
        match depth.cmp(&(len - 1)) {
            core::cmp::Ordering::Less => __torajs_arr_get_any_value(rec, 1 + depth),
            core::cmp::Ordering::Equal => unit,
            core::cmp::Ordering::Greater => 0,
        }
    }
}

/// K5-2 — whether two evaluation words stand under the SAME evaluation
/// of the class at `depth` of their chains (1 / 0): the §7.3.31 brand
/// of that class's private elements across its subclasses. `B1`'s
/// instance and `A1`'s agree at `A`'s depth exactly when `B1` was
/// evaluated under `A1`.
///
/// # Safety
/// Both words come from a hidden evaluation slot or a static `this`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_unit_same_at(mine: u64, theirs: u64, depth: i64) -> i64 {
    if depth < 0 {
        return 0;
    }
    let mine = unsafe { unit_at(mine, depth as u64) };
    i64::from(mine != 0 && mine == unsafe { unit_at(theirs, depth as u64) })
}

/// K5-3 — the evaluation of the depth-`depth` class that `word`
/// stands under, as an OWNED AnyValue (`undefined` when there is none):
/// the HOME of that class's methods, whose [[Prototype]] is the
/// `super` base of §13.3.7.
///
/// # Safety
/// `word` comes from a hidden evaluation slot or a static `this`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_unit_home(word: u64, depth: i64) -> u64 {
    let unit = if depth < 0 {
        0
    } else {
        unsafe { unit_at(word, depth as u64) }
    };
    if unit == 0 {
        return VALUE_UNDEFINED_IMM;
    }
    unsafe { __torajs_rc_inc(unit as *mut c_void) };
    unit
}

/// K5-2 — whether `unit` is `leaf` or one of `leaf`'s recorded
/// ancestors: the call-boundary guard's question, a face minted by
/// `unit`'s evaluation running on a receiver of `leaf`'s.
///
/// # Safety
/// `unit` is a live unit class object; `leaf` is any word.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_unit_covers(unit: u64, leaf: u64) -> i64 {
    unsafe {
        let Some((_, len)) = unit_record(unit as *const c_void) else {
            return 0;
        };
        i64::from(unit_at(leaf, len - 1) == unit)
    }
}
