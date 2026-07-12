//! §9.4.2.3 ArraySpeciesCreate constructor-face guard (RFC
//! 20260713-array-proto-residual blade 3).
//!
//! The species family (concat / filter / flat / flatMap / map /
//! slice / splice) reads `Get(O, "constructor")` before creating
//! its product. tr always creates plain Arrays, which is
//! spec-equivalent for the overwhelmingly common shapes — absent /
//! `undefined` constructor (step 6 default), or an object whose
//! `@@species` is undefined (steps 5-6: tr values never carry
//! `@@species`). The one OBSERVABLE divergence is step 7: a
//! present non-object non-undefined constructor (`a.constructor =
//! null / 1 / "s" / false`) must throw TypeError instead of
//! silently defaulting.
//!
//! Recorded boundaries: a same-realm CONSTRUCTOR function (step 4)
//! would be used to build the product per spec — tr defaults to
//! Array (matching the no-`@@species` fn shape, diverging for real
//! subclass ctors); an accessor `constructor` entry's getter is
//! not invoked (spec Get would run it).
//!
//! Fast path: an array that never grew expando props is one
//! NULL-check (`ARR_PROPS_OFF` load).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::layout::ARR_PROPS_OFF;

unsafe extern "C" {
    /// torajs-str — key alloc for the props probe.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — catchable TypeError (records via TLS; the
    /// compiled caller's throw check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Guard the species-family constructor face. Returns 1 when a
/// TypeError was recorded (the any-tier dispatcher answers
/// undefined early; compiled callers rely on the throw check), 0
/// when the derive may proceed with the default Array product.
///
/// # Safety
/// `arr` is a live array heap block pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_species_guard(arr: *const u8) -> i64 {
    unsafe {
        let props = *(arr.add(ARR_PROPS_OFF) as *const *const c_void);
        if props.is_null() {
            return 0;
        }
        let key = __torajs_str_alloc(b"constructor".as_ptr(), 11);
        let dtag =
            crate::props::__torajs_arrprops_get_tag(arr as *mut c_void, key as *const c_void);
        let dval =
            crate::props::__torajs_arrprops_get_value(arr as *mut c_void, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        match dtag {
            // Absent or explicit undefined — step 6 default Array.
            5 => 0,
            // Accessor entry — recorded boundary (getter not run).
            6 => 0,
            // Heap cell: objects (dynobj / struct / closure / any
            // container) reach steps 5-6 → @@species undefined →
            // default; heap-shaped PRIMITIVES (Str / Symbol /
            // BigInt) are step 7 non-constructors.
            4 => {
                let tag = ((dval as *const u8).add(4) as *const u16).read();
                if tag == Tag::Str as u16 || tag == Tag::Symbol as u16 || tag == Tag::BigInt as u16
                {
                    __torajs_throw_type_error(
                        c"array species constructor is not a constructor".as_ptr(),
                    );
                    1
                } else {
                    0
                }
            }
            // null / bool / number primitives — step 7 TypeError.
            _ => {
                __torajs_throw_type_error(
                    c"array species constructor is not a constructor".as_ptr(),
                );
                1
            }
        }
    }
}
