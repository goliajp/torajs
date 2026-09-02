//! Array custom-property printing — the `[ 1, 2, x: 5 ]` props face.
//!
//! bun prints an array's non-index (side-table) properties after the
//! elements as `, key: value` pairs before the closing bracket: the
//! string keys in insertion order, then the Symbol keys (the
//! elements are the index keys, so the face always follows them —
//! `__torajs_dynobj_iter_print_order_after_index`). A key goes
//! through the shared key writer (bare identifier, JSON-quoted
//! otherwise, `[Symbol(desc)]`), a value through the inspect walker
//! at the element indent, so a nested object renders exactly as it
//! would inside the array:
//!
//! ```text
//! [ 9, g: {
//!     y: "b",
//!     d: {
//!       w: 1,
//!     },
//!   } ]
//! ```
//!
//! Empty arrays print `[]` with no props face (bun ground truth), so
//! the element printers' empty early-exit is already correct — this
//! hook only fires on the non-empty path. Holes (NULL key) and
//! non-enumerable entries are skipped here, caller-side, per the
//! iter contract.

use core::ffi::c_void;

use crate::print::put_bytes;

/// `enumerable` flag — bit 1 of the dynobj entry's W/E/C flags
/// (mirror of torajs-dynobj's `BUCKET_FLAG_ENUMERABLE`).
const DYNOBJ_FLAG_ENUMERABLE: u64 = 1 << 1;

unsafe extern "C" {
    /// torajs-dynobj — iteration surface; `iter_print_order_after_index`
    /// materializes bun's print sequence for a face that follows
    /// index keys (strings in insertion order, then symbols; holes
    /// excluded).
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_print_order_after_index(
        obj: *const c_void,
        out: *mut u64,
        cap: u64,
    ) -> u64;
    /// torajs-anyvalue — a Str key bare when it is an ASCII
    /// identifier, JSON-quoted otherwise; a Symbol key as
    /// `[Symbol(desc)]` (the dynobj and struct walkers' key writer).
    fn __torajs_print_str_cell_as_key(cell: *const c_void);
    /// torajs-anyvalue — indent-threaded inline AnyValue printer
    /// (inspect indent trunk), the same walker the elements use.
    fn __torajs_print_anyv_inline_at(v: u64, indent: u32);
    /// torajs-mmalloc libc-compat pair (crate-wide idiom, grow.rs) —
    /// the visit-order buffer is a per-print cold-path allocation.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn libc_free(p: *mut c_void);
}

/// Emit the props face for `arr` between its last element and the
/// ` ]` suffix: `, key: value` per live enumerable entry. `indent`
/// is the element column (the array's indent + 2), which a nested
/// composite value pads from. No-op when the array never had a
/// property written.
///
/// # Safety
/// `arr` is a live array heap pointer.
pub(crate) unsafe fn put_arrprops(arr: *mut c_void, indent: u32) {
    let dynobj = unsafe { crate::props::dynobj_of(arr) };
    if dynobj.is_null() {
        return;
    }
    let len = unsafe { __torajs_dynobj_iter_len(dynobj) };
    if len == 0 {
        return;
    }
    let order = unsafe { malloc(len as usize * 8) } as *mut u64;
    let n = unsafe { __torajs_dynobj_iter_print_order_after_index(dynobj, order, len) };
    for j in 0..n {
        let i = unsafe { *order.add(j as usize) };
        let key = unsafe { __torajs_dynobj_iter_key(dynobj, i) };
        if unsafe { __torajs_dynobj_iter_flags(dynobj, i) } & DYNOBJ_FLAG_ENUMERABLE == 0 {
            continue;
        }
        unsafe {
            put_bytes(b", ");
            __torajs_print_str_cell_as_key(key);
            put_bytes(b": ");
            __torajs_print_anyv_inline_at(__torajs_dynobj_iter_value(dynobj, i), indent);
        }
    }
    unsafe { libc_free(order as *mut c_void) };
}
