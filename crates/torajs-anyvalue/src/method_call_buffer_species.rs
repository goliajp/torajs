//! §23.2.4.1 TypedArraySpeciesCreate — the construct channel for the
//! four `%TypedArray%.prototype` species methods (`filter` / `map` /
//! `slice` / `subarray`), RFC 20260823-typedarray-substrate.
//!
//! The face walk (constructor read, accessor getters, `@@species`,
//! the non-constructor TypeErrors) is `buffer_species`'s — this file
//! only CONSUMES its resolution. The Array family's shape
//! (`method_call_arr_species`), adapted: the default kernel runs
//! FIRST and the species constructor is then invoked with the
//! default product's measurements — `« len »`, or `« buffer,
//! byteOffset, length »` for `subarray` — and the elements
//! transplant across. Same recorded approximation the Array family
//! ships: the constructor-face read lands ahead of the method's own
//! index coercions, and the per-element store timing lands after the
//! kernel run instead of interleaved with it.
//!
//! §23.2.4.2 TypedArrayCreate over the product is real: a
//! non-TypedArray or detached product is a TypeError, and a
//! single-number argument list requires the product to be at least
//! as long as the default's.

use torajs_rc::Tag;

use crate::buffer_species::{SpeciesResolved, ta_species_resolve};
use crate::method_call_arr_species::{
    SpeciesOutcome, release_species_product, run_species_ctor_argv, store_elem,
};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_i64;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    /// §23.2.4.4 at the ABI — the length, or -1 with a pending throw.
    fn __torajs_typedarray_validate(recv: AnyValue) -> i64;
    fn __torajs_typedarray_index_get(recv: AnyValue, index: f64) -> AnyValue;
    fn __torajs_typedarray_buffer(av: AnyValue) -> AnyValue;
    fn __torajs_typedarray_byte_offset(av: AnyValue) -> i64;
    fn __torajs_typedarray_length(av: AnyValue) -> i64;
}

/// The four mids §23.2.3 routes through TypedArraySpeciesCreate.
fn is_species_mid(mid: i64) -> bool {
    mid == torajs_rc::ANY_METHOD_FILTER
        || mid == torajs_rc::ANY_METHOD_MAP
        || mid == torajs_rc::ANY_METHOD_SLICE
        || mid == torajs_rc::ANY_METHOD_SUBARRAY
}

/// The whole species route for a TypedArray receiver: `None` = not a
/// species mid or no constructor face — the caller proceeds with the
/// plain dispatch. `Some(v)` = the route owns the answer (a
/// constructed product, or undefined under a pending throw).
///
/// # Safety
/// `recv` is a live TypedArray AnyValue; `argv` holds `argc` live
/// AnyValues.
pub(crate) unsafe fn ta_species_route(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    if !is_species_mid(mid) {
        return None;
    }
    let ctor = match unsafe { ta_species_resolve(recv) } {
        SpeciesResolved::Default => return None,
        SpeciesResolved::Threw => return Some(VALUE_UNDEFINED),
        SpeciesResolved::Ctor(c) => c,
    };
    // Default kernel first — its receiver coercions and element
    // semantics are the method's own; only the product identity
    // swaps (module-doc approximation).
    let d = unsafe {
        crate::method_call_buffer::typedarray_method(recv, mid, argv, argc)
            .unwrap_or(VALUE_UNDEFINED)
    };
    unsafe {
        if __torajs_throw_check() != 0 || !is_typedarray_cell(d) {
            release_species_product(ctor);
            return Some(d);
        }
        Some(construct_and_transplant(ctor, d, mid))
    }
}

fn is_typedarray_cell(v: AnyValue) -> bool {
    is_cell(v) && {
        let p = as_void_ptr(v);
        !p.is_null()
            && unsafe { (p.cast::<u8>().add(4) as *const u16).read() } == Tag::TypedArray as u16
    }
}

/// §23.2.4.1 steps 2-3 + §23.2.4.2 over the default product `d`
/// (OWNED — released here) and the species constructor `ctor`
/// (OWNED — released here).
unsafe fn construct_and_transplant(ctor: AnyValue, d: AnyValue, mid: i64) -> AnyValue {
    unsafe {
        let d_len = __torajs_typedarray_length(d);
        let outcome = if mid == torajs_rc::ANY_METHOD_SUBARRAY {
            // §23.2.3.30 step 12 — « buffer, beginByteOffset,
            // newLength »: the product views the SAME buffer.
            let buf = __torajs_typedarray_buffer(d);
            let off = __torajs_anyv_box_i64(__torajs_typedarray_byte_offset(d));
            let len = __torajs_anyv_box_i64(d_len);
            let out = run_species_ctor_argv(ctor, &[buf, off, len]);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(buf);
            out
        } else {
            run_species_ctor_argv(ctor, &[__torajs_anyv_box_i64(d_len)])
        };
        release_species_product(ctor);
        let product = match outcome {
            // The argv form never defaults — the face already
            // resolved a constructor — so both non-product arms
            // answer the pending throw's undefined.
            SpeciesOutcome::Threw | SpeciesOutcome::Default => {
                release_species_product(d);
                return VALUE_UNDEFINED;
            }
            SpeciesOutcome::Product(p) => p,
        };
        // §23.2.4.2 TypedArrayCreate steps 2-3.
        let plen = __torajs_typedarray_validate(product);
        if plen < 0 {
            release_species_product(product);
            release_species_product(d);
            return VALUE_UNDEFINED;
        }
        if mid != torajs_rc::ANY_METHOD_SUBARRAY && plen < d_len {
            __torajs_throw_type_error(c"species-constructed typed array is too small".as_ptr());
            release_species_product(product);
            release_species_product(d);
            return VALUE_UNDEFINED;
        }
        // subarray's product views the same buffer at the same
        // window — the bytes are already there. The « len » family
        // copies element-wise through the any-lane store.
        if mid != torajs_rc::ANY_METHOD_SUBARRAY {
            let mut product_slot = product;
            for i in 0..d_len {
                let elem = __torajs_typedarray_index_get(d, i as f64);
                let ok = store_elem(&mut product_slot, i, elem);
                if !ok {
                    release_species_product(product_slot);
                    release_species_product(d);
                    return VALUE_UNDEFINED;
                }
            }
            release_species_product(d);
            return product_slot;
        }
        release_species_product(d);
        product
    }
}
