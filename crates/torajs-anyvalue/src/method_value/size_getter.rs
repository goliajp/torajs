//! The reified `get size` getter cells — one per proto family
//! (index 0 = Map proto tag 11, 1 = Set proto tag 12) because the
//! spec getters are DISTINCT function objects per prototype. Same
//! immortal closure shape as [`super::builtin_method_cell`]; the
//! carried id [`torajs_rc::ANY_METHOD_GET_SIZE`] routes a
//! `.call(recv)` through the mapset dispatcher's size arm, and
//! `.name` / `.length` reads resolve "get size" / 0 off the meta
//! row.

use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use super::{
    CELL_SIZE, CLOSURE_BOXED_ENTRY_OFF, CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF,
    CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF, bare_entry, native_entry,
};

static SIZE_GETTER_CELLS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// The getter cell for a Map (proto tag 11) or Set (proto tag 12)
/// `size` accessor — lazily allocated, immortal. Callers guarantee
/// `proto_tag` is 11 or 12.
pub(crate) fn size_getter_cell(proto_tag: i64) -> *mut u8 {
    let slot = &SIZE_GETTER_CELLS[(proto_tag - 11) as usize];
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
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = torajs_rc::ANY_METHOD_GET_SIZE as u64;
        slot.store(cell as u64, Ordering::Relaxed);
        cell
    }
}
