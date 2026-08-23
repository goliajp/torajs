//! The species-product WRITE face — split from
//! `method_call_arr_species` (that file resolves the constructor
//! face and runs the species ctor; this one answers "how an element
//! — or a whole spread — lands in the product", whatever shape the
//! foreign constructor minted).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::index_any::MIRROR_ARR_LEN_OFF;
use crate::method_call_arr_species::release_species_product;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_encode::{__torajs_anyv_box_i64, __torajs_anyv_box_pointer};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    /// torajs-arr — kind-aware `arr[idx]` read; the answer is an
    /// OWNED AnyValue (+1) whatever the slot kind.
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-str — key alloc for the define / length stores.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — §10.1.6.3 [[DefineOwnProperty]] (throwing
    /// flavor); `obj_slot` threads a resize relocation back. Value
    /// rc transfers under `DEFINE_PRESENT_VALUE`.
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
}

/// §23.1.3.1 concat with A = the species product (B3 first cut):
/// spread the receiver, then each argument (a `Tag::Arr` heap
/// argument spreads, everything else appends as one element — the
/// same `@@isConcatSpreadable`-free subset the default-product
/// kernel implements), writing each element through the any-lane
/// index-set kernel (CreateDataPropertyOrThrow approximation), then
/// set `length`. An abrupt store (frozen product, non-writable
/// index) stops the derive and propagates — the product is released
/// and the dispatcher answers undefined.
///
/// The element writes ride `__torajs_any_index_set`, so a DynObj
/// product (a plain-fn species constructor's fresh `this`) stores
/// through its keyed bag — a resize-relocated block writes itself
/// back through the threaded product slot — an Arr product through
/// the kind-aware element store, and a `Tag::Obj` product through
/// the decimal-key member route; a receiver shape without an
/// indexed-write arm records the kernel's own TypeError (loud,
/// never silent-wrong).
///
/// # Safety
/// `product` is an owned AnyValue; `this_arr` is a live `Tag::Arr`
/// heap pointer; `argv` holds `argc` borrowed AnyValues.
pub(crate) unsafe fn concat_into_foreign(
    product: AnyValue,
    this_arr: *mut c_void,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let mut product = product;
        let mut n: i64 = 0;
        if !append_spread(&mut product, this_arr, &mut n) {
            release_species_product(product);
            return VALUE_UNDEFINED;
        }
        for k in 0..argc {
            let av = *argv.add(k as usize);
            let mut spread_arr: Option<*mut c_void> = None;
            if is_cell(av) {
                let p = as_void_ptr(av);
                if !p.is_null() && (p.cast::<u8>().add(4) as *const u16).read() == Tag::Arr as u16 {
                    spread_arr = Some(p);
                }
            }
            let ok = match spread_arr {
                Some(p) => append_spread(&mut product, p, &mut n),
                None => {
                    // Borrowed argv slot → the store pair takes its
                    // own stake (tag 4 transfers one rc).
                    crate::nanbox_ffi::__torajs_anyv_rc_inc(av);
                    store_elem(&mut product, n, av) && {
                        n += 1;
                        true
                    }
                }
            };
            if !ok {
                release_species_product(product);
                return VALUE_UNDEFINED;
            }
        }
        write_product_length(&mut product, n);
        if __torajs_throw_check() != 0 {
            release_species_product(product);
            return VALUE_UNDEFINED;
        }
        product
    }
}

/// Spread one `Tag::Arr` source into `product` starting at `*n`
/// (kind-aware element reads — each read answers an OWNED box that
/// transfers straight into the store pair). `false` = a store
/// recorded a pending throw.
pub(crate) unsafe fn append_spread(product: &mut AnyValue, src: *mut c_void, n: &mut i64) -> bool {
    unsafe {
        let len = *((src as *const u8).add(MIRROR_ARR_LEN_OFF) as *const u64) as i64;
        for i in 0..len {
            let elem = __torajs_arr_index_get(src as *const c_void, i);
            if !store_elem(product, *n, elem) {
                return false;
            }
            *n += 1;
        }
        true
    }
}

/// The all-true data descriptor CreateDataPropertyOrThrow passes to
/// [[DefineOwnProperty]]: value + writable + enumerable +
/// configurable, every field present (`torajs-dynobj layout.rs`
/// flags-byte encoding).
const DEFINE_ALL_TRUE: u64 = 0x7F;

/// One CreateDataPropertyOrThrow store: a DynObj product (a plain-fn
/// species constructor's fresh `this`) takes the real §10.1.6.3
/// define kernel with an all-true data descriptor — §23.1.3.1.1
/// step 5.c.iii's define semantics REDEFINE a configurable
/// non-writable entry and refuse a non-configurable one (the
/// create-species-with-non-* t262 quartet); the set-semantics
/// shortcut this replaces threw on `writable: false` however
/// configurable the entry was. Every other product shape keeps the
/// any-lane index-set kernel (an Arr product's fresh dense slots
/// have no attributes to collide with; that kernel also threads the
/// product slot so a resize relocation writes the fresh cell back).
/// `false` when the store recorded a pending throw.
pub(crate) unsafe fn store_elem(product: &mut AnyValue, idx: i64, owned_elem: AnyValue) -> bool {
    unsafe {
        if is_cell(*product) {
            let p = as_void_ptr(*product);
            if !p.is_null() && (p.cast::<u8>().add(4) as *const u16).read() == Tag::DynObj as u16 {
                let digits = idx.to_string();
                let key = __torajs_str_alloc(digits.as_ptr(), digits.len() as i64);
                let mut obj = p;
                __torajs_dynobj_define(
                    &mut obj,
                    key as *mut c_void,
                    crate::nanbox_encode::__torajs_anyv_unbox_tag(owned_elem) as u64,
                    crate::nanbox_encode::__torajs_anyv_unbox_value(owned_elem) as u64,
                    DEFINE_ALL_TRUE,
                );
                __torajs_str_drop(key as *mut c_void);
                if obj != p {
                    *product = __torajs_anyv_box_pointer(obj);
                }
                return __torajs_throw_check() == 0;
            }
        }
        crate::index_any_set::__torajs_any_index_set(
            *product,
            idx,
            crate::nanbox_encode::__torajs_anyv_unbox_tag(owned_elem) as u64,
            crate::nanbox_encode::__torajs_anyv_unbox_value(owned_elem) as u64,
            product as *mut AnyValue,
        );
        __torajs_throw_check() == 0
    }
}

/// Step 6 `Set(A, "length", n, true)` — an Arr product's length is
/// already `n` from the dense element stores (the explicit set is a
/// no-op there); a DynObj product takes a real `length` data entry
/// through the keyed member store (soft flavor, hint -1 = no
/// interned fast path — a fresh species product never refuses).
pub(crate) unsafe fn write_product_length(product: &mut AnyValue, n: i64) {
    unsafe {
        if is_cell(*product) {
            let p = as_void_ptr(*product);
            if !p.is_null() && (p.cast::<u8>().add(4) as *const u16).read() == Tag::Arr as u16 {
                return;
            }
        }
        let key = __torajs_str_alloc(b"length".as_ptr(), 6);
        let nbox = __torajs_anyv_box_i64(n);
        crate::member_set::__torajs_any_member_set_soft(
            product as *mut AnyValue,
            key as *mut c_void,
            crate::nanbox_encode::__torajs_anyv_unbox_tag(nbox) as u64,
            crate::nanbox_encode::__torajs_anyv_unbox_value(nbox) as u64,
            -1,
        );
        __torajs_str_drop(key as *mut c_void);
    }
}
