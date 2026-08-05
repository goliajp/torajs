//! §20.1.2.9 `Object.getOwnPropertyDescriptors(O)` — the plural of
//! the descriptor read.
//!
//! The singular form has been answering for a long time; the plural
//! had no lowering at all, so it did not merely answer wrong, it
//! refused to compile ("unsupported member call shape"). Nothing new
//! is needed to build it: the own-key walks and the descriptor read
//! both already exist, and the spec's own definition is exactly their
//! composition — OwnPropertyKeys, then [[GetOwnProperty]] per key,
//! collected onto a fresh ordinary object.
//!
//! Both key kinds are walked, because OwnPropertyKeys returns both
//! and a symbol-keyed property is no less own than a string-keyed
//! one. Non-enumerable string keys are included for the same reason —
//! this is the "own" surface, not the enumerable one.

use core::ffi::c_void;

use crate::reflect::{ANY_HEAP, VALUE_NULL_IMM, VALUE_UNDEFINED_IMM, is_cell_imm};

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_anyv_own_keys(v: u64, include_nonenum: i64) -> *mut c_void;
    fn __torajs_anyv_own_symbols(obj_any: u64) -> *mut c_void;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// Reads the in-flight flag WITHOUT clearing it — the caller's
    /// own throw-check still gets to see and propagate the throw.
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// `torajs_arr::layout` mirrors — the length word and the element
/// storage pointer (`obj_own_keys`' twins).
const ARR_LEN_OFF: usize = 8;
const ARR_DATA_PTR_OFF: usize = 32;

/// Drain one own-key array onto `out`, reading each key's descriptor
/// through the singular kernel. The array is consumed: its cells are
/// borrowed for the read (`dynobj_set` takes its own share of the
/// key) and the whole array is released at the end.
///
/// A key whose descriptor comes back `undefined` is skipped rather
/// than stored — the two walks disagree only if a property vanished
/// between them, and §20.1.2.9 step 4.b says to skip exactly that.
///
/// # Safety
/// `keys` is a freshly-minted `Arr` of live key cells (or NULL);
/// `out_slot` points at a live dynobj pointer.
unsafe fn drain_keys(obj_any: u64, keys: *mut c_void, out_slot: *mut *mut c_void) {
    if keys.is_null() {
        return;
    }
    let len = unsafe { keys.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
    let data = unsafe {
        keys.cast::<u8>()
            .add(ARR_DATA_PTR_OFF)
            .cast::<*mut u64>()
            .read()
    };
    if !data.is_null() {
        for i in 0..len {
            let k = unsafe { data.add(i as usize).read() } as *const c_void;
            if k.is_null() {
                continue;
            }
            let d = unsafe {
                crate::reflect_get_property_descriptor::__torajs_anyv_get_property_descriptor(
                    obj_any, k,
                )
            };
            // A getter-backed descriptor field can throw while being
            // read; stop rather than pile more work on a pending one.
            if unsafe { __torajs_throw_check() } != 0 {
                break;
            }
            if d == VALUE_UNDEFINED_IMM {
                continue;
            }
            let tag = unsafe { __torajs_anyv_unbox_tag(d) } as u64;
            let value = unsafe { __torajs_anyv_unbox_value(d) } as u64;
            unsafe { __torajs_dynobj_set(out_slot, k as *const u8, tag, value) };
        }
    }
    unsafe { __torajs_value_drop_heap(keys) };
}

/// `Object.getOwnPropertyDescriptors(O)` — a fresh ordinary object
/// carrying one descriptor per own property of `O`.
///
/// # Safety
/// `obj_any` carries a valid AnyValue bit pattern. The caller must
/// run its throw-check after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_get_property_descriptors(obj_any: u64) -> u64 {
    // §20.1.2.9 step 1 — `Let obj be ? ToObject(O)`, which throws for
    // null / undefined and boxes every other primitive to a wrapper
    // that carries no own properties of its own beyond what the key
    // walks below already answer for it.
    if obj_any == VALUE_UNDEFINED_IMM {
        unsafe { __torajs_throw_type_error(c"undefined is not an object".as_ptr()) };
        return VALUE_UNDEFINED_IMM;
    }
    if obj_any == VALUE_NULL_IMM {
        unsafe { __torajs_throw_type_error(c"null is not an object".as_ptr()) };
        return VALUE_UNDEFINED_IMM;
    }
    let mut out = unsafe { __torajs_dynobj_alloc() };
    if out.is_null() {
        return VALUE_UNDEFINED_IMM;
    }
    let out_slot: *mut *mut c_void = &mut out;
    // include_nonenum = 1: this is the OWN surface, not the
    // enumerable one.
    let keys = unsafe { __torajs_anyv_own_keys(obj_any, 1) };
    unsafe { drain_keys(obj_any, keys, out_slot) };
    // A non-cell receiver (a bare number / boolean immediate) has no
    // symbol-keyed own properties and no cell for the symbol walk to
    // read, so it is asked only for its string keys above.
    if is_cell_imm(obj_any) {
        let syms = unsafe { __torajs_anyv_own_symbols(obj_any) };
        unsafe { drain_keys(obj_any, syms, out_slot) };
    }
    // The answer is the dynobj itself, boxed as a heap AnyValue —
    // the caller adopts the mint's one share.
    unsafe { __torajs_anyv_box_from_pair(ANY_HEAP, out as i64) }
}
