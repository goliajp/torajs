//! `new` through a first-class builtin constructor value — split from
//! the parent so the per-tag table has room to grow. The parent
//! answers what a builtin constructor IS as a value (the interned
//! cells, their names, the call form); this answers what `new` on one
//! of them mints. Body verbatim; a child module reaches the parent's
//! private externs and helpers directly.

use super::*;
use torajs_rc::collection_kind::{
    COLLECTION_MAP, COLLECTION_SET, COLLECTION_WEAKMAP, COLLECTION_WEAKSET,
};

unsafe extern "C" {
    /// torajs-weak — fresh empty WeakMap / WeakSet cells (rc 1).
    fn __torajs_weakmap_create() -> *mut c_void;
    fn __torajs_weakset_create() -> *mut c_void;
}

/// [[Construct]] of a builtin-constructor value — `new N(5)` through
/// a `const N: any = Number` binding, `new (Array.bind(null))(3)`,
/// `new globalThis.Array(3)`. The wrapper trio mints real wrapper
/// cells (§21.1.1.2 / §22.1.1.2 / §20.3.1.2 — construct differs from
/// call: `new String(sym)` runs plain ToString and throws where the
/// call form stringifies); Object / Array share their call-form
/// semantics per spec; Date covers the zero-arg (now) and one-number
/// (ms) mints; the four collections mint fresh cells and seed them
/// from the optional iterable (§24.1.1.1 / §24.2.1.1 / §24.3.1.1 /
/// §24.4.1.1); WeakRef is §26.1.1.1; Promise is §27.2.3.1 over the
/// subclass `super(executor)` kernel; Symbol and BigInt throw the
/// TypeError their own step 1 prescribes for a `new`. Everything else
/// keeps the loud TypeError.
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
            11 => seed_collection(__torajs_map_create(), COLLECTION_MAP, argc, arg0()),
            12 => seed_collection(__torajs_set_create(), COLLECTION_SET, argc, arg0()),
            16 => seed_collection(__torajs_weakmap_create(), COLLECTION_WEAKMAP, argc, arg0()),
            17 => seed_collection(__torajs_weakset_create(), COLLECTION_WEAKSET, argc, arg0()),
            // §27.2.3.1 — executor first, then the mint
            // (`promise_subclass::promise_base_construct`).
            10 => crate::promise_subclass::promise_base_construct(arg0()),
            // §20.4.1 / §21.2.1 — both carry [[Construct]] (a class may
            // extend them) and both throw when NewTarget is not
            // undefined, which `new` always makes it.
            5 => {
                __torajs_throw_type_error(c"Symbol is not a constructor".as_ptr());
                VALUE_UNDEFINED
            }
            6 => {
                __torajs_throw_type_error(c"BigInt is not a constructor".as_ptr());
                VALUE_UNDEFINED
            }
            // §22.2.4.1 step 2 — with a NewTarget present the
            // identity shortcut is skipped and the pattern is always
            // rebuilt; the body itself is shared with the call form.
            REGEXP_PROTO_FAMILY => crate::dispatch_seam::__torajs_ctor_regexp_build(
                if argc > 0 {
                    argv.read()
                } else {
                    VALUE_UNDEFINED
                },
                if argc > 1 {
                    argv.add(1).read()
                } else {
                    VALUE_UNDEFINED
                },
            ),
            18 => {
                let cell = crate::method_call_weak::__torajs_weakref_new_checked(arg0());
                if __torajs_throw_check() != 0 {
                    __torajs_value_drop_heap(cell);
                    return VALUE_UNDEFINED;
                }
                box_void_ptr(cell)
            }
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

/// A fresh collection cell (rc 1) seeded from the constructor's
/// optional iterable — the §24.1.1.1 steps 5–9 shared by all four
/// families (`collection_init`). An abrupt seed releases the cell and
/// leaves the throw pending for the caller's check.
unsafe fn seed_collection(cell: *mut c_void, kind: i64, argc: i64, iterable: u64) -> u64 {
    use crate::nanbox::{VALUE_UNDEFINED, box_void_ptr};
    let target = box_void_ptr(cell);
    if argc == 0 {
        return target;
    }
    unsafe {
        crate::collection_init::__torajs_collection_init_from_iterable(target, iterable, kind);
        if __torajs_throw_check() != 0 {
            __torajs_value_drop_heap(cell);
            return VALUE_UNDEFINED;
        }
    }
    target
}
