//! sort / fill / copyWithin arms of the array-like mutator lane —
//! carved out of `method_call_arraylike_mut.rs` verbatim (file-size
//! cap). Each is a self-contained §23.1.3 kernel the parent's `match
//! mid` dispatches to; they reach the shared Has/Get/Set/Delete
//! helpers and externs through the parent module.

use super::*;
use crate::callback_resolve::Callback;

/// §23.1.3.30 step 1 — the comparator is `undefined` or callable
/// (§7.2.3); `Err` = not callable. `Ok(Some)` keeps a minted thunk
/// alive for as long as it is held — across the whole sort.
unsafe fn comparator(cmp: AnyValue) -> Result<Option<Callback>, ()> {
    if is_undefined(cmp) {
        return Ok(None);
    }
    unsafe { crate::callback_resolve::resolve(cmp, crate::nanbox::VALUE_UNDEFINED) }
        .map(Some)
        .ok_or(())
}

/// The `(env, entry, has_cb)` triple the sort kernels take.
fn callback_triple(own: Option<&Callback>) -> (*mut c_void, u64, i64) {
    match own {
        None => (core::ptr::null_mut(), 0u64, 0i64),
        Some(c) => (c.env(), c.entry(), 1),
    }
}

/// `sort` over the per-index walk. `cmp` is the comparator its caller
/// already admitted (§23.1.3.30 step 1 precedes the length read).
pub(super) unsafe fn do_sort(obj: &mut *mut c_void, len: i64, cmp: Option<&Callback>) -> AnyValue {
    unsafe {
        let (cb_env, cb_entry, has_cb) = callback_triple(cmp);
        let mut tmp = __torajs_arr_alloc_any(len.clamp(0, 4096) as u64);
        let mut count: i64 = 0;
        // The staging Gets reach user getters — stop on a pending
        // throw (a fn-level return is safe: the relocate writeback
        // runs in the caller).
        let mut j = 0;
        while j < len && __torajs_throw_check() == 0 {
            if arraylike_has(*obj, j) {
                let v = arraylike_get(*obj, j);
                tmp = __torajs_arr_push_any(
                    tmp as *mut c_void,
                    __torajs_anyv_unbox_tag(v) as u64,
                    __torajs_anyv_unbox_value(v) as u64,
                );
                count += 1;
            }
            j += 1;
        }
        if __torajs_throw_check() == 0 {
            __torajs_arr_any_sort(tmp, cb_env, cb_entry, has_cb);
        }
        if __torajs_throw_check() != 0 {
            __torajs_value_drop_heap(tmp as *mut c_void);
            return VALUE_UNDEFINED;
        }
        let mut j = 0;
        while j < count && __torajs_throw_check() == 0 {
            let bv = __torajs_arr_get_any_boxed(tmp as *const c_void, j as u64);
            // Borrowed slot read → the bucket's stake.
            __torajs_rc_inc(bv as *mut c_void);
            set_at(obj, j, bv);
            j += 1;
        }
        let mut j = count;
        while j < len && __torajs_throw_check() == 0 {
            delete_at(*obj, j);
            j += 1;
        }
        __torajs_value_drop_heap(tmp as *mut c_void);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        __torajs_rc_inc(*obj);
        __torajs_anyv_box_pointer(*obj)
    }
}

/// `fill(value, start, end)` per §23.1.3.7 — relative-wrapped Sets
/// of the same borrowed value (a stake per slot).
pub(super) unsafe fn do_fill(
    obj: &mut *mut c_void,
    len: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let arg_at = |i: i64| -> u64 {
            if i < argc {
                *argv.add(i as usize)
            } else {
                VALUE_UNDEFINED
            }
        };
        let v = arg_at(0);
        let wrap = |r: i64| if r < 0 { (r + len).max(0) } else { r.min(len) };
        let lo = wrap(to_index(arg_at(1), 0));
        let hi = wrap(to_index(arg_at(2), i64::MAX));
        let mut k = lo;
        // Sets reach user setters — stop on a pending throw.
        while k < hi && __torajs_throw_check() == 0 {
            __torajs_rc_inc(v as *mut c_void);
            set_at(obj, k, v);
            k += 1;
        }
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        __torajs_rc_inc(*obj);
        __torajs_anyv_box_pointer(*obj)
    }
}

/// `copyWithin(target, start, end)` per §23.1.3.4 — direction-aware
/// Has-gated moves.
pub(super) unsafe fn do_copy_within(
    obj: &mut *mut c_void,
    len: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let arg_at = |i: i64| -> u64 {
            if i < argc {
                *argv.add(i as usize)
            } else {
                VALUE_UNDEFINED
            }
        };
        let wrap = |r: i64| if r < 0 { (r + len).max(0) } else { r.min(len) };
        let mut to = wrap(to_index(arg_at(0), 0));
        let mut from = wrap(to_index(arg_at(1), 0));
        let fin = wrap(to_index(arg_at(2), i64::MAX));
        let mut count = (fin - from).min(len - to);
        if count > 0 && from < to && to < from + count {
            // Overlap — copy backwards.
            from += count - 1;
            to += count - 1;
            // Has/Get/Set reach user accessors — stop on a pending
            // throw (both directions).
            while count > 0 && __torajs_throw_check() == 0 {
                if arraylike_has(*obj, from) {
                    let v = arraylike_get(*obj, from);
                    set_at(obj, to, v);
                } else {
                    delete_at(*obj, to);
                }
                from -= 1;
                to -= 1;
                count -= 1;
            }
        } else {
            while count > 0 && __torajs_throw_check() == 0 {
                if arraylike_has(*obj, from) {
                    let v = arraylike_get(*obj, from);
                    set_at(obj, to, v);
                } else {
                    delete_at(*obj, to);
                }
                from += 1;
                to += 1;
                count -= 1;
            }
        }
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        __torajs_rc_inc(*obj);
        __torajs_anyv_box_pointer(*obj)
    }
}

/// Can an index of this Array cell be something other than a plain own
/// data property — a shadow hole / accessor (`FLAG_ARR_EXOTIC_INDEX`) or
/// a slot hole (`FLAG_ARR_ANY_HOLEY`, meaningful only with
/// `FLAG_ARR_ANY`: bit 10 is the element kind on a typed cell)? Mirrors
/// torajs-arr `layout::needs_index_mop`.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell.
pub(crate) unsafe fn arr_needs_index_mop(arr: *const c_void) -> bool {
    const HOLEY: u16 = torajs_rc::FLAG_ARR_ANY | torajs_rc::FLAG_ARR_ANY_HOLEY;
    let flags = unsafe { (*(arr as *const torajs_rc::HeapHeader)).flags };
    flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0 || flags & HOLEY == HOLEY
}

/// `Array.prototype.sort` over an Array cell that carries a hole
/// (shadow or slot) or an accessor index. §23.1.3.30
/// SortIndexedProperties reads every index through `HasProperty` /
/// `Get` and writes back through `Set` / `DeletePropertyOrThrow`,
/// which is exactly [`do_sort`]; the raw-slot merge sort cannot
/// answer it (a hole's shadow stays where it was, and an accessor
/// never runs). Answers the receiver +1, or undefined with a pending
/// throw.
///
/// The typed sort lane reaches this through the
/// `__torajs_arr_sort_exotic` seam (default in `torajs-dispatch`),
/// which the link judgment stubs when no writer of
/// `FLAG_ARR_EXOTIC_INDEX` / `FLAG_ARR_ANY_HOLEY` is live; the any
/// lane's own sort calls the impl directly.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell; `cmp` is borrowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_sort_exotic_impl(
    arr: *mut c_void,
    cmp: AnyValue,
) -> AnyValue {
    unsafe {
        let Ok(own) = comparator(cmp) else {
            return not_callable();
        };
        if (*(arr as *const torajs_rc::HeapHeader)).flags & torajs_rc::FLAG_ARR_ANY == 0 {
            return sort_typed_holes(arr, own.as_ref());
        }
        let len = (arr.cast::<u8>().add(crate::index_any::MIRROR_ARR_LEN_OFF) as *const u64).read();
        let mut obj = arr;
        do_sort(&mut obj, len as i64, own.as_ref())
    }
}

/// A typed cell carries holes but never an accessor index
/// (`define_index_accessor` refuses one), and a boxed `Set` cannot
/// store back into every element kind (a pointer kind does not say
/// which pointer type it holds) — so torajs-arr permutes its raw
/// slots instead. Same answer as [`do_sort`].
unsafe fn sort_typed_holes(arr: *mut c_void, cmp: Option<&Callback>) -> AnyValue {
    unsafe {
        let (cb_env, cb_entry, has_cb) = callback_triple(cmp);
        __torajs_arr_sort_typed_holes(arr, cb_env, cb_entry, has_cb);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        __torajs_rc_inc(arr);
        __torajs_anyv_box_pointer(arr)
    }
}

unsafe extern "C" {
    /// torajs-arr — sort a typed cell whose holes are marked.
    fn __torajs_arr_sort_typed_holes(arr: *mut c_void, env: *mut c_void, entry: u64, has: i64);
}

/// `__torajs_arr_mutate_generic(arr, mid, argv, argc)` — `pop` /
/// `shift` / `unshift` over an Array cell whose indices need the
/// per-index walk (a hole or an accessor index,
/// [`arr_needs_index_mop`]). §23.1.3.22 / §23.1.3.27 read through
/// [[Get]] BEFORE the length Set: a getter reached through a hole runs
/// (and may freeze the array, which the Set then answers with the
/// TypeError); §23.1.3.36 moves each index up with HasProperty / Get /
/// Set / DeletePropertyOrThrow, so a hole travels as a hole. The
/// array-like arm is exactly those sequences. Answers the removed
/// element (owned) or the new length, or undefined with a pending
/// throw.
///
/// # Safety
/// `arr` is a live `Tag::Arr` cell; `mid` is `ANY_METHOD_POP`,
/// `ANY_METHOD_SHIFT` or `ANY_METHOD_UNSHIFT`; `argv` holds `argc`
/// borrowed AnyValues (NULL / 0 for pop and shift).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_mutate_generic(
    arr: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let len = (arr.cast::<u8>().add(crate::index_any::MIRROR_ARR_LEN_OFF) as *const u64).read();
        super::arraylike_mut(
            arr,
            mid,
            len as i64,
            core::ptr::null_mut(),
            argv,
            argc,
            None,
        )
    }
}
