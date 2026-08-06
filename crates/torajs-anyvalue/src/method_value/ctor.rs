//! Constructor-value family — the per-builtin-family interned
//! `constructor` cells (rotation 131) plus the L3b ④ value faces:
//! the bare namespace ident read (`Object` as a VALUE) and the
//! `.constructor` reify probe. Split from `method_value.rs`
//! (file-size HARD RULE); parent-private layout consts and the
//! throwing entries resolve through `super::`.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use crate::nanbox::AnyValue;

use super::{
    CELL_SIZE, CLOSURE_BOXED_ENTRY_OFF, CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF,
    CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF, bare_entry, native_entry,
};

/// Per-proto-tag interned `constructor` cells (rotation 131 —
/// the gOPD 15.2.3.3-4 constructor family). `<Ctor>.prototype
/// .constructor` must answer ONE identity per builtin so
/// `desc.value === Date.prototype.constructor` holds; the cell is
/// the same immortal closure shape as [`builtin_method_cell`]
/// (calling it is the bare-receiver TypeError — constructing
/// through a first-class ctor value is a recorded follow-up).
static CTOR_CELLS: [AtomicU64; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS] =
    [const { AtomicU64::new(0) }; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS];

/// The interned `constructor` cell for a builtin proto tag —
/// lazily allocated, immortal.
pub(crate) fn builtin_ctor_cell(proto_tag: i64) -> *mut u8 {
    let slot = &CTOR_CELLS[proto_tag as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    // SAFETY: fresh CELL_SIZE allocation, fully initialized below —
    // same layout as builtin_method_cell.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = bare_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = torajs_rc::ANY_METHOD_UNKNOWN as u64;
        slot.store(cell as u64, Ordering::Relaxed);
        cell
    }
}

/// L3b ④ — the boxed builtin-constructor value for the bare
/// namespace ident read (`Object` / `Array` / `Number` … as a
/// VALUE: `o.constructor === Object`). Immortal interned cell —
/// the box is a pure bit-encode over the static-flagged cell, rc
/// traffic no-ops. The compiler gates `proto_tag` to the builtin
/// family range.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_builtin_ctor_value(proto_tag: i64) -> u64 {
    crate::nanbox::box_void_ptr(builtin_ctor_cell(proto_tag) as *mut core::ffi::c_void)
}

/// `.constructor` through the reify surface (L3b ④) — the
/// receiver's builtin constructor per its family tag, the same
/// interned identity the prototype's own `constructor` entry and
/// the bare ident read answer. `None` for non-constructor keys and
/// for receivers whose constructor rides another channel (a struct
/// instance walks its class prototype chain; the own-face shadow
/// probes already ran in every member-get arm).
pub(crate) unsafe fn ctor_cell_for_recv(recv: AnyValue, key: *const c_void) -> Option<*mut u8> {
    if !unsafe { crate::prop_has::key_is(key, b"constructor") } {
        return None;
    }
    let tag = ctor_family_tag(recv)?;
    Some(builtin_ctor_cell(tag))
}

/// Reverse lookup: the proto tag whose interned ctor cell is `cell`,
/// `None` for every other pointer. Linear over ≤16 slots — gOPD /
/// reflection cold path only (RFC 20260720-ctor-static-reflection
/// 刀 2).
pub(crate) fn ctor_tag_of_cell(cell: *const c_void) -> Option<i64> {
    CTOR_CELLS
        .iter()
        .position(|slot| slot.load(Ordering::Relaxed) == cell as u64 && !cell.is_null())
        .map(|i| i as i64)
}

/// ES `name` / `length` of a builtin constructor (刀 3) — the
/// torajs-rc single-source table (the lowering's ctor-namespace
/// member fold reads the same rows).
pub(super) fn ctor_meta(proto_tag: i64) -> Option<(&'static str, u32)> {
    torajs_rc::builtin_proto::builtin_ctor_meta(proto_tag)
}

/// Per-proto-tag interned `.name` Str cells (刀 3) — same immortal
/// shape as the method-name intern table.
static CTOR_NAME_CELLS: [AtomicU64; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS] =
    [const { AtomicU64::new(0) }; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS];

/// The interned `.name` Str cell of a ctor cell — lazily minted,
/// immortal.
pub(super) fn ctor_name_cell(proto_tag: i64) -> Option<*mut u8> {
    let (name, _) = ctor_meta(proto_tag)?;
    let slot = &CTOR_NAME_CELLS[proto_tag as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return Some(p as *mut u8);
    }
    let cell = super::mint_immortal_str(name.as_bytes());
    slot.store(cell as u64, Ordering::Relaxed);
    Some(cell)
}

/// The builtin-prototype family tag of a receiver (`torajs-rc
/// builtin_proto` order: Number 0 / Object 1 / Array 2 / String 3 /
/// Boolean 4 / Symbol 5 / BigInt 6 / RegExp 7 / Date 8 / Error 9 /
/// Promise 10 / Map 11 / Set 12 / Function 13). Wrapper cells
/// answer their inner primitive's family. `None` for struct
/// instances (class chain) and null/undefined (TypeError upstream).
fn ctor_family_tag(recv: AnyValue) -> Option<i64> {
    use crate::nanbox::{is_bool, is_cell, is_double, is_int32, is_short_str};
    if is_short_str(recv) {
        return Some(3);
    }
    if is_int32(recv) || is_double(recv) {
        return Some(0);
    }
    if is_bool(recv) {
        return Some(4);
    }
    if !is_cell(recv) {
        return None;
    }
    let ptr = crate::nanbox::as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live heap header.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    match tag {
        t if t == Tag::Str as u16 => Some(3),
        t if t == Tag::Arr as u16 => Some(2),
        t if t == Tag::DynObj as u16 => Some(1),
        // RFC 20260721 刀 4 — an async-form cell (FLAG_FN_ASYNC,
        // bit 7 Closure-private) reflects %AsyncFunction% (§27.7.1);
        // every other closure keeps the Function ctor.
        t if t == Tag::Closure as u16 => {
            let flags = unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() };
            if flags & torajs_rc::FLAG_FN_ASYNC != 0 {
                Some(14)
            } else {
                Some(13)
            }
        }
        t if t == Tag::BigInt as u16 => Some(6),
        t if t == Tag::Symbol as u16 => Some(5),
        t if t == Tag::RegExp as u16 => Some(7),
        t if t == Tag::Date as u16 => Some(8),
        t if t == Tag::Map as u16 => Some(11),
        t if t == Tag::Set as u16 => Some(12),
        t if t == Tag::WeakMap as u16 => Some(16),
        t if t == Tag::WeakSet as u16 => Some(17),
        t if t == Tag::WeakRef as u16 => Some(18),
        t if t == Tag::Promise as u16 => Some(10),
        t if t == Tag::NumberWrapper as u16 => Some(0),
        t if t == Tag::StringWrapper as u16 => Some(3),
        t if t == Tag::BooleanWrapper as u16 => Some(4),
        _ => None,
    }
}
