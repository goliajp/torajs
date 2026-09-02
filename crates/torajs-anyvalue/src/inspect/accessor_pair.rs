//! An accessor property's value cell as inspect renders it — the
//! dynobj / expando walkers hand the `Tag::AccessorPair` cell over as
//! the entry value, and bun prints the accessor itself, never the
//! closure: `[Getter]` / `[Setter]` / `[Getter/Setter]`.

use core::ffi::c_void;

use super::formatters::put_bytes;

/// Getter closure @ +8, setter @ +16 (torajs-dynobj `ACC_GET_OFF` /
/// `ACC_SET_OFF`), NULL for an absent half.
const ACC_GET_OFF: usize = 8;
const ACC_SET_OFF: usize = 16;

/// Emit the accessor's inspect form, no trailing newline.
///
/// # Safety
///
/// `cell` is a live `Tag::AccessorPair` heap cell.
pub(super) unsafe fn put_accessor_pair_inline(cell: *const c_void) {
    let get = unsafe { ((cell as *const u8).add(ACC_GET_OFF) as *const *const c_void).read() };
    let set = unsafe { ((cell as *const u8).add(ACC_SET_OFF) as *const *const c_void).read() };
    let form: &[u8] = match (get.is_null(), set.is_null()) {
        (false, false) => b"[Getter/Setter]",
        (false, true) => b"[Getter]",
        _ => b"[Setter]",
    };
    unsafe { put_bytes(form) };
}
