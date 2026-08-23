//! The reified builtin-proto accessor getter cells — one per proto
//! family (index 0 = Map proto tag 11 / 1 = Set proto tag 12 `get
//! size`, 2 = Symbol proto tag 5 `get description`) because the
//! spec getters are DISTINCT function objects per prototype. Same
//! immortal closure shape as [`super::builtin_method_cell`]; the
//! carried id ([`torajs_rc::ANY_METHOD_GET_SIZE`] /
//! [`torajs_rc::ANY_METHOD_GET_DESCRIPTION`]) routes a
//! `.call(recv)` through the matching dispatcher arm, and `.name` /
//! `.length` reads resolve "get size" / "get description" / 0 off
//! the meta row.

use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use super::{
    CELL_SIZE, CLOSURE_BOXED_ENTRY_OFF, CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF,
    CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF, bare_entry, native_entry,
};

static ACCESSOR_GETTER_CELLS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// The getter cell for a builtin-proto accessor — Map (proto tag
/// 11) / Set (proto tag 12) `size`, Symbol (proto tag 5)
/// `description` — lazily allocated, immortal. Callers guarantee
/// `proto_tag` is one of those three (the
/// `proto_tag_accessor_mid` probe is the gate).
pub(crate) fn proto_accessor_getter_cell(proto_tag: i64) -> *mut u8 {
    let (idx, mid) = match proto_tag {
        11 => (0, torajs_rc::ANY_METHOD_GET_SIZE),
        12 => (1, torajs_rc::ANY_METHOD_GET_SIZE),
        // §25.1.6.13 `get ArrayBuffer.prototype.resizable`.
        19 => (3, torajs_rc::ANY_METHOD_GET_RESIZABLE),
        _ => (2, torajs_rc::ANY_METHOD_GET_DESCRIPTION),
    };
    let slot = &ACCESSOR_GETTER_CELLS[idx];
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
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = mid as u64;
        slot.store(cell as u64, Ordering::Relaxed);
        cell
    }
}
