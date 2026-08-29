//! §25.3 DataView — the byte-granular view over an ArrayBuffer
//! (RFC 20260823-typedarray-substrate 刀 7).
//!
//! ```text
//! { header:8 | buffer:8 (AnyValue) | byte_offset:8 | byte_len:8
//!   | props:8 }                                            (40 B)
//! ```
//!
//! `byte_len == -1` is a **length-tracking** view (§25.3.2 step 10.a
//! `[[ByteLength]] = auto`) whose length is re-derived from the
//! buffer on every access — the same call the TypedArray cell made,
//! for the same reason: a resizable buffer moves under it.
//!
//! Unlike a typed array, a DataView is NOT an integer-indexed exotic
//! object: `dv[0]` is an ordinary property walk and lands in the
//! expando bag, never on the bytes. The bytes are reached only
//! through the §25.3.4 get/set accessor methods (刀 7 second half).

use core::ffi::{c_char, c_void};

use torajs_anyvalue::__torajs_anyv_box_pointer;
use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use torajs_rc::Tag;

use crate::arraybuffer::{NOT_RESIZABLE, byte_len, data_ptr, is_arraybuffer, max_byte_len};

pub(crate) const BUFFER_OFF: usize = 8;
pub(crate) const BYTE_OFFSET_OFF: usize = 16;
pub(crate) const BYTE_LEN_OFF: usize = 24;
/// Lazy expando props dynobj — NULL until the first own-property
/// write / define. Deliberately the SAME offset as the ArrayBuffer
/// cell's (`arraybuffer.rs::PROPS_OFF` = 32), so every bag-face
/// consumer whose per-tag dispatch falls through to "off 32" is
/// already correct for this cell too.
pub(crate) const PROPS_OFF: usize = 32;
const CELL_SIZE: usize = 40;

/// `byte_len` for a view whose length tracks its buffer
/// (§25.3.2 step 10.a's `auto`).
pub(crate) const AUTO_LENGTH: i64 = -1;

unsafe extern "C" {
    /// torajs-cycle — cycle-root buffer push / scrub (rationale in
    /// `torajs-cycle::buffer`). The push is gated on
    /// `has_walkable_children`, so a bagless cell pays a tag test.
    fn __torajs_cycle_buffer(p: *mut c_void);
    fn __torajs_cycle_unbuffer(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    fn __torajs_to_index(n: f64) -> i64;
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
    fn __torajs_anyv_rc_inc(v: AnyValue);
    fn __torajs_anyv_rc_dec(v: AnyValue);
    /// Tag-dispatched heap release — drops the expando props dynobj
    /// without this crate knowing the dynobj layout.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Is `av` a DataView cell? Answers on the heap tag alone.
#[inline]
pub fn is_dataview(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    unsafe { as_void_ptr(av).cast::<u8>().add(4).cast::<u16>().read() == Tag::DataView as u16 }
}

/// # Safety
/// `ptr` is a live DataView cell.
#[inline]
pub(crate) unsafe fn buffer_of(ptr: *mut c_void) -> AnyValue {
    unsafe { (ptr.cast::<u8>().add(BUFFER_OFF) as *const u64).read() }
}

/// # Safety
/// `ptr` is a live DataView cell.
#[inline]
pub(crate) unsafe fn byte_offset_of(ptr: *mut c_void) -> i64 {
    unsafe { (ptr.cast::<u8>().add(BYTE_OFFSET_OFF) as *const i64).read() }
}

/// # Safety
/// `ptr` is a live DataView cell.
#[inline]
pub(crate) unsafe fn stored_byte_len(ptr: *mut c_void) -> i64 {
    unsafe { (ptr.cast::<u8>().add(BYTE_LEN_OFF) as *const i64).read() }
}

/// What the view is RIGHT NOW: its first byte and its byte length,
/// or `None` when the buffer is detached or the view is out of
/// bounds (§25.3.1.2 GetViewByteLength through
/// MakeDataViewWithBufferWitnessRecord).
///
/// The only place that decides a length; every accessor and every
/// get/set method re-asks rather than carry an answer across
/// anything that can run user code.
///
/// # Safety
/// `ptr` is a live DataView cell.
pub(crate) unsafe fn resolve(ptr: *mut c_void) -> Option<(*mut u8, i64)> {
    unsafe {
        let buf = buffer_of(ptr);
        if !is_arraybuffer(buf) {
            return None;
        }
        let bptr = as_void_ptr(buf);
        let data = data_ptr(bptr);
        if data.is_null() {
            return None;
        }
        let buf_len = byte_len(bptr);
        let off = byte_offset_of(ptr);
        if off > buf_len {
            return None;
        }
        let stored = stored_byte_len(ptr);
        let len = if stored == AUTO_LENGTH {
            buf_len - off
        } else {
            if off + stored > buf_len {
                return None;
            }
            stored
        };
        Some((data.add(off as usize), len))
    }
}

/// Mint a view cell over `buffer`, which the cell takes its own
/// reference to. `byte_len` is [`AUTO_LENGTH`] for a tracking view.
///
/// # Safety
/// `buffer` is a live ArrayBuffer AnyValue.
unsafe fn mint(buffer: AnyValue, byte_offset: i64, byte_len: i64) -> AnyValue {
    unsafe {
        __torajs_anyv_rc_inc(buffer);
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::DataView as u16;
        *(cell.add(BUFFER_OFF) as *mut u64) = buffer;
        *(cell.add(BYTE_OFFSET_OFF) as *mut i64) = byte_offset;
        *(cell.add(BYTE_LEN_OFF) as *mut i64) = byte_len;
        __torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// `value_drop`'s DataView arm — release the buffer and free.
///
/// # Safety
/// `cell` is a live DataView cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dataview_drop(cell: *mut c_void) {
    unsafe {
        if torajs_rc::__torajs_rc_dec(cell) == 0 {
            // Still referenced. A live own-property bag makes this cell a
            // potential cycle root — the shape rotation 528 taught the
            // collector to walk, and the reason it can now be reached.
            __torajs_cycle_buffer(cell);
            return;
        }
        __torajs_anyv_rc_dec((cell.cast::<u8>().add(BUFFER_OFF) as *const u64).read());
        let props = (cell.cast::<u8>().add(PROPS_OFF) as *const u64).read() as *mut c_void;
        if !props.is_null() {
            __torajs_value_drop_heap(props);
        }
        // Scrub from the root buffer before the memory goes away: a
        // cell buffered above that later normal-drops to zero would
        // leave a dangling candidate. No-op when never buffered.
        __torajs_cycle_unbuffer(cell);
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        std::alloc::dealloc(cell as *mut u8, layout);
    }
}

/// §25.3.2 `new DataView(buffer [, byteOffset [, byteLength]])`.
///
/// The step order is the thing to preserve: both `ToIndex` coercions
/// run before the buffer is measured, and either can run user code
/// that detaches or resizes it — so the detach check and the range
/// checks read the buffer only after both are done.
///
/// # Safety
/// The argument slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dataview_create(
    buffer: AnyValue,
    a1: AnyValue,
    a2: AnyValue,
) -> AnyValue {
    unsafe {
        // Step 2 — RequireInternalSlot(buffer, [[ArrayBufferData]]).
        if !is_arraybuffer(buffer) {
            __torajs_throw_type_error(
                c"DataView constructor argument is not an ArrayBuffer".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // Step 3 — ToIndex(byteOffset), user code allowed.
        let offset = if a1 == VALUE_UNDEFINED {
            0
        } else {
            let n = __torajs_anyv_to_number(a1);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            let o = __torajs_to_index(n);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            o
        };
        // Steps 8-9 — ToIndex(byteLength) when given, user code
        // allowed here too.
        let explicit_len = if a2 == VALUE_UNDEFINED {
            None
        } else {
            let n = __torajs_anyv_to_number(a2);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            let l = __torajs_to_index(n);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            Some(l)
        };
        // Steps 4-6 — only NOW is the buffer inspected.
        let bptr = as_void_ptr(buffer);
        if data_ptr(bptr).is_null() {
            __torajs_throw_type_error(c"Cannot construct a DataView on a detached buffer".as_ptr());
            return VALUE_UNDEFINED;
        }
        let buf_len = byte_len(bptr);
        if offset > buf_len {
            __torajs_throw_range_error(
                b"start offset is outside the bounds of the buffer\0".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let view_len = match explicit_len {
            // Step 10.a — no length over a resizable buffer tracks it.
            None if max_byte_len(bptr) != NOT_RESIZABLE => AUTO_LENGTH,
            // Step 10.b — a fixed buffer's remainder.
            None => buf_len - offset,
            // Step 11.b-c — an explicit length must fit.
            Some(l) => {
                if offset + l > buf_len {
                    __torajs_throw_range_error(
                        b"length is outside the bounds of the buffer\0".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                l
            }
        };
        mint(buffer, offset, view_len)
    }
}

/// §25.3.4.2 `get DataView.prototype.byteLength` — a TypeError once
/// the buffer is detached or the view is out of bounds (unlike the
/// typed-array getter, which answers 0 there; the two clauses
/// genuinely differ).
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dataview_byte_length(av: AnyValue) -> i64 {
    if !is_dataview(av) {
        unsafe { brand_error() };
        return 0;
    }
    match unsafe { resolve(as_void_ptr(av)) } {
        Some((_, len)) => len,
        None => {
            unsafe { brand_error() };
            0
        }
    }
}

/// §25.3.4.3 `get DataView.prototype.byteOffset` — same TypeError
/// posture as `byteLength`.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dataview_byte_offset(av: AnyValue) -> i64 {
    if !is_dataview(av) {
        unsafe { brand_error() };
        return 0;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        if resolve(ptr).is_none() {
            brand_error();
            return 0;
        }
        byte_offset_of(ptr)
    }
}

/// §25.3.4.1 `get DataView.prototype.buffer` — OWNED; answers even
/// over a detached buffer (the getter has no bounds check).
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dataview_buffer(av: AnyValue) -> AnyValue {
    if !is_dataview(av) {
        unsafe { brand_error() };
        return VALUE_UNDEFINED;
    }
    unsafe {
        let b = buffer_of(as_void_ptr(av));
        __torajs_anyv_rc_inc(b);
        b
    }
}

/// §25.3.4's shared brand / bounds TypeError.
unsafe fn brand_error() {
    unsafe {
        __torajs_throw_type_error(
            c"DataView operation called on an invalid or out-of-bounds view".as_ptr(),
        );
    }
}
