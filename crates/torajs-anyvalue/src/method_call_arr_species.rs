//! §9.4.2.3 ArraySpeciesCreate steps 5-10, the `@@species` half
//! (RFC 20260808 knife 4).
//!
//! The torajs-arr guard (`__torajs_arr_species_guard`, RFC 20260713
//! blade 3) classifies the `constructor` FACE: absent / undefined
//! defaults, a primitive throws, and an accessor getter runs. What
//! it answers 0 for — a heap-object constructor — used to proceed
//! straight to the default Array product, skipping steps 6-10: the
//! object's `@@species` slot was never read, so a poisoned species
//! constructor (`a.constructor = {}; a.constructor[Symbol.species]
//! = function () { throw … }`) never ran and its throw was
//! unobservable (the create-species-abrupt / -poisoned t262
//! family).
//!
//! This sibling picks up where the guard's 0 leaves off:
//!
//! - read the constructor's `@@species` (the §10.1.8.1 symbol-key
//!   walk — an own or inherited entry);
//! - undefined / null → default Array (steps 7.b-8);
//! - a callable — a class object or a closure cell with a boxed
//!   dual entry — RUNS with the receiver's length as its argument
//!   (step 10 Construct(C, « len »)); its throw propagates. The
//!   product is handed back to the dispatcher (RFC 20260808
//!   construct-channel B3): `concat` derives INTO it
//!   ([`concat_into_foreign`] — §23.1.3.1 with A = the species
//!   product, per-element CreateDataPropertyOrThrow through the
//!   any-lane index-set kernel); the other six family methods still
//!   release it and proceed with the default Array — recorded
//!   divergence, one method at a time;
//! - anything else → the step-10 Construct TypeError.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::construct::is_constructor;
use crate::index_any::MIRROR_ARR_LEN_OFF;
use crate::member_get_symbol::symbol_key_pair;
use crate::method_call::{closure_boxed_entry, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_encode::{__torajs_anyv_box_i64, __torajs_anyv_box_pointer};

/// `torajs-arr layout::ARR_PROPS_OFF` mirror (the guard reads the
/// same slot): HEAP_HEADER_SIZE(8) + 16.
const MIRROR_ARR_PROPS_OFF: usize = 24;

/// Alphabetical index of `Symbol.species` in the §6.1.5.1 well-known
/// table (torajs-str `WELL_KNOWN_DESCS`).
const WELL_KNOWN_SPECIES_IDX: i64 = 10;

unsafe extern "C" {
    /// torajs-arr — expando-bag probe pair (same contract the
    /// member-get arm uses; tag 5 = absent-or-undefined).
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
    /// torajs-str — key alloc for the props probe + the well-known
    /// singleton (owned +1).
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-throw — step-10 TypeError + pending-throw probe.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    /// Universal NaN-box-safe release (the well-known key's stake).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-arr — kind-aware `arr[idx]` read; the answer is an
    /// OWNED AnyValue (+1) whatever the slot kind.
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-arr — §9.4.2.3 steps 5-8 constructor-face classifier
    /// (RFC 20260713 blade 3): 1 = threw (primitive constructor /
    /// poisoned accessor), 0 = proceed.
    fn __torajs_arr_species_guard(arr: *const u8) -> i64;
}

/// How the §23.1.3 species-family pre-gate resolved (B3 cut 2).
pub(crate) enum SpeciesGate {
    /// The gate's own answer — a recorded throw's undefined, or the
    /// concat derive into the constructed product.
    Early(AnyValue),
    /// Run the default kernel, then transplant its product's
    /// elements into this constructed product
    /// ([`transplant_product`]) — the slice / splice / filter / map
    /// / flat / flatMap cut: the kernel keeps its receiver side
    /// effects (splice's in-place mutation) and its element
    /// semantics; only the product identity swaps. Recorded
    /// approximation: the per-element CreateDataProperty timing
    /// lands after the kernel run instead of interleaved with it —
    /// unobservable without proxies, which tr does not surface.
    TransplantInto(AnyValue),
    /// No species face — derive with the default Array product.
    Proceed,
}

/// The whole §23.1.3 species-family pre-gate, hoisted out of
/// `arr_method` (file-size discipline).
///
/// # Safety
/// `arr` is a live array heap block pointer; `argv` holds `argc`
/// borrowed AnyValues.
pub(crate) unsafe fn species_family_pregate(
    arr: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> SpeciesGate {
    if !matches!(mid, m if m == torajs_rc::ANY_METHOD_SLICE
        || m == torajs_rc::ANY_METHOD_SPLICE
        || m == torajs_rc::ANY_METHOD_CONCAT
        || m == torajs_rc::ANY_METHOD_FILTER
        || m == torajs_rc::ANY_METHOD_MAP
        || m == torajs_rc::ANY_METHOD_FLAT
        || m == torajs_rc::ANY_METHOD_FLAT_MAP)
    {
        return SpeciesGate::Proceed;
    }
    unsafe {
        if __torajs_arr_species_guard(arr as *const u8) != 0 {
            return SpeciesGate::Early(VALUE_UNDEFINED);
        }
        match arr_species_object_face(arr) {
            SpeciesOutcome::Threw => SpeciesGate::Early(VALUE_UNDEFINED),
            SpeciesOutcome::Product(product) => {
                if mid == torajs_rc::ANY_METHOD_CONCAT {
                    return SpeciesGate::Early(concat_into_foreign(product, arr, argv, argc));
                }
                SpeciesGate::TransplantInto(product)
            }
            SpeciesOutcome::Default => SpeciesGate::Proceed,
        }
    }
}

/// Does this family method run a `Set(A, "length", n, true)` step
/// after its element writes? slice (§23.1.3.4 step 15) and splice
/// (§23.1.3.31 step 12) do; map / filter / flat / flatMap only
/// CreateDataProperty their elements — a non-Array species product
/// keeps NO length entry there (concat's step rides its own derive).
pub(crate) fn family_sets_length(mid: i64) -> bool {
    mid == torajs_rc::ANY_METHOD_SLICE || mid == torajs_rc::ANY_METHOD_SPLICE
}

/// Move the default kernel's product into the species-constructed
/// one (B3 cut 2): spread every element of the default Arr product
/// into `product` through the same CreateDataPropertyOrThrow store
/// the concat derive uses, set length, release the default. A
/// pending throw (the kernel's own, or a poisoned store) releases
/// the constructed product and answers the kernel's result verbatim
/// — the dispatcher's throw check propagates it.
///
/// # Safety
/// `product` is an owned AnyValue; `default_result` is the kernel's
/// owned answer.
pub(crate) unsafe fn transplant_product(
    product: AnyValue,
    default_result: AnyValue,
    set_length: bool,
) -> AnyValue {
    unsafe {
        if __torajs_throw_check() != 0 {
            release_species_product(product);
            return default_result;
        }
        let d_ptr = if is_cell(default_result) {
            let p = as_void_ptr(default_result);
            if !p.is_null() && (p.cast::<u8>().add(4) as *const u16).read() == Tag::Arr as u16 {
                Some(p)
            } else {
                None
            }
        } else {
            None
        };
        let Some(d_ptr) = d_ptr else {
            // Not an Arr product (a sentinel / undefined edge) — the
            // constructed product has no elements to receive; hand
            // the kernel's answer through unchanged.
            release_species_product(product);
            return default_result;
        };
        let mut product = product;
        let mut n: i64 = 0;
        let ok = append_spread(&mut product, d_ptr, &mut n);
        if ok && set_length {
            write_product_length(&mut product, n);
        }
        release_species_product(default_result);
        if !ok || __torajs_throw_check() != 0 {
            release_species_product(product);
            return VALUE_UNDEFINED;
        }
        product
    }
}

/// What the `@@species` object half resolved to (RFC 20260808
/// construct-channel B3).
pub(crate) enum SpeciesOutcome {
    /// Steps 5-8 defaulted — derive with the default Array product.
    Default,
    /// The species read / construct recorded a pending throw — the
    /// dispatcher answers undefined early.
    Threw,
    /// Step 10 completed: the constructed product, OWNED (+1). The
    /// consuming method derives into it or releases it (recorded
    /// divergence for the methods not yet cut over).
    Product(AnyValue),
}

/// The species-family gate's object half — call AFTER
/// `__torajs_arr_species_guard` answered 0.
///
/// # Safety
/// `arr` is a live array heap block pointer.
pub(crate) unsafe fn arr_species_object_face(arr: *mut c_void) -> SpeciesOutcome {
    unsafe {
        let props = *((arr as *const u8).add(MIRROR_ARR_PROPS_OFF) as *const *const c_void);
        if props.is_null() {
            return SpeciesOutcome::Default;
        }
        let key = __torajs_str_alloc(b"constructor".as_ptr(), 11);
        let ctag = __torajs_arrprops_get_tag(arr, key as *const c_void);
        let cval = __torajs_arrprops_get_value(arr, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        // Only a heap OBJECT reaches step 6's species read — the
        // guard already threw for primitives and defaulted for
        // absent/undefined. Heap-shaped primitives threw there too.
        if ctag != 4 {
            return SpeciesOutcome::Default;
        }
        let cptr = cval as *mut c_void;
        let hdr_tag = (cptr.cast::<u8>().add(4) as *const u16).read();
        if hdr_tag == Tag::Str as u16
            || hdr_tag == Tag::Symbol as u16
            || hdr_tag == Tag::BigInt as u16
        {
            return SpeciesOutcome::Default;
        }
        let sym = __torajs_symbol_well_known(WELL_KNOWN_SPECIES_IDX);
        if sym.is_null() {
            return SpeciesOutcome::Default;
        }
        let ctor_av = __torajs_anyv_box_pointer(cptr);
        let (stag, sval) = symbol_key_pair(ctor_av, sym as *const c_void);
        __torajs_value_drop_heap(sym);
        // step 7.a with the INHERITED default getter (§23.1.2.5 —
        // `get Array[@@species]` returns this, and an `extends
        // Array` class inherits it through its static chain, which
        // tr does not walk): an own-miss on a marked class answers
        // the class itself. Recorded approximation: an explicit own
        // `@@species = undefined` on a marked class would default
        // per spec but constructs here (the override spelling is
        // absent from the surveyed corpus).
        if stag == 5 && crate::construct::ctor_arr_species_self(cptr as u64) {
            return run_species_ctor(arr, ctor_av);
        }
        // steps 7.b-8 — undefined / null species defaults.
        if stag == 5 || stag == 0 {
            return SpeciesOutcome::Default;
        }
        let species = crate::nanbox_encode::__torajs_anyv_box_from_pair(stag as i64, sval as i64);
        run_species_ctor(arr, species)
    }
}

/// Step 10 `Construct(C, « len »)` over the shapes tr can run: a
/// class object takes the real [[Construct]] kernel, a closure cell
/// with `FLAG_FN_PROTO` the plain-fn construct kernel (B1), a bare
/// boxed dual entry the call-semantics fallback (this = undefined).
unsafe fn run_species_ctor(arr: *mut c_void, species: AnyValue) -> SpeciesOutcome {
    unsafe {
        let len = *((arr as *const u8).add(MIRROR_ARR_LEN_OFF) as *const u64) as i64;
        let len_boxed = __torajs_anyv_box_i64(len);
        let argv = [len_boxed];
        let product = if is_constructor(species) {
            crate::construct::__torajs_anyv_construct(species, argv.as_ptr(), 1)
        } else if let Some((env, entry)) = closure_boxed_entry(species) {
            invoke_with_this(env, entry, VALUE_UNDEFINED, argv.as_ptr(), 1)
        } else {
            __torajs_throw_type_error(c"array species constructor is not a constructor".as_ptr());
            return SpeciesOutcome::Threw;
        };
        if __torajs_throw_check() != 0 {
            if is_cell(product) && !as_void_ptr(product).is_null() {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
            }
            return SpeciesOutcome::Threw;
        }
        SpeciesOutcome::Product(product)
    }
}

/// Release a species product a method is not (yet) deriving into —
/// the pre-B3 recorded divergence, kept per method until each cuts
/// over to its foreign-derive path.
///
/// # Safety
/// `product` is an owned AnyValue.
pub(crate) unsafe fn release_species_product(product: AnyValue) {
    unsafe {
        if is_cell(product) && !as_void_ptr(product).is_null() {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
        }
    }
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
unsafe fn append_spread(product: &mut AnyValue, src: *mut c_void, n: &mut i64) -> bool {
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

/// One CreateDataPropertyOrThrow-approximating store: the pair
/// transfers into the any-lane index-set kernel; `false` when the
/// kernel recorded a pending throw. The product slot threads through
/// so a DynObj resize relocation writes the fresh cell back.
unsafe fn store_elem(product: &mut AnyValue, idx: i64, owned_elem: AnyValue) -> bool {
    unsafe {
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
unsafe fn write_product_length(product: &mut AnyValue, n: i64) {
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
