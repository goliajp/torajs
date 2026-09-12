//! `GlobalCell` — the heap object behind a global-object property
//! whose storage IS a top-level binding's data-global slot (RFC
//! 20260912-global-var-cell).
//!
//! §16.1.7 makes a script's top-level `var` an own data property of
//! the global object AND the binding the bare name reads — one
//! storage. tr keeps the binding where it always was (an LLVM data
//! global the compiled code loads and stores directly, typed) and lets
//! the entry in the global object's dynobj point at it: the entry's
//! `value_anyv` is a cell, resolved like an `AccessorPair` by the
//! pointee's `type_tag` (`torajs_rc::Tag::GlobalCell` = 30), and the
//! kernels that hand entry values out or take them in (`get_tag` /
//! `get_value`, `iter_value`, `set`, `define_redefine`) see through
//! it. Every bare-name access stays a plain load / store; only the
//! reflective face pays.
//!
//! ```text
//! offset | size | field
//! -------|------|------
//!   0    |  8B  | universal heap header (refcount + type_tag=30)
//!   8    |  8B  | slot_ptr — the data global's address (borrowed, never freed here)
//!  16    |  8B  | kind     — how the slot spells its value (`KIND_*`)
//! ```
//!
//! The cell does not own the slot's value: the exit-time drop hook
//! still settles the binding exactly as it did before the entry
//! existed. Dropping the cell frees 24 bytes and nothing else.
//!
//! A typed slot can only take a value of its own representation. A
//! write through the object of anything else is REFUSED (the caller
//! records the TypeError): storing a string's bits into an `i64` slot
//! would be silent wrong at every bare read after it, and widening the
//! slot is a compile-time decision this run cannot revisit. The `any`
//! slot — which is what every hoisted top-level `var` becomes — takes
//! everything.

use core::ffi::c_void;

use crate::layout::{
    ANY_HEAP, BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE,
    DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE,
    DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE, DEFINE_PRESENT_VALUE,
    DEFINE_PRESENT_WRITABLE,
};

unsafe extern "C" {
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_value_drop_heap(child: *mut c_void);
    /// torajs-anyvalue — borrow-shaped cell-pointer read: a heap cell
    /// answers its pointer bits, every immediate answers 0.
    fn __torajs_anyv_cell_ptr(v: u64) -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
}

/// `type_tag` of a `GlobalCell` block (mirrors `torajs_rc::Tag::GlobalCell`).
pub const TAG_GLOBAL_CELL: u16 = 30;
/// Total block size.
pub const GLOBAL_CELL_SIZE: usize = 24;
const SLOT_OFF: usize = 8;
const KIND_OFF: usize = 16;

/// The slot holds a NaN-box `AnyValue` it owns (every hoisted `var`).
pub const KIND_ANY: u64 = 0;
/// The slot holds a raw `i64` (an `i32` slot is loaded and stored as
/// a 64-bit word by the codegen too, so it spells the same).
pub const KIND_I64: u64 = 1;
/// The slot holds raw `f64` bits.
pub const KIND_F64: u64 = 2;
/// The slot holds a 64-bit word that is 0 or 1.
pub const KIND_BOOL: u64 = 3;
/// The slot holds an owned `Str` cell pointer.
pub const KIND_STR: u64 = 4;
/// The slot holds an owned pointer to some other heap cell (Arr / Obj
/// / Closure / …): readable, never writable through the object.
pub const KIND_HEAP: u64 = 5;

/// `AnySlotTag` mirrors (`torajs_rc::AnySlotTag`, stable wire format).
const ANY_BOOL: u64 = 1;
const ANY_I64: u64 = 2;
const ANY_F64: u64 = 3;
const ANY_UNDEF: u64 = 5;
/// `torajs_rc::Tag::Str`.
const TAG_STR: u16 = 0;

/// True when an entry's `value_anyv` is a `GlobalCell` — the single
/// detection predicate every seeing-through kernel shares.
#[inline]
pub(crate) unsafe fn value_is_global_cell(v_anyv: u64) -> bool {
    let ptr = unsafe { __torajs_anyv_cell_ptr(v_anyv) } as *const c_void;
    !ptr.is_null() && unsafe { *((ptr as *const u8).add(4) as *const u16) } == TAG_GLOBAL_CELL
}

/// An entry value with any `GlobalCell` resolved to the slot's
/// CURRENT value — what a reader is owed. Not a cell: answered as is.
/// The heap payload of the answer is borrowed, the same contract
/// `get_value` and `iter_value` have always had.
#[inline]
pub(crate) unsafe fn deref_global_cell(v_anyv: u64) -> u64 {
    if unsafe { value_is_global_cell(v_anyv) } {
        unsafe { global_cell_read(__torajs_anyv_cell_ptr(v_anyv) as *const c_void) }
    } else {
        v_anyv
    }
}

/// `__torajs_global_cell_alloc(slot_ptr, kind)` — mint a cell over a
/// data-global slot. Refcount 1, owned by the entry it is stored into.
///
/// # Safety
/// `slot_ptr` is the address of a live data global that outlives the
/// global object (a link-time slot does).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_global_cell_alloc(
    slot_ptr: *mut c_void,
    kind: u64,
) -> *mut c_void {
    let p = unsafe { calloc(GLOBAL_CELL_SIZE) } as *mut u8;
    // SAFETY: fresh zeroed block of GLOBAL_CELL_SIZE bytes.
    unsafe {
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_GLOBAL_CELL;
        *(p.add(SLOT_OFF) as *mut u64) = slot_ptr as u64;
        *(p.add(KIND_OFF) as *mut u64) = kind;
    }
    p as *mut c_void
}

/// `__torajs_global_cell_drop(cell)` — refcount dec; on hit-zero free
/// the block. The slot behind it is not the cell's to settle.
///
/// # Safety
/// `cell` is null or a live `GlobalCell`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_global_cell_drop(cell: *mut c_void) {
    if cell.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(cell) } == 0 {
        return;
    }
    unsafe { free(cell, GLOBAL_CELL_SIZE) };
}

/// The slot's current value as a NaN-box, per the cell's kind. Heap
/// payloads are borrowed. A null heap slot (a typed binding read
/// through the object before its declaration ran) answers `undefined`
/// rather than a null pointer.
pub(crate) unsafe fn global_cell_read(cell: *const c_void) -> u64 {
    let slot = unsafe { *((cell as *const u8).add(SLOT_OFF) as *const u64) } as *const u64;
    let kind = unsafe { *((cell as *const u8).add(KIND_OFF) as *const u64) };
    let word = unsafe { *slot };
    match kind {
        KIND_ANY => word,
        KIND_I64 => unsafe { __torajs_anyv_box_from_pair(ANY_I64 as i64, word as i64) },
        KIND_F64 => unsafe { __torajs_anyv_box_from_pair(ANY_F64 as i64, word as i64) },
        KIND_BOOL => unsafe { __torajs_anyv_box_from_pair(ANY_BOOL as i64, (word & 1) as i64) },
        _ => {
            if word == 0 {
                unsafe { __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0) }
            } else {
                unsafe { __torajs_anyv_box_from_pair(ANY_HEAP as i64, word as i64) }
            }
        }
    }
}

/// Store `(tag, value)` — an entry payload under the same ownership
/// contract `set` has (a heap value is the caller's transferred
/// reference) — into the slot behind `cell`. Answers `true` when the
/// slot's representation takes the value, `false` when it cannot; on
/// `false` nothing was written and the caller still owns `value`.
pub(crate) unsafe fn global_cell_write(cell: *const c_void, tag: u64, value: u64) -> bool {
    let slot = unsafe { *((cell as *const u8).add(SLOT_OFF) as *const u64) } as *mut u64;
    let kind = unsafe { *((cell as *const u8).add(KIND_OFF) as *const u64) };
    let new_word = match kind {
        KIND_ANY => {
            let old = unsafe { *slot };
            if unsafe { __torajs_anyv_unbox_tag(old) } as u64 == ANY_HEAP {
                unsafe { __torajs_value_drop_heap(old as *mut c_void) };
            }
            unsafe { __torajs_anyv_box_from_pair(tag as i64, value as i64) }
        }
        KIND_I64 => match tag {
            ANY_I64 => value,
            ANY_F64 if f64::from_bits(value).fract() == 0.0 => f64::from_bits(value) as i64 as u64,
            _ => return false,
        },
        KIND_F64 => match tag {
            ANY_F64 => value,
            ANY_I64 => (value as i64 as f64).to_bits(),
            _ => return false,
        },
        KIND_BOOL => match tag {
            ANY_BOOL => value & 1,
            _ => return false,
        },
        KIND_STR => {
            let is_str = tag == ANY_HEAP
                && value != 0
                && unsafe { *((value as *const u8).add(4) as *const u16) } == TAG_STR;
            if !is_str {
                return false;
            }
            let old = unsafe { *slot };
            if old != 0 {
                let boxed = unsafe { __torajs_anyv_box_from_pair(ANY_HEAP as i64, old as i64) };
                unsafe { __torajs_value_drop_heap(boxed as *mut c_void) };
            }
            value
        }
        _ => return false,
    };
    unsafe { *slot = new_word };
    true
}

/// The outcome of a value/attribute redefine over a global-cell entry.
pub(crate) enum CellRedefine {
    /// Value (if any) written through; the entry keeps its flags.
    Done,
    /// Value (if any) written through, and the caller must repack the
    /// entry's flags — a `configurable`/`enumerable` change on a
    /// configurable binding (§10.1.6.3).
    RepackFlags,
}

/// The `define_redefine` arm for a global-cell entry — §10.1.6.3 over
/// one storage. A [[Value]]-only redefine writes THROUGH to the
/// binding; a change of attributes, or an accessor coming in, is
/// refused: the compiled bare writes would not see a `writable:
/// false`, so honouring it here would be a silent wrong at every one
/// of them. `Err` carries the refusal message; nothing was written
/// and the caller still owns `value`.
pub(crate) unsafe fn redefine_through_cell(
    cur_value_anyv: u64,
    cur_flags: u64,
    tag: u64,
    value: u64,
    flags_byte: u64,
) -> Result<CellRedefine, *const u8> {
    let differs = |present: u64, desc: u64, cur: u64| {
        flags_byte & present != 0 && (flags_byte & desc != 0) != (cur_flags & cur != 0)
    };
    let writable_change = differs(
        DEFINE_PRESENT_WRITABLE,
        DEFINE_FLAG_WRITABLE,
        BUCKET_FLAG_WRITABLE,
    );
    let enum_change = differs(
        DEFINE_PRESENT_ENUMERABLE,
        DEFINE_FLAG_ENUMERABLE,
        BUCKET_FLAG_ENUMERABLE,
    );
    let config_change = differs(
        DEFINE_PRESENT_CONFIGURABLE,
        DEFINE_FLAG_CONFIGURABLE,
        BUCKET_FLAG_CONFIGURABLE,
    );
    let cur_configurable = cur_flags & BUCKET_FLAG_CONFIGURABLE != 0;
    let has_value = flags_byte & DEFINE_PRESENT_VALUE != 0;
    let incoming_accessor = has_value
        && tag == ANY_HEAP
        && value != 0
        && unsafe { *((value as *const u8).add(4) as *const u16) }
            == crate::accessor::TAG_ACCESSOR_PAIR;
    // `writable: false` on a slot the bare compiled writes go straight
    // into cannot be honoured; an accessor replacing the value slot
    // likewise. Both stay loud (registered tr limitation).
    if writable_change || incoming_accessor {
        return Err(c"Cannot change the attributes of a global variable binding through the global object (not yet supported).".as_ptr() as *const u8);
    }
    // §10.1.6.3 — a non-configurable binding refuses an enumerable /
    // configurable change (the non-cell arm makes the same rejection).
    if (enum_change || config_change) && !cur_configurable {
        return Err(c"Cannot change the attributes of a non-configurable global variable binding through the global object.".as_ptr() as *const u8);
    }
    if has_value {
        let cell = unsafe { __torajs_anyv_cell_ptr(cur_value_anyv) } as *const c_void;
        if !unsafe { global_cell_write(cell, tag, value) } {
            return Err(c"Cannot store a value of this type into a typed global variable binding through the global object (not yet supported).".as_ptr() as *const u8);
        }
    }
    if enum_change || config_change {
        Ok(CellRedefine::RepackFlags)
    } else {
        Ok(CellRedefine::Done)
    }
}

/// `__torajs_global_var_bind`'s "is this name already a cell" probe —
/// the RAW entry value, which `get_tag` / `get_value` no longer show
/// (they see through the cell to the binding's value, which is the
/// point). Answers 0 for an absent key, a non-dynobj, or a plain
/// entry.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header;
/// `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_entry_is_global_cell(
    obj: *const c_void,
    key: *const c_void,
) -> i32 {
    if obj.is_null() || unsafe { crate::get::type_tag(obj) } != crate::layout::TAG_DYNOBJ {
        return 0;
    }
    let p = unsafe { crate::probe::probe(obj, key) };
    if !p.found {
        return 0;
    }
    let v = unsafe { (*crate::probe::entries(obj).add(p.entry as usize)).value_anyv };
    unsafe { value_is_global_cell(v) as i32 }
}
