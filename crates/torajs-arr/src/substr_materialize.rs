//! Two ways to turn the substring VIEWS in an array's slots into
//! owned strings — the runtime half of the rule that a view may only
//! live in the split block that owns its storage.
//!
//! `s.split(sep)` answers one block: the array header, N pointer
//! slots, and N inline 32-byte view cells those slots point at. A
//! view reads its text through its parent string and its storage is
//! the block's tail, so it is exactly as alive as the block. Every
//! copy of a slot into some *other* container — `a.slice(1)`,
//! `a.concat(b)`, `[...a]`, `a.filter(f)`, `Array.from(a)`,
//! `out[i] = a[j]` — used to copy the pointer and bump the view
//! cell's own refcount, which the inline drop path does not read: it
//! only releases the parent. Once the split block went away the copy
//! pointed at reclaimed pool memory, and the program printed whatever
//! the next allocation had written there (rotation 468 probes:
//! `["4","q!","xy"]` for `[...a, "xy"]`). The fix is that a copy out
//! of the block is an owned string, and the copied array is typed
//! `Arr<Str>` from then on.
//!
//! The two entry points differ only in what the slot already owed:
//!
//! - [`__torajs_arr_substr_adopt_copied`] — the slots were just
//!   memcpy'd from another array and own nothing yet (the position
//!   `ssa_lower` used to fill with an rc-inc walk). A view slot is
//!   replaced by a fresh owned string; an owned string is shared by
//!   one refcount; nothing is released, because the source still
//!   holds every reference it had.
//! - [`__torajs_arr_substr_materialize_owned`] — the slots already
//!   own their references: this is the split product itself, whose
//!   inline views each hold one reference on the parent (the split
//!   kernel's `PARENT_RC`). A view slot is replaced by a fresh owned
//!   string and the view is released, which hands that parent
//!   reference back. Used when the program is going to write owned
//!   strings INTO the array, which an `Arr<Substr>` slot cannot take.
//!
//! Both leave null slots (holes) and the immortal `undefined`
//! sentinel alone: `substr_to_owned` answers the Str-shaped sentinel
//! for the Substr-shaped one, and the drop path returns at once on
//! its static flag. Owned strings already in a slot (an array that
//! mixed the two before this module existed cannot exist any more,
//! but a `Str` array handed to the "owned" entry by a conservative
//! caller can) pass through untouched.

use core::ffi::c_void;

use torajs_rc::{__torajs_rc_inc, HeapHeader};
use torajs_str::substr::{FLAG_SUBSTR_INLINE, FLAG_SUBSTR_VIEW};

use crate::layout::{ARR_CAP_OFF, arr_data, arr_live_extent};

unsafe extern "C" {
    /// Fresh owned Str with the view's text (torajs-str
    /// `substr_methods.rs`); answers the Str sentinel for the Substr
    /// sentinel.
    fn __torajs_substr_to_owned(v: *const u8) -> *mut c_void;
    /// The definition of releasing one Str-tagged cell, views
    /// included (torajs-str `str_drop.rs`).
    fn __torajs_str_drop(s: *mut c_void);
}

/// True for a substring view of either shape (inline in a split
/// block, or standalone). Owned strings have neither flag.
#[inline]
unsafe fn is_view(cell: *const u8) -> bool {
    let flags = unsafe { (*(cell as *const HeapHeader)).flags };
    flags & (FLAG_SUBSTR_VIEW | FLAG_SUBSTR_INLINE) != 0
}

/// True only for an INLINE view — the shape whose storage belongs to
/// the split block. The any-world entry points of this crate
/// (`arr_extend_typed_into_any`, `arr_get_any_owned`) materialize
/// exactly these; a standalone view owns its cell and is refcounted
/// like any heap value.
#[inline]
pub(crate) unsafe fn is_inline_view(cell: *const u8) -> bool {
    let flags = unsafe { (*(cell as *const HeapHeader)).flags };
    flags & FLAG_SUBSTR_INLINE != 0
}

/// Fresh owned string (rc 1) with the view's text.
#[inline]
pub(crate) unsafe fn view_to_owned(view: *const u8) -> *mut u8 {
    unsafe { __torajs_substr_to_owned(view) as *mut u8 }
}

/// The slot vector, past the deque head (`cap:u32 | head:u32` packed
/// at `ARR_CAP_OFF`, same as `drop_str_elems`).
#[inline]
unsafe fn slots(cell: *mut u8) -> *mut *mut u8 {
    let head = unsafe { *(cell.add(ARR_CAP_OFF + 4) as *const u32) } as usize;
    unsafe { arr_data(cell).add(head * 8) as *mut *mut u8 }
}

/// Make the slots in `[start, end)` own their elements, materializing
/// every view into an owned string on the way. Replaces the rc-inc
/// walk after a slot memcpy (`arr_slice` / `arr_concat` /
/// `arr_extend` / …) when the source array's element type is
/// `Substr`; after this the array holds only owned strings and is
/// typed `Arr<Str>`.
///
/// # Safety
///
/// `p` is null or a live array cell whose slots in `[start, end)`
/// hold `Str`-tagged pointers (owned strings or views) that the array
/// does NOT yet own a reference to. `start <= end <= live extent`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_substr_adopt_copied(p: *mut c_void, start: i64, end: i64) {
    if p.is_null() || start >= end {
        return;
    }
    let slots = unsafe { slots(p as *mut u8) };
    for i in start as usize..end as usize {
        let elem = unsafe { *slots.add(i) };
        if elem.is_null() {
            continue;
        }
        if unsafe { is_view(elem) } {
            unsafe { *slots.add(i) = __torajs_substr_to_owned(elem) as *mut u8 };
        } else {
            unsafe { __torajs_rc_inc(elem as *mut c_void) };
        }
    }
}

/// Materialize every view slot of an array that already owns its
/// elements — the split product that is about to receive owned
/// writes. Each view is released after its text is copied out, which
/// returns the parent reference the split kernel took for it. Answers
/// the same array, so the SSA site can rebind it under its new
/// `Arr<Str>` type (the shape `arr_push` and friends use).
///
/// # Safety
///
/// `p` is null or a live array cell whose slots hold `Str`-tagged
/// pointers the array owns one reference to each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_substr_materialize_owned(p: *mut c_void) -> *mut c_void {
    if p.is_null() {
        return p;
    }
    let cell = p as *mut u8;
    let extent = unsafe { arr_live_extent(cell) } as usize;
    let slots = unsafe { slots(cell) };
    for i in 0..extent {
        let elem = unsafe { *slots.add(i) };
        if elem.is_null() || !unsafe { is_view(elem) } {
            continue;
        }
        let owned = unsafe { __torajs_substr_to_owned(elem) } as *mut u8;
        unsafe {
            *slots.add(i) = owned;
            __torajs_str_drop(elem as *mut c_void);
        }
    }
    p
}
