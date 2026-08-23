//! The `torajs-buffer` printers the three inspect walkers reach
//! (RFC 20260823-typedarray-substrate).
//!
//! Their own file rather than another block in `formatters.rs`: that
//! file is the shared extern surface for every OTHER substrate's
//! printer and was four lines from the size limit when the second of
//! these arrived. The buffer family will keep growing — DataView is
//! still to come — so it gets somewhere to grow.
//!
//! Neither writes a trailing newline; the top-level caller adds one
//! and the nested callers add their own separators.

use core::ffi::c_void;

unsafe extern "C" {
    /// `ArrayBuffer(N) [ … ]` — the bytes, not the maximum, so a
    /// resizable buffer prints like a fixed-length one of the same
    /// current length.
    pub(super) fn __torajs_arraybuffer_print(cell: *const c_void);
    /// `Uint8Array(N) [ … ]` — elements read through the element
    /// type, and the BigInt kinds carry the `n` suffix.
    pub(super) fn __torajs_typedarray_print(cell: *const c_void);
    /// `DataView(N) [ … ]` — the view's bytes; detached or
    /// out-of-bounds prints as length zero.
    pub(super) fn __torajs_dataview_print(cell: *const c_void);
}
