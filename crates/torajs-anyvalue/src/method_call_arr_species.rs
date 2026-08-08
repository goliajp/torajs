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
//!   (step 10 Construct(C, « len »)); its throw propagates. A
//!   normal completion's product is RELEASED and the derive
//!   proceeds with the default Array — recorded divergence: the
//!   product's identity (and the exact len argument for the
//!   count-shaped surfaces) diverges, but the constructor's side
//!   effects and abrupt completions are real, which is what the
//!   t262 abrupt/poisoned family observes;
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
}

/// The species-family gate's object half — call AFTER
/// `__torajs_arr_species_guard` answered 0. Returns 1 when a throw
/// was recorded (the dispatcher answers undefined early), 0 when the
/// derive proceeds with the default Array product.
///
/// # Safety
/// `arr` is a live array heap block pointer.
pub(crate) unsafe fn arr_species_object_face(arr: *mut c_void) -> i64 {
    unsafe {
        let props = *((arr as *const u8).add(MIRROR_ARR_PROPS_OFF) as *const *const c_void);
        if props.is_null() {
            return 0;
        }
        let key = __torajs_str_alloc(b"constructor".as_ptr(), 11);
        let ctag = __torajs_arrprops_get_tag(arr, key as *const c_void);
        let cval = __torajs_arrprops_get_value(arr, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        // Only a heap OBJECT reaches step 6's species read — the
        // guard already threw for primitives and defaulted for
        // absent/undefined. Heap-shaped primitives threw there too.
        if ctag != 4 {
            return 0;
        }
        let cptr = cval as *mut c_void;
        let hdr_tag = (cptr.cast::<u8>().add(4) as *const u16).read();
        if hdr_tag == Tag::Str as u16
            || hdr_tag == Tag::Symbol as u16
            || hdr_tag == Tag::BigInt as u16
        {
            return 0;
        }
        let sym = __torajs_symbol_well_known(WELL_KNOWN_SPECIES_IDX);
        if sym.is_null() {
            return 0;
        }
        let ctor_av = __torajs_anyv_box_pointer(cptr);
        let (stag, sval) = symbol_key_pair(ctor_av, sym as *const c_void);
        __torajs_value_drop_heap(sym);
        // steps 7.b-8 — undefined / null species defaults.
        if stag == 5 || stag == 0 {
            return 0;
        }
        let species = crate::nanbox_encode::__torajs_anyv_box_from_pair(stag as i64, sval as i64);
        run_species_ctor(arr, species)
    }
}

/// Step 10 `Construct(C, « len »)` over the shapes tr can run: a
/// class object takes the real [[Construct]] kernel, a closure cell
/// its boxed dual entry (this = undefined). The product is released
/// — identity divergence recorded in the module doc; the abrupt
/// completion and the body's side effects are the observable part.
unsafe fn run_species_ctor(arr: *mut c_void, species: AnyValue) -> i64 {
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
            return 1;
        };
        let threw = __torajs_throw_check() != 0;
        if is_cell(product) && !as_void_ptr(product).is_null() {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
        }
        i64::from(threw)
    }
}
