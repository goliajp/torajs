//! §27.1.4 eager Iterator-helper consumers — toArray / forEach /
//! some / every / find / reduce. Split from [`crate::iter_helper`]
//! at the size cap; the bodies are that module's eager family
//! verbatim. The chain entry ([`crate::iter_helper::try_helper_chain`])
//! stays in the parent — these run over any iterator-protocol cell
//! it routes here.

use core::ffi::c_void;

use crate::method_call_closure_dispatch::{closure_cell_entry, invoke_boxed};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_pointer;
use crate::{as_void_ptr, is_cell};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
}

/// §27.1.4.10 toArray — eager: drive the receiver (any
/// iterator-protocol cell — the chain entry is shared with the lazy
/// mints) to exhaustion into a fresh Array<Any>. A step throw
/// answers undefined with the pending throw in flight.
///
/// # Safety
/// `ptr` is a live heap cell honoring the iterator protocol.
pub(crate) unsafe fn iter_to_array(ptr: *mut c_void) -> AnyValue {
    unsafe {
        // §27.1.4.10 step 3 GetIteratorDirect — one next-method Get.
        let Ok(next_av) = crate::iter_helper_next::resolve_next_method(ptr as u64) else {
            return VALUE_UNDEFINED;
        };
        let mut arr = __torajs_arr_alloc_any(0);
        loop {
            let mut item: AnyValue = VALUE_UNDEFINED;
            let hit = crate::iter_helper_next::step_via(ptr as u64, next_av, &mut item);
            if hit == 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(next_av);
                if __torajs_throw_check() != 0 {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(__torajs_anyv_box_pointer(
                        arr as *mut c_void,
                    ));
                    return VALUE_UNDEFINED;
                }
                return __torajs_anyv_box_pointer(arr as *mut c_void);
            }
            let (t, p) = (
                crate::__torajs_anyv_unbox_tag(item),
                crate::__torajs_anyv_unbox_value(item),
            );
            // push_any is transfer-shaped — the step's owned ref
            // hands over.
            arr = __torajs_arr_push_any(arr as *mut c_void, t as u64, p as u64);
        }
    }
}

/// §27.1.4 eager consumers — forEach / some / every / find /
/// reduce over any iterator-protocol cell (chain entry shared with
/// the lazy mints). Short-circuits close the underlying (§7.4.9);
/// a callback throw closes and forwards.
///
/// # Safety
/// `ptr` is a live heap cell honoring the iterator protocol; `argv`
/// holds `argc` boxed values.
pub(crate) unsafe fn iter_eager(
    ptr: *mut c_void,
    mid: i64,
    argv: *const AnyValue,
    argc: i64,
) -> AnyValue {
    unsafe {
        let fn_av = if argc >= 1 {
            argv.read()
        } else {
            VALUE_UNDEFINED
        };
        let Some((env, entry)) = (if is_cell(fn_av) {
            closure_cell_entry(as_void_ptr(fn_av) as *mut c_void)
        } else {
            None
        }) else {
            __torajs_throw_type_error(c"Iterator helper callback is not a function".as_ptr());
            return VALUE_UNDEFINED;
        };
        let recv = ptr as u64;
        // §27.1.4.x step 3 GetIteratorDirect — the callable check
        // above precedes the one next-method Get, per spec order.
        let Ok(next_av) = crate::iter_helper_next::resolve_next_method(recv) else {
            return VALUE_UNDEFINED;
        };
        // reduce seeds from the explicit initialValue or the first
        // step; an empty iterator with no initial is the §27.1.4.8
        // step 6.b TypeError.
        let mut acc: AnyValue = VALUE_UNDEFINED;
        let mut has_acc = false;
        if mid == torajs_rc::ANY_METHOD_REDUCE && argc >= 2 {
            acc = argv.add(1).read();
            // argv is borrowed — the accumulator takes its own stake.
            crate::payload_rc_inc(
                crate::__torajs_anyv_unbox_tag(acc),
                crate::__torajs_anyv_unbox_value(acc),
            );
            has_acc = true;
        }
        let mut counter: i64 = 0;
        loop {
            let mut item: AnyValue = VALUE_UNDEFINED;
            let hit = crate::iter_helper_next::step_via(recv, next_av, &mut item);
            if hit == 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(next_av);
                if __torajs_throw_check() != 0 {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(acc);
                    return VALUE_UNDEFINED;
                }
                return match mid {
                    torajs_rc::ANY_METHOD_SOME => crate::nanbox::VALUE_FALSE,
                    torajs_rc::ANY_METHOD_EVERY => crate::nanbox::VALUE_TRUE,
                    torajs_rc::ANY_METHOD_REDUCE => {
                        if !has_acc {
                            __torajs_throw_type_error(
                                c"Reduce of empty iterator with no initial value".as_ptr(),
                            );
                            return VALUE_UNDEFINED;
                        }
                        acc
                    }
                    // forEach / find land on undefined.
                    _ => VALUE_UNDEFINED,
                };
            }
            if mid == torajs_rc::ANY_METHOD_REDUCE && !has_acc {
                acc = item;
                has_acc = true;
                counter += 1;
                continue;
            }
            let counter_av = crate::__torajs_anyv_box_from_pair(2, counter) as u64;
            let argv2 = [
                if mid == torajs_rc::ANY_METHOD_REDUCE {
                    acc
                } else {
                    item
                },
                if mid == torajs_rc::ANY_METHOD_REDUCE {
                    item
                } else {
                    counter_av
                },
                counter_av,
            ];
            let n_args: i64 = if mid == torajs_rc::ANY_METHOD_REDUCE {
                3
            } else {
                2
            };
            let r = invoke_boxed(env, entry, argv2.as_ptr(), n_args);
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(r);
                if mid != torajs_rc::ANY_METHOD_REDUCE {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(acc);
                }
                crate::nanbox_ffi::__torajs_anyv_rc_dec(next_av);
                crate::iter_any_close::__torajs_iter_close_value(recv);
                return VALUE_UNDEFINED;
            }
            match mid {
                torajs_rc::ANY_METHOD_REDUCE => {
                    // r replaces acc; the consumed item and old acc
                    // release.
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(acc);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                    acc = r;
                }
                torajs_rc::ANY_METHOD_SOME => {
                    let b = crate::nanbox_ffi::__torajs_anyv_to_bool(r);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(r);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                    if b {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(next_av);
                        crate::iter_any_close::__torajs_iter_close_value(recv);
                        return crate::nanbox::VALUE_TRUE;
                    }
                }
                torajs_rc::ANY_METHOD_EVERY => {
                    let b = crate::nanbox_ffi::__torajs_anyv_to_bool(r);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(r);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                    if !b {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(next_av);
                        crate::iter_any_close::__torajs_iter_close_value(recv);
                        return crate::nanbox::VALUE_FALSE;
                    }
                }
                torajs_rc::ANY_METHOD_FIND => {
                    let b = crate::nanbox_ffi::__torajs_anyv_to_bool(r);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(r);
                    if b {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(next_av);
                        crate::iter_any_close::__torajs_iter_close_value(recv);
                        return item;
                    }
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                }
                // forEach — discard both.
                _ => {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(r);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                }
            }
            counter += 1;
        }
    }
}
