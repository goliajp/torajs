//! Array custom-property printing — the `[ 1, 2, x: 5 ]` props face.
//!
//! bun prints an array's non-index (side-table) properties after the
//! elements, in **insertion order**, as `, key: value` pairs before
//! the closing bracket. Values dispatch on the NaN-box AnyValue tag;
//! nested dynobj values render as bun's multi-line block:
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
//! (props indent = 2 + 2×depth, closing brace indent = 2×depth,
//! trailing comma after every prop — bun ground truth 2026-06-13.)
//!
//! Empty arrays print `[]` with no props face (bun ground truth), so
//! [`crate::print::__torajs_arr_print_i64`]-family's empty early-exit
//! is already correct — this hook only fires on the non-empty path.
//!
//! Insertion order comes from `torajs-dynobj`'s dense entry array via
//! the `__torajs_dynobj_iter_*` externs (holes → NULL key, skipped).
//! Non-enumerable properties are filtered here, caller-side, per the
//! iter contract.
//!
//! Heap values other than Str / DynObj fall back to `[object]` — the
//! same bar as `torajs-anyvalue::inspect`'s `__torajs_print_anyv`
//! (heap-typed pretty-print is a later wedge).

use core::ffi::c_void;

use torajs_rc::{AnySlotTag, Tag};

use crate::print::{put_byte, put_bytes, put_snprintf_f64_g, put_snprintf_i64, put_str_payload};

// Str layout mirror (same constants print.rs duplicates — see its
// header note on the Layer-3 sibling-dep avoidance pattern).
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
const STR_FLAG_IS_LATIN1: u16 = 0x0002;
const HDR_FLAGS_OFF: usize = 6;
const HDR_TYPE_TAG_OFF: usize = 4;

/// `enumerable` flag — bit 1 of the dynobj entry's W/E/C flags
/// (mirror of torajs-dynobj's `BUCKET_FLAG_ENUMERABLE`).
const DYNOBJ_FLAG_ENUMERABLE: u64 = 1 << 1;

/// Heap-header flags bit marking a null-prototype dynobj (mirror of
/// torajs-dynobj's `DYNOBJ_HDR_FLAG_NULL_PROTO`) — regex `.groups`
/// dicts; drives bun's `[Object: null prototype] ` print prefix.
const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

unsafe extern "C" {
    /// torajs-dynobj — iteration surface. `iter_order` materializes
    /// the ES §10.1.11.1 visit sequence (array-index keys ascending
    /// first, then insertion order; holes excluded).
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    /// torajs-anyvalue — NaN-box AnyValue pair decoders.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-mmalloc libc-compat pair (crate-wide idiom, grow.rs) —
    /// the visit-order buffer is a per-print cold-path allocation.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn libc_free(p: *mut c_void);
}

/// Emit the props face for `arr` between its last element and the
/// ` ]` suffix: `, key: value` per live enumerable entry, insertion
/// order. No-op when the array never had a property written.
///
/// # Safety
/// `arr` is a live array heap pointer.
pub(crate) unsafe fn put_arrprops(arr: *mut c_void) {
    let dynobj = unsafe { crate::props::dynobj_of(arr) };
    if dynobj.is_null() {
        return;
    }
    let len = unsafe { __torajs_dynobj_iter_len(dynobj) };
    if len == 0 {
        return;
    }
    // ES §10.1.11.1 visit order (L3b #17) — holes pre-excluded.
    let order = unsafe { malloc(len as usize * 8) } as *mut u64;
    let n = unsafe { __torajs_dynobj_iter_order(dynobj, order, len) };
    for j in 0..n {
        let i = unsafe { *order.add(j as usize) };
        let key = unsafe { __torajs_dynobj_iter_key(dynobj, i) };
        if unsafe { __torajs_dynobj_iter_flags(dynobj, i) } & DYNOBJ_FLAG_ENUMERABLE == 0 {
            continue;
        }
        unsafe {
            put_bytes(b", ");
            put_str_raw(key as *const u8);
            put_bytes(b": ");
            put_anyv_inline(__torajs_dynobj_iter_value(dynobj, i), 1);
        }
    }
    unsafe { libc_free(order as *mut c_void) };
}

/// Emit a Str's payload bytes (encoding-aware: Latin-1 passthrough /
/// supplement expansion, UTF-16 LE transcode) with no quoting.
///
/// # Safety
/// `s` is a live Str heap pointer.
unsafe fn put_str_raw(s: *const u8) {
    unsafe {
        let length = *(s.add(STR_LEN_OFF) as *const u32);
        let flags = *(s.add(HDR_FLAGS_OFF) as *const u16);
        let is_latin1 = (flags & STR_FLAG_IS_LATIN1) != 0;
        let byte_cnt = if is_latin1 {
            length as usize
        } else {
            (length as usize) * 2
        };
        if byte_cnt > 0 {
            let bytes = core::slice::from_raw_parts(s.add(STR_DATA_OFF), byte_cnt);
            put_str_payload(bytes, is_latin1);
        }
    }
}

/// Emit one property value, dispatching on the NaN-box AnyValue tag.
/// `depth` is the nesting level for dynobj block indentation (the
/// array props face itself is depth 1).
///
/// # Safety
/// `anyv` is a NaN-box AnyValue whose Heap pointee (if any) is live.
unsafe fn put_anyv_inline(anyv: u64, depth: usize) {
    let tag = unsafe { __torajs_anyv_unbox_tag(anyv) } as u64;
    unsafe {
        if tag == AnySlotTag::Null as u64 {
            put_bytes(b"null");
        } else if tag == AnySlotTag::Undef as u64 {
            put_bytes(b"undefined");
        } else if tag == AnySlotTag::Bool as u64 {
            let v = __torajs_anyv_unbox_value(anyv);
            put_bytes(if v != 0 { b"true" } else { b"false" });
        } else if tag == AnySlotTag::I64 as u64 {
            put_snprintf_i64(__torajs_anyv_unbox_value(anyv));
        } else if tag == AnySlotTag::F64 as u64 {
            let d = f64::from_bits(__torajs_anyv_unbox_value(anyv) as u64);
            if d.is_nan() {
                put_bytes(b"NaN");
            } else if d == f64::INFINITY {
                put_bytes(b"Infinity");
            } else if d == f64::NEG_INFINITY {
                put_bytes(b"-Infinity");
            } else {
                put_snprintf_f64_g(d);
            }
        } else {
            // AnySlotTag::Heap — dispatch on the pointee's type_tag.
            let p = __torajs_anyv_unbox_value(anyv) as *const u8;
            let t = *(p.add(HDR_TYPE_TAG_OFF) as *const u16);
            if t == Tag::Str as u16 {
                put_byte(b'"');
                put_str_raw(p);
                put_byte(b'"');
            } else if t == Tag::DynObj as u16 {
                put_dynobj_block(p as *const c_void, depth);
            } else {
                // Heap pretty-print for other tags is a later wedge —
                // same fallback bar as __torajs_print_anyv.
                put_bytes(b"[object]");
            }
        }
    }
}

/// Emit a nested dynobj value as bun's multi-line block (see module
/// doc for the indent rule). Empty (no live enumerable entries) →
/// `{}`.
///
/// # Safety
/// `obj` is a live dynobj heap pointer.
unsafe fn put_dynobj_block(obj: *const c_void, depth: usize) {
    let hdr_flags = unsafe { *((obj as *const u8).add(HDR_FLAGS_OFF) as *const u16) };
    if hdr_flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
        unsafe { put_bytes(b"[Object: null prototype] ") };
    }
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let enumerable = |i: u64| -> bool {
        (unsafe { __torajs_dynobj_iter_flags(obj, i) }) & DYNOBJ_FLAG_ENUMERABLE != 0
    };
    // ES §10.1.11.1 visit order (L3b #17) — holes pre-excluded.
    let order = if len > 0 {
        unsafe { malloc(len as usize * 8) as *mut u64 }
    } else {
        core::ptr::null_mut()
    };
    let n = unsafe { __torajs_dynobj_iter_order(obj, order, len) };
    let idx = |j: u64| -> u64 { unsafe { *order.add(j as usize) } };
    if !(0..n).any(|j| enumerable(idx(j))) {
        if !order.is_null() {
            unsafe { libc_free(order as *mut c_void) };
        }
        unsafe { put_bytes(b"{}") };
        return;
    }
    unsafe {
        put_byte(b'{');
        for j in 0..n {
            let i = idx(j);
            if !enumerable(i) {
                continue;
            }
            put_byte(b'\n');
            for _ in 0..(2 + 2 * depth) {
                put_byte(b' ');
            }
            put_str_raw(__torajs_dynobj_iter_key(obj, i) as *const u8);
            put_bytes(b": ");
            put_anyv_inline(__torajs_dynobj_iter_value(obj, i), depth + 1);
            put_byte(b',');
        }
        put_byte(b'\n');
        for _ in 0..(2 * depth) {
            put_byte(b' ');
        }
        put_byte(b'}');
        libc_free(order as *mut c_void);
    }
}
