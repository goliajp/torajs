//! Wrapper-subclass instance allocation (RFC
//! 20260730-exotic-backed-class-instance blade 2).
//!
//! `class C extends Number | String | Boolean` mints a REAL wrapper
//! cell — `instanceof Number`, `valueOf`, the primitive coercion
//! surface all come for free because the instance IS a wrapper. The
//! class identity rides blade 0's substrate (`FLAG_SUBCLASSED` +
//! torajs-meta side table), scrubbed by the wrapper drop paths.
//!
//! The mint runs BEFORE the ctor body (the factory's `__this` slot),
//! so each cell starts at its builtin's no-argument default —
//! `[[NumberData]] = +0` (§21.1.1.1), `[[StringData]] = ""`
//! (§22.1.1.1, the NULL inner cell every downstream consumer already
//! reads as empty), `[[BooleanData]] = false` (§20.3.1.1). `super(v)`
//! then applies the builtin ctor's coercion to the already-minted
//! cell — exact, because this-TDZ means nothing observed the default.
//!
//! Each `super` kernel answers a fresh OWNED reference on the
//! instance: `super(v)` sits in statement position and the lowerer
//! releases the discarded any value — answering a borrowed alias
//! would let that release steal the ctor's own reference (the blade-1
//! churn probe caught exactly this shape on Array).

use core::ffi::c_void;

use torajs_rc::{__torajs_rc_inc, FLAG_SUBCLASSED};

use crate::{
    __torajs_boolean_wrapper_new, __torajs_number_wrapper_new, __torajs_string_wrapper_new,
    BOOLEAN_WRAPPER_VALUE_OFF, NUMBER_WRAPPER_VALUE_OFF, STRING_WRAPPER_CELL_OFF,
};

/// `torajs_rc::AnySlotTag` mirrors (same constants the arr subclass
/// kernel carries).
const ANY_HEAP: i64 = 4;

unsafe extern "C" {
    /// torajs-meta — record the fresh instance's class identity
    /// (blade 0). Takes no reference on the proto cell.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
    /// torajs-meta classmeta — the class's registered `__proto_<C>`
    /// AnyValue immediate (0 when unregistered).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    /// torajs-anyvalue — NaN-box encode / decode.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-anyvalue — §7.1 coercions over a boxed any.
    fn __torajs_anyv_to_number(v: u64) -> f64;
    fn __torajs_anyv_to_bool(v: u64) -> bool;
    /// ToString — answers a freshly-owned Str cell (NULL only when a
    /// pending throw was raised, e.g. a Symbol operand).
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    /// Universal drop dispatcher (release the replaced inner cell).
    fn __torajs_value_drop_heap(child: *mut c_void);
}

/// Mark + register the fresh wrapper's class identity and answer it
/// boxed — subclass instances live in the any world (the factory's
/// `__this` is `any`).
unsafe fn mint_common(ptr: *mut u8, class_tag: i64) -> u64 {
    unsafe {
        *(ptr.add(6) as *mut u16) |= FLAG_SUBCLASSED;
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(ptr as *mut c_void, class_tag, proto_cell);
        __torajs_anyv_box_from_pair(ANY_HEAP, ptr as i64)
    }
}

/// Mint a Number-subclass instance (`[[NumberData]] = +0` until
/// `super(v)` runs).
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_number_wrapper_subclass_alloc(class_tag: i64) -> u64 {
    unsafe {
        let ptr = __torajs_number_wrapper_new(0.0);
        mint_common(ptr, class_tag)
    }
}

/// Mint a String-subclass instance (NULL inner cell = `""` until
/// `super(v)` runs).
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_string_wrapper_subclass_alloc(class_tag: i64) -> u64 {
    unsafe {
        let ptr = __torajs_string_wrapper_new(core::ptr::null_mut());
        mint_common(ptr, class_tag)
    }
}

/// Mint a Boolean-subclass instance (`[[BooleanData]] = false` until
/// `super(v)` runs).
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_boolean_wrapper_subclass_alloc(class_tag: i64) -> u64 {
    unsafe {
        let ptr = __torajs_boolean_wrapper_new(0);
        mint_common(ptr, class_tag)
    }
}

/// `super(v)` inside a Number-subclass ctor — `[[NumberData]] =
/// ToNumber(v)` (§21.1.1.1 step 2) applied to the minted instance.
/// Answers a fresh owned reference (see module doc).
///
/// # Safety
/// `this_av` is the factory's freshly minted subclass instance boxed
/// ANY_HEAP (or any non-cell box, answered back unchanged); `val_av`
/// is any boxed value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_number_wrapper_subclass_super(this_av: u64, val_av: u64) -> u64 {
    unsafe {
        let n = __torajs_anyv_to_number(val_av);
        let p = __torajs_anyv_unbox_value(this_av) as *mut u8;
        if p.is_null() {
            return this_av;
        }
        (p.add(NUMBER_WRAPPER_VALUE_OFF) as *mut f64).write(n);
        __torajs_rc_inc(p as *mut c_void);
    }
    this_av
}

/// `super(v)` inside a String-subclass ctor — `[[StringData]] =
/// ToString(v)` (§22.1.1.1 step 2). A pending throw from ToString
/// (Symbol operand) leaves the default and answers the borrowed box
/// un-inc'd for the caller's throw-check to divert past (the arr
/// RangeError shape). Answers a fresh owned reference otherwise.
///
/// # Safety
/// Same contract as [`__torajs_number_wrapper_subclass_super`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_string_wrapper_subclass_super(this_av: u64, val_av: u64) -> u64 {
    unsafe {
        let s = __torajs_anyv_to_str(val_av);
        let p = __torajs_anyv_unbox_value(this_av) as *mut u8;
        if p.is_null() || s.is_null() {
            return this_av;
        }
        let slot = p.add(STRING_WRAPPER_CELL_OFF) as *mut *mut u8;
        let old = slot.read();
        slot.write(s as *mut u8);
        if !old.is_null() {
            __torajs_value_drop_heap(old as *mut c_void);
        }
        __torajs_rc_inc(p as *mut c_void);
    }
    this_av
}

/// `super(v)` inside a Boolean-subclass ctor — `[[BooleanData]] =
/// ToBoolean(v)` (§20.3.1.1 step 2). Answers a fresh owned reference.
///
/// # Safety
/// Same contract as [`__torajs_number_wrapper_subclass_super`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_boolean_wrapper_subclass_super(this_av: u64, val_av: u64) -> u64 {
    unsafe {
        let b = __torajs_anyv_to_bool(val_av);
        let p = __torajs_anyv_unbox_value(this_av) as *mut u8;
        if p.is_null() {
            return this_av;
        }
        (p.add(BOOLEAN_WRAPPER_VALUE_OFF) as *mut u8).write(if b { 1 } else { 0 });
        __torajs_rc_inc(p as *mut c_void);
    }
    this_av
}
