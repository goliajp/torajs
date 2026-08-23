//! §7.3.20 SpeciesConstructor over a buffer-family receiver — the
//! constructor-FACE half (torajs-arr `species.rs` twin; RFC
//! 20260823-typedarray-substrate `@@species` knife).
//!
//! §23.2.4.1 TypedArraySpeciesCreate step 2 begins with
//! `Get(exemplar, "constructor")`. Now that a view carries an own
//! expando bag, that read can hit an instance-installed entry — and
//! the test262 speciesctor family installs a THROWING getter there,
//! whose throw must surface from `slice` / `subarray` / `filter` /
//! `map`. This kernel classifies the face:
//!
//! - absent / undefined → 0 (step 3, default constructor);
//! - accessor entry → the getter RUNS (§7.3.2 Get); a throw is 1,
//!   an answer re-classifies;
//! - heap-shaped primitive (Str / Symbol / BigInt) or a non-cell
//!   imm → step 4 TypeError, 1;
//! - any other object → 0. The `@@species` read and the foreign
//!   construct channel are the recorded second half (the Array
//!   family's `method_call_arr_species` shape) — a user species
//!   constructor still yields the default product today.

use core::ffi::{c_char, c_void};

use torajs_anyvalue::nanbox::{AnyValue, as_void_ptr};
use torajs_rc::Tag;

use crate::arraybuffer::is_arraybuffer;
use crate::typedarray::is_typedarray;

unsafe extern "C" {
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv: AnyValue) -> AnyValue;
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_anyv_unbox_tag(v: AnyValue) -> i64;
    fn __torajs_anyv_unbox_value(v: AnyValue) -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> AnyValue;
}

/// The receiver's expando bag, or NULL (mirrors the member channels).
unsafe fn props_of(recv: AnyValue) -> *const c_void {
    let ptr = as_void_ptr(recv);
    let off = if is_typedarray(recv) {
        crate::typedarray::PROPS_OFF
    } else {
        crate::arraybuffer::PROPS_OFF
    };
    unsafe { (ptr.cast::<u8>().add(off) as *const u64).read() as *const c_void }
}

/// §23.2.4.1 step 2 / §7.3.20 steps 2-4 — 1 = a throw is pending
/// (the caller returns undefined), 0 = proceed with the default
/// constructor's product.
///
/// # Safety
/// `recv` is a live buffer-family AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_buffer_species_guard(recv: AnyValue) -> i64 {
    unsafe {
        if !is_typedarray(recv) && !is_arraybuffer(recv) {
            return 0;
        }
        let props = props_of(recv);
        if props.is_null() {
            return 0;
        }
        let key = __torajs_str_alloc(b"constructor".as_ptr(), 11);
        let dtag = __torajs_dynobj_get_tag(props, key as *const c_void);
        let dval = __torajs_dynobj_get_value(props, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        classify_ctor_entry(recv, dtag, dval)
    }
}

/// The per-entry dispatch (probe pair or accessor-getter answer) —
/// torajs-arr `classify_ctor_pair_entry` twin.
unsafe fn classify_ctor_entry(recv: AnyValue, dtag: u64, dval: u64) -> i64 {
    unsafe {
        match dtag {
            // Absent or explicit undefined — default constructor.
            5 => 0,
            // Accessor entry — the getter runs; its throw is the
            // whole point of the speciesctor-abrupt family.
            6 => {
                let got = __torajs_accessor_invoke_getter(dval as *const c_void, recv);
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(as_void_ptr(got));
                    return 1;
                }
                let gtag = __torajs_anyv_unbox_tag(got);
                let gval = __torajs_anyv_unbox_value(got);
                let verdict = classify_ctor_entry(recv, gtag as u64, gval as u64);
                __torajs_value_drop_heap(as_void_ptr(got));
                verdict
            }
            // Heap cell: an object proceeds (the @@species half is
            // the recorded follow-up); a heap-shaped primitive is
            // the §7.3.20 step 4 TypeError.
            4 => {
                let tag = ((dval as *const u8).add(4) as *const u16).read();
                if tag == Tag::Str as u16 || tag == Tag::Symbol as u16 || tag == Tag::BigInt as u16
                {
                    __torajs_throw_type_error(c"species constructor is not a constructor".as_ptr());
                    return 1;
                }
                0
            }
            // Number / boolean imm — step 4 TypeError.
            _ => {
                __torajs_throw_type_error(c"species constructor is not a constructor".as_ptr());
                1
            }
        }
    }
}
