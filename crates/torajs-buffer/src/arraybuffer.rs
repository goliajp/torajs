//! §25.1 ArrayBuffer — the byte store every view in this crate sits
//! on (RFC 20260823-typedarray-substrate 刀 1).
//!
//! ```text
//! { header:8 | data:8 | byte_len:8 | max_byte_len:8 | props:8 }   (40 B)
//! ```
//!
//! `data == null` IS detached and `max_byte_len == -1` IS "has no
//! `[[ArrayBufferMaxByteLength]]`". Both are states the spec names
//! outright, so neither gets a flag byte that could drift away from
//! the thing it is supposed to describe — the same call the Proxy
//! cell made for revocation.
//!
//! A resizable buffer allocates its **maximum** at construction and
//! `resize` only moves `byte_len`. Every live view therefore keeps a
//! valid data pointer across a resize, which is what lets §10.4.5
//! re-derive a length on every single access without that being a
//! reallocation hazard.

use core::ffi::{c_char, c_void};

use torajs_anyvalue::__torajs_anyv_box_pointer;
use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use torajs_rc::Tag;

pub(crate) const DATA_OFF: usize = 8;
pub(crate) const BYTE_LEN_OFF: usize = 16;
pub(crate) const MAX_BYTE_LEN_OFF: usize = 24;
/// Lazy expando props dynobj — NULL until the first own-property
/// write / define against the buffer (§10.1 ordinary object face;
/// mirror of torajs-anyvalue `member_get_layout` and torajs-dynobj
/// `layout.rs::ARRAYBUFFER_PROPS_OFF`). `alloc_zeroed` seeds it.
pub(crate) const PROPS_OFF: usize = 32;
const CELL_SIZE: usize = 40;

/// `max_byte_len` when the object has no `[[ArrayBufferMaxByteLength]]`.
pub(crate) const NOT_RESIZABLE: i64 = -1;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    /// §7.1.22 ToIndex, over the Number the step reads. Records a
    /// RangeError and answers 0 when the value is out of range.
    fn __torajs_to_index(n: f64) -> i64;
    /// §7.1.4 ToNumber over an arbitrary value (can run user code
    /// through `valueOf` / `Symbol.toPrimitive`, hence the throw
    /// check at every call site).
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
    /// The generic [[Get]], owned result — how this crate reads an
    /// options bag without duplicating the property walk.
    fn __torajs_any_member_get_with_receiver(
        target: AnyValue,
        key: *const c_void,
        receiver: AnyValue,
    ) -> AnyValue;
    fn __torajs_anyv_rc_dec(v: AnyValue);
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// Tag-dispatched heap release — how the expando props dynobj is
    /// dropped without this crate knowing the dynobj layout.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Is `av` an ArrayBuffer cell? Answers on the heap tag alone.
#[inline]
pub fn is_arraybuffer(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    unsafe { as_void_ptr(av).cast::<u8>().add(4).cast::<u16>().read() == Tag::ArrayBuffer as u16 }
}

/// `[[ArrayBufferData]]` — null exactly when the buffer is detached.
///
/// # Safety
/// `ptr` is a live ArrayBuffer cell.
#[inline]
pub(crate) unsafe fn data_ptr(ptr: *mut c_void) -> *mut u8 {
    unsafe { (ptr.cast::<u8>().add(DATA_OFF) as *const *mut u8).read() }
}

/// `[[ArrayBufferByteLength]]`.
///
/// # Safety
/// `ptr` is a live ArrayBuffer cell.
#[inline]
pub(crate) unsafe fn byte_len(ptr: *mut c_void) -> i64 {
    unsafe { (ptr.cast::<u8>().add(BYTE_LEN_OFF) as *const i64).read() }
}

/// `[[ArrayBufferMaxByteLength]]`, or [`NOT_RESIZABLE`] when absent.
///
/// # Safety
/// `ptr` is a live ArrayBuffer cell.
#[inline]
pub(crate) unsafe fn max_byte_len(ptr: *mut c_void) -> i64 {
    unsafe { (ptr.cast::<u8>().add(MAX_BYTE_LEN_OFF) as *const i64).read() }
}

/// §25.1.3.1 AllocateArrayBuffer, minus the prototype plumbing.
/// `max` is [`NOT_RESIZABLE`] for a fixed-length buffer; otherwise
/// the whole maximum is reserved now (see the module note).
///
/// Answers a raw cell pointer, or null when the reservation fails —
/// callers turn that into the §25.1.3.1 step 4 RangeError.
pub(crate) fn allocate(len: i64, max: i64) -> *mut c_void {
    let reserve = if max == NOT_RESIZABLE { len } else { max };
    // A zero-length buffer is legal and still needs a distinguishable
    // non-null `data` — null means detached, and a fresh buffer is
    // not detached. One byte is the cheapest way to keep that
    // predicate honest.
    let alloc_bytes = if reserve == 0 { 1 } else { reserve as usize };
    unsafe {
        let Ok(data_layout) = core::alloc::Layout::from_size_align(alloc_bytes, 8) else {
            return core::ptr::null_mut();
        };
        let data = std::alloc::alloc_zeroed(data_layout);
        if data.is_null() {
            return core::ptr::null_mut();
        }
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::ArrayBuffer as u16;
        *(cell.add(DATA_OFF) as *mut *mut u8) = data;
        *(cell.add(BYTE_LEN_OFF) as *mut i64) = len;
        *(cell.add(MAX_BYTE_LEN_OFF) as *mut i64) = max;
        cell as *mut c_void
    }
}

/// How many bytes `allocate` reserved for a cell — the figure
/// `dealloc` has to give back, which is the *maximum* for a resizable
/// buffer and not its current length.
///
/// # Safety
/// `ptr` is a live ArrayBuffer cell.
#[inline]
unsafe fn reserved_bytes(ptr: *mut c_void) -> usize {
    unsafe {
        let max = max_byte_len(ptr);
        let reserve = if max == NOT_RESIZABLE {
            byte_len(ptr)
        } else {
            max
        };
        if reserve == 0 { 1 } else { reserve as usize }
    }
}

/// §25.1.3.1 GetArrayBufferMaxByteLengthOption. A non-object
/// `options`, or one whose `maxByteLength` is `undefined`, is
/// *absent* — [`NOT_RESIZABLE`] — which is a different answer from a
/// maximum of zero.
///
/// # Safety
/// `options` is a live AnyValue.
unsafe fn max_byte_length_option(options: AnyValue) -> i64 {
    if !is_cell(options) {
        return NOT_RESIZABLE;
    }
    unsafe {
        let key = __torajs_str_alloc(b"maxByteLength".as_ptr(), 13) as *const c_void;
        let got = __torajs_any_member_get_with_receiver(options, key, options);
        __torajs_str_drop(key as *mut c_void);
        if __torajs_throw_check() != 0 {
            return NOT_RESIZABLE;
        }
        if got == VALUE_UNDEFINED {
            return NOT_RESIZABLE;
        }
        let n = __torajs_anyv_to_number(got);
        __torajs_anyv_rc_dec(got);
        if __torajs_throw_check() != 0 {
            return NOT_RESIZABLE;
        }
        __torajs_to_index(n)
    }
}

/// §25.1.4.1 `new ArrayBuffer(length [, options])`. Arguments are
/// BORROWED; the answer is an owned cell boxed as `any`.
///
/// The two coercions run in spec order (`ToIndex(length)` before the
/// options read) because both can call user code, and a case that
/// counts the order of its own side effects is the only way anyone
/// notices.
///
/// # Safety
/// `length` and `options` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_create(
    length: AnyValue,
    options: AnyValue,
) -> AnyValue {
    unsafe {
        let n = __torajs_anyv_to_number(length);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let len = __torajs_to_index(n);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let max = max_byte_length_option(options);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        if max != NOT_RESIZABLE && len > max {
            __torajs_throw_range_error(
                b"ArrayBuffer length exceeds the specified maxByteLength\0".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let cell = allocate(len, max);
        if cell.is_null() {
            __torajs_throw_range_error(b"ArrayBuffer allocation failed\0".as_ptr());
            return VALUE_UNDEFINED;
        }
        __torajs_anyv_box_pointer(cell)
    }
}

/// §25.1.3.3 DetachArrayBuffer — the byte store goes back and the
/// slot becomes null, which IS the detached state (there is nothing
/// else to read, so there is nothing else to set).
///
/// Every view over this buffer keeps its reference to the CELL, so
/// they all learn at once: `resolve` reads a null `data` and answers
/// "detached" from that moment. Idempotent — a second detach has
/// nothing to give back.
///
/// # Safety
/// `ptr` is a live ArrayBuffer cell.
pub(crate) unsafe fn detach(ptr: *mut c_void) {
    unsafe {
        let data = data_ptr(ptr);
        if data.is_null() {
            return;
        }
        // The reservation is read BEFORE the length is cleared: a
        // resizable buffer gave back its maximum, not its current
        // length, and `reserved_bytes` computes that from the two
        // fields below.
        let n = reserved_bytes(ptr);
        let layout = core::alloc::Layout::from_size_align(n, 8).unwrap();
        std::alloc::dealloc(data, layout);
        *(ptr.cast::<u8>().add(DATA_OFF) as *mut *mut u8) = core::ptr::null_mut();
        *(ptr.cast::<u8>().add(BYTE_LEN_OFF) as *mut i64) = 0;
    }
}

/// `value_drop`'s ArrayBuffer arm — release the byte store and free.
///
/// # Safety
/// `cell` is a live ArrayBuffer cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_drop(cell: *mut c_void) {
    unsafe {
        if torajs_rc::__torajs_rc_dec(cell) == 0 {
            return;
        }
        let data = data_ptr(cell);
        if !data.is_null() {
            let n = reserved_bytes(cell);
            let data_layout = core::alloc::Layout::from_size_align(n, 8).unwrap();
            std::alloc::dealloc(data, data_layout);
        }
        let props = (cell.cast::<u8>().add(PROPS_OFF) as *const u64).read() as *mut c_void;
        if !props.is_null() {
            __torajs_value_drop_heap(props);
        }
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        std::alloc::dealloc(cell.cast::<u8>(), layout);
    }
}

/// §25.1.6.1 `get ArrayBuffer.prototype.byteLength` — a detached
/// buffer answers 0 rather than throwing.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_byte_length(av: AnyValue) -> i64 {
    if !is_arraybuffer(av) {
        unsafe {
            __torajs_throw_type_error(
                c"ArrayBuffer.prototype.byteLength called on incompatible receiver".as_ptr(),
            )
        };
        return 0;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        if data_ptr(ptr).is_null() {
            return 0;
        }
        byte_len(ptr)
    }
}

/// §25.1.6.4 `get ArrayBuffer.prototype.maxByteLength` — detached is
/// 0, resizable is the maximum, and a fixed-length buffer answers its
/// own byte length (it cannot grow, so that IS its maximum).
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_max_byte_length(av: AnyValue) -> i64 {
    if !is_arraybuffer(av) {
        unsafe {
            __torajs_throw_type_error(
                c"ArrayBuffer.prototype.maxByteLength called on incompatible receiver".as_ptr(),
            )
        };
        return 0;
    }
    unsafe {
        let ptr = as_void_ptr(av);
        if data_ptr(ptr).is_null() {
            return 0;
        }
        let max = max_byte_len(ptr);
        if max == NOT_RESIZABLE {
            byte_len(ptr)
        } else {
            max
        }
    }
}

/// §25.1.6.3 `get ArrayBuffer.prototype.resizable`.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_resizable(av: AnyValue) -> i64 {
    if !is_arraybuffer(av) {
        unsafe {
            __torajs_throw_type_error(
                c"ArrayBuffer.prototype.resizable called on incompatible receiver".as_ptr(),
            )
        };
        return 0;
    }
    i64::from(unsafe { max_byte_len(as_void_ptr(av)) } != NOT_RESIZABLE)
}

/// §25.1.6.2 `get ArrayBuffer.prototype.detached`.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_detached(av: AnyValue) -> i64 {
    if !is_arraybuffer(av) {
        unsafe {
            __torajs_throw_type_error(
                c"ArrayBuffer.prototype.detached called on incompatible receiver".as_ptr(),
            )
        };
        return 0;
    }
    i64::from(unsafe { data_ptr(as_void_ptr(av)) }.is_null())
}

/// §25.1.5.1 `ArrayBuffer.isView(arg)` — true for a TypedArray or a
/// DataView, which is a question about the argument's tag and not
/// about any buffer.
///
/// # Safety
/// `av` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_is_view(av: AnyValue) -> i64 {
    if !is_cell(av) {
        return 0;
    }
    let tag = unsafe { as_void_ptr(av).cast::<u8>().add(4).cast::<u16>().read() };
    i64::from(tag == Tag::TypedArray as u16 || tag == Tag::DataView as u16)
}
