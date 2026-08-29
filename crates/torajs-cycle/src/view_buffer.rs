//! What a view owns of its buffer, for the trial-deletion walk.
//!
//! §10.4.5's TypedArray and §25.3's DataView each hold one strong
//! reference to the ArrayBuffer they read through — the reference
//! their own destructor releases, with `__torajs_anyv_rc_dec` on the
//! `+8` word, before the cell goes away. Rotation 528 gave all three
//! buffer-family tags their property bag and 529 named this slot as
//! one of the three still unwalked; the bag has been the only child
//! the collector enumerated for a view, so a buffer reachable from
//! the view over it — `const b = new ArrayBuffer(8);
//! const v: any = new DataView(b); (b as any).v = v` — was a
//! two-cell ring nothing could see.
//!
//! Mirror of `torajs-buffer/src/typedarray.rs` and
//! `torajs-buffer/src/dataview.rs`. Both layouts put the buffer
//! directly after the header, so one arm serves both tags:
//!
//! ```text
//! TypedArray (48B)               DataView (40B)
//! offset | field                 offset | field
//! -------|------                 -------|------
//!   0    | heap header             0    | heap header
//!   8    | buffer  — slot 0        8    | buffer  — slot 0
//!  16    | byte_offset            16    | byte_offset
//!  24    | array_len              24    | byte_len
//!  32    | kind + pad             32    | props (the bag)
//!  40    | props (the bag)
//! ```
//!
//! Like [`crate::proxy`]'s two slots and unlike
//! [`crate::iter_src`]'s pair, these are NaN-boxed AnyValues rather
//! than raw cell pointers, so the read carries the immediate filter.
//! Detaching a buffer does not touch this slot — a detached view
//! still names the ArrayBuffer it was made over — so the filter is
//! here for the shape of the word, not for a state it expects.
//!
//! Both tags are [`crate::layout_bag::corpse_takes_own_destructor`]
//! shapes, so a zeroed slot is exactly what the corpse's own
//! destructor needs to see: `__torajs_anyv_rc_dec` on a zero word
//! releases nothing, which is how an edge this collect already
//! accounted for stays accounted for exactly once.

use core::ffi::c_void;

use crate::layout::nan_box_is_cell_like;
use crate::layout_bag::{TAG_DATAVIEW, TAG_TYPEDARRAY};

/// Byte offset of `TypedArray::buffer` / `DataView::buffer`.
const VIEW_BUFFER_OFF: usize = 8;

/// The child index [`crate::children::for_each_child`] yields the
/// buffer under. Neither view cell has an entry table, so index 0 is
/// free — the props bag keeps its own `PROPS_SLOT_INDEX` sentinel.
pub const VIEW_BUFFER_SLOT: u64 = 0;

/// True for the two view cells that own the buffer they read
/// through. An ArrayBuffer owns only its bytes, so it is not one.
#[inline]
pub fn is_view_cell(type_tag: u16) -> bool {
    matches!(type_tag, TAG_TYPEDARRAY | TAG_DATAVIEW)
}

/// The ArrayBuffer this view reads through, or NULL once the slot
/// has been broken by this collect.
///
/// # Safety
/// `p` must be a live TypedArray or DataView cell — check
/// [`is_view_cell`] on its tag first.
#[inline]
pub unsafe fn view_buffer(p: *mut c_void) -> *mut c_void {
    let v = unsafe { *((p as *const u8).add(VIEW_BUFFER_OFF) as *const u64) } as *mut c_void;
    if nan_box_is_cell_like(v) {
        v
    } else {
        core::ptr::null_mut()
    }
}

/// Zero the buffer slot — `collect_white`'s first-sweep cycle break.
///
/// # Safety
/// Same as [`view_buffer`].
#[inline]
pub unsafe fn view_buffer_clear(p: *mut c_void) {
    unsafe { *((p as *mut u8).add(VIEW_BUFFER_OFF) as *mut u64) = 0 };
}
