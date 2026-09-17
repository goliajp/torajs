//! `new` through a first-class builtin constructor value — split from
//! the parent so the per-tag table has room to grow. The parent
//! answers what a builtin constructor IS as a value (the interned
//! cells, their names, the call form); this answers what `new` on one
//! of them mints. Body verbatim; a child module reaches the parent's
//! private externs and helpers directly.

use super::*;

/// [[Construct]] of a builtin-constructor value — `new N(5)` through
/// a `const N: any = Number` binding, `new (Array.bind(null))(3)`,
/// `new globalThis.Array(3)`. The wrapper trio mints real wrapper
/// cells (§21.1.1.2 / §22.1.1.2 / §20.3.1.2 — construct differs from
/// call: `new String(sym)` runs plain ToString and throws where the
/// call form stringifies); Object / Array share their call-form
/// semantics per spec; Date covers the zero-arg (now) and one-number
/// (ms) mints; Map / Set mint fresh empties (the iterable-seed form
/// keeps a loud reject). Everything else keeps the loud TypeError.
pub(crate) unsafe fn ctor_construct(tag: i64, argv: *const u64, argc: i64) -> u64 {
    use crate::nanbox::{VALUE_UNDEFINED, box_void_ptr};
    let arg0 = || -> u64 {
        if argc > 0 {
            unsafe { argv.read() }
        } else {
            VALUE_UNDEFINED
        }
    };
    // The twelve buffer families construct through one sibling
    // (`ctor_buffer`), which owns their kernels and slot layout.
    if tag == torajs_rc::builtin_proto::FINREG_PROTO_TAG as i64 {
        return unsafe { crate::finreg::__torajs_finreg_new(arg0()) };
    }
    if tag >= 19 {
        return unsafe { super::super::ctor_buffer::construct(tag, argv, argc) };
    }
    unsafe {
        match tag {
            0 => {
                let n = if argc == 0 {
                    0.0
                } else {
                    let x = crate::nanbox_ffi::__torajs_anyv_to_number(arg0());
                    if __torajs_throw_check() != 0 {
                        return VALUE_UNDEFINED;
                    }
                    x
                };
                box_void_ptr(__torajs_number_wrapper_new(n) as *mut c_void)
            }
            3 => {
                let cell = if argc == 0 {
                    empty_str_cell()
                } else {
                    let s = crate::nanbox_ffi::__torajs_anyv_to_str(arg0());
                    if __torajs_throw_check() != 0 {
                        return VALUE_UNDEFINED;
                    }
                    s as *mut u8
                };
                box_void_ptr(__torajs_string_wrapper_new(cell) as *mut c_void)
            }
            4 => {
                let b = argc > 0 && crate::nanbox_ffi::__torajs_anyv_to_bool(arg0());
                box_void_ptr(__torajs_boolean_wrapper_new(b as u8) as *mut c_void)
            }
            1 => crate::to_object::__torajs_any_to_object(arg0()),
            2 => {
                if argc == 0 {
                    return box_void_ptr(__torajs_arr_alloc_any(0) as *mut c_void);
                }
                let n0 = arg0();
                if argc > 1 || !(crate::nanbox::is_int32(n0) || crate::nanbox::is_double(n0)) {
                    __torajs_throw_type_error(
                        c"Array constructor elements form is not supported through a first-class ctor value".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                let len = crate::nanbox_ffi::__torajs_anyv_to_number(n0);
                let arr = __torajs_arr_alloc_any_filled_f64(len);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                box_void_ptr(arr as *mut c_void)
            }
            8 => {
                if argc == 0 {
                    return box_void_ptr(__torajs_date_now());
                }
                let n0 = arg0();
                if argc > 1 || !(crate::nanbox::is_int32(n0) || crate::nanbox::is_double(n0)) {
                    __torajs_throw_type_error(
                        c"Date constructor component forms are not supported through a first-class ctor value".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                box_void_ptr(__torajs_date_from_ms(
                    crate::nanbox_ffi::__torajs_anyv_to_number(n0),
                ))
            }
            11 | 12 if argc == 0 => box_void_ptr(if tag == 11 {
                __torajs_map_create()
            } else {
                __torajs_set_create()
            }),
            _ => {
                __torajs_throw_type_error(
                    c"this constructor cannot be constructed through a first-class value yet"
                        .as_ptr(),
                );
                VALUE_UNDEFINED
            }
        }
    }
}
