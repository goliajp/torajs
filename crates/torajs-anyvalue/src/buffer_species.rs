//! §7.3.20 SpeciesConstructor over a buffer-family receiver (RFC
//! 20260823-typedarray-substrate `@@species` knife, both halves;
//! the constructor-face half started life in torajs-buffer and
//! moved here when the `@@species` read needed this crate's symbol
//! walk and constructor probe).
//!
//! §23.2.4.1 TypedArraySpeciesCreate step 2 / §25.1.6.16 step 14:
//!
//! - `Get(O, "constructor")` reads the instance's expando bag — an
//!   accessor getter RUNS (the speciesctor-abrupt family's throw),
//!   absent / undefined defaults, a primitive is the step-4
//!   TypeError;
//! - an object constructor's `@@species` is read through the
//!   symbol-key walk — a getter runs, undefined / null default,
//!   a non-constructor is the step-8 TypeError;
//! - a species CONSTRUCTOR still yields the default product — the
//!   foreign construct channel (the Array family's transplant,
//!   `method_call_arr_species`) is the recorded follow-up.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::construct::is_constructor;
use crate::member_get_symbol::symbol_key_pair;
use crate::nanbox::{AnyValue, as_void_ptr};

/// Alphabetical index of `Symbol.species` in the §6.1.5.1 well-known
/// table (torajs-str `WELL_KNOWN_DESCS`).
const WELL_KNOWN_SPECIES_IDX: i64 = 10;

unsafe extern "C" {
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv: AnyValue) -> AnyValue;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-meta — the registered subclass proto dynobj AnyValue
    /// immediate (0 when the cell carries no subclass entry).
    fn __torajs_subclass_proto(cell: *const c_void) -> u64;
}

/// §23.2.4.1 step 2 / §7.3.20 — 1 = a throw is pending (the caller
/// returns undefined), 0 = proceed with the default constructor's
/// product. Reached from torajs-buffer's `slice` / `subarray` /
/// ArrayBuffer `slice` kernels over the C edge and from the map /
/// filter walkers in this crate.
///
/// # Safety
/// `recv` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_buffer_species_guard(recv: AnyValue) -> i64 {
    match unsafe { ta_species_resolve(recv) } {
        SpeciesResolved::Threw => 1,
        SpeciesResolved::Default => 0,
        // A real species constructor: this guard's callers have no
        // construct channel (ArrayBuffer `slice`), so the product
        // stays the default's — recorded, same wording as before.
        SpeciesResolved::Ctor(c) => {
            unsafe { crate::method_call_arr_species::release_species_product(c) };
            0
        }
    }
}

/// What the whole §7.3.20 face resolved to, VALUE included — the
/// typed-array construct channel consumes the constructor; the
/// guard above collapses it to a verdict.
pub(crate) enum SpeciesResolved {
    /// No face / an explicitly defaulting one.
    Default,
    /// The face read recorded a throw.
    Threw,
    /// A runnable species constructor (OWNED).
    Ctor(AnyValue),
}

/// The face walk behind the guard, resolution-shaped.
///
/// # Safety
/// `recv` is a live AnyValue.
pub(crate) unsafe fn ta_species_resolve(recv: AnyValue) -> SpeciesResolved {
    unsafe {
        let Some((ptr, t)) = crate::member_get_layout::recv_cell(recv) else {
            return SpeciesResolved::Default;
        };
        if !crate::member_get_buffer::is_buffer_family(t) {
            return SpeciesResolved::Default;
        }
        let props = crate::member_get_layout::buffer_props(ptr, t);
        let key = __torajs_str_alloc(b"constructor".as_ptr(), 11);
        let (mut dtag, mut dval) = if props.is_null() {
            (5u64, 0u64)
        } else {
            (
                __torajs_dynobj_get_tag(props, key as *const c_void),
                __torajs_dynobj_get_value(props, key as *const c_void),
            )
        };
        // A subclass instance's `constructor` lives on its class
        // prototype (§7.3.20's Get walks own → proto): an own-bag
        // miss consults the registered subclass proto dynobj before
        // defaulting, which is what makes `My.prototype.constructor`
        // — the class itself — the species face.
        if dtag == 5
            && (ptr.cast::<u8>().add(6) as *const u16).read() & torajs_rc::FLAG_SUBCLASSED != 0
        {
            let proto = __torajs_subclass_proto(ptr);
            if crate::nanbox::is_cell(proto) {
                let pp = as_void_ptr(proto);
                if !pp.is_null() {
                    dtag = __torajs_dynobj_get_tag(pp, key as *const c_void);
                    dval = __torajs_dynobj_get_value(pp, key as *const c_void);
                }
            }
        }
        __torajs_str_drop(key as *mut c_void);
        classify_ctor_entry(recv, dtag, dval)
    }
}

/// The constructor-face dispatch (probe pair or accessor-getter
/// answer) — torajs-arr `classify_ctor_pair_entry` twin.
unsafe fn classify_ctor_entry(recv: AnyValue, dtag: u64, dval: u64) -> SpeciesResolved {
    unsafe {
        match dtag {
            // Absent or explicit undefined — default constructor.
            5 => SpeciesResolved::Default,
            // Accessor entry — the getter runs; its throw is the
            // whole point of the speciesctor-abrupt family.
            6 => {
                let got = __torajs_accessor_invoke_getter(dval as *const c_void, recv);
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(as_void_ptr(got));
                    return SpeciesResolved::Threw;
                }
                let gtag = crate::nanbox_encode::__torajs_anyv_unbox_tag(got);
                let gval = crate::nanbox_encode::__torajs_anyv_unbox_value(got);
                let verdict = classify_ctor_entry(recv, gtag as u64, gval as u64);
                // A ShortStr answer materialized an owned rc=1 Str in
                // the unbox above; the recursion only probes it and
                // the box-level drop below no-ops on the immediate —
                // release the materialization (546-02 M1 family).
                if crate::nanbox::is_short_str(got) && gval != 0 {
                    __torajs_value_drop_heap(gval as *mut c_void);
                }
                __torajs_value_drop_heap(as_void_ptr(got));
                verdict
            }
            // Heap cell: an object proceeds to the @@species read; a
            // heap-shaped primitive is the §7.3.20 step 4 TypeError.
            4 => {
                let tag = ((dval as *const u8).add(4) as *const u16).read();
                if tag == Tag::Str as u16 || tag == Tag::Symbol as u16 || tag == Tag::BigInt as u16
                {
                    __torajs_throw_type_error(c"species constructor is not a constructor".as_ptr());
                    return SpeciesResolved::Threw;
                }
                species_object_face(dval as *mut c_void)
            }
            // Number / boolean imm — step 4 TypeError.
            _ => {
                __torajs_throw_type_error(c"species constructor is not a constructor".as_ptr());
                SpeciesResolved::Threw
            }
        }
    }
}

/// §7.3.20 steps 5-8 over the constructor OBJECT: read `@@species`
/// through the symbol walk (an own or inherited entry; an accessor
/// getter runs), default on undefined / null, refuse a
/// non-constructor. A real species constructor answers 0 too — the
/// construct channel is the recorded follow-up, so the product it
/// would build is still the default's.
unsafe fn species_object_face(ctor: *mut c_void) -> SpeciesResolved {
    unsafe {
        let sym = __torajs_symbol_well_known(WELL_KNOWN_SPECIES_IDX);
        if sym.is_null() {
            return SpeciesResolved::Default;
        }
        let ctor_av = crate::nanbox_encode::__torajs_anyv_box_pointer(ctor);
        let (stag, sval) = symbol_key_pair(ctor_av, sym as *const c_void);
        __torajs_value_drop_heap(sym);
        classify_species_entry(ctor_av, stag, sval)
    }
}

/// The `@@species` value dispatch — undefined (5) / null (0)
/// default; an accessor getter runs and its answer re-classifies;
/// a constructor proceeds (default product, recorded); anything
/// else is the step-8 TypeError.
unsafe fn classify_species_entry(ctor_av: AnyValue, stag: u64, sval: u64) -> SpeciesResolved {
    unsafe {
        // §23.2.2.4 with the INHERITED default getter — `get
        // %TypedArray%[@@species]` returns this, and a subclass
        // inherits it through its static chain, which tr does not
        // walk: an own-miss on a marked class answers the class
        // itself (the Array family's `ctor_arr_species_self` mark,
        // same table, same recorded approximation).
        if stag == 5 && crate::construct::ctor_arr_species_self(as_void_ptr(ctor_av) as u64) {
            crate::nanbox_ffi::__torajs_anyv_rc_inc(ctor_av);
            return SpeciesResolved::Ctor(ctor_av);
        }
        match stag {
            5 | 0 => SpeciesResolved::Default,
            6 => {
                let got = __torajs_accessor_invoke_getter(sval as *const c_void, ctor_av);
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(as_void_ptr(got));
                    return SpeciesResolved::Threw;
                }
                let gtag = crate::nanbox_encode::__torajs_anyv_unbox_tag(got);
                let gval = crate::nanbox_encode::__torajs_anyv_unbox_value(got);
                let verdict = classify_species_entry(ctor_av, gtag as u64, gval as u64);
                // A Ctor verdict keeps its own stake (the recursion
                // inc'd it) — the getter's answer releases either way.
                // A ShortStr answer additionally materialized an
                // owned rc=1 Str in the unbox above that the
                // box-level drop below cannot see (546-02 M1 family).
                if crate::nanbox::is_short_str(got) && gval != 0 {
                    __torajs_value_drop_heap(gval as *mut c_void);
                }
                __torajs_value_drop_heap(as_void_ptr(got));
                verdict
            }
            _ => {
                let s = crate::nanbox_encode::__torajs_anyv_box_from_pair(stag as i64, sval as i64);
                // §7.3.22 step 9 is IsConstructor, not IsCallable. The
                // `|| callable` widening was invisible while every
                // callable cell tr could reach here also had
                // [[Construct]]; §20.2.3's %Function.prototype% is the
                // counter-example the spec's own test names —
                // callable, no [[Construct]], and step 10's TypeError
                // is the whole point of the case.
                if is_constructor(s) {
                    crate::nanbox_ffi::__torajs_anyv_rc_inc(s);
                    return SpeciesResolved::Ctor(s);
                }
                __torajs_throw_type_error(c"species constructor is not a constructor".as_ptr());
                SpeciesResolved::Threw
            }
        }
    }
}
