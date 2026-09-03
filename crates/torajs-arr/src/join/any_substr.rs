//! Tail kernels of [`super`]'s join family — `arr_join_any` (the
//! kind-aware ToString walk every exotic receiver delegates to) and
//! `arr_join_substr`. Split out of the parent under the 500-line file
//! discipline (刀 5 G3 pushed `join.rs` over); a child module reaches
//! its parent's private items, so the extern block, the NaN-box
//! sentinels and the shared two-pass helpers all stay private with no
//! visibility churn.

use core::ffi::c_void;

use super::*;

// ============================================================
// arr_join_any — Array<Any>
// ============================================================

/// `Array<Any>.join(sep)`. Each slot is a NaN-box `AnyValue` u64
/// (Step 7e-A — 8-byte stride matches the typed-tier helpers, so
/// `slot_addr` reuses without a custom stride helper).
///
/// Spec §22.1.3.15.5: per-element ToString is delegated to
/// `__torajs_anyv_to_str` (`torajs-anyvalue::nanbox_ffi`), which
/// honors ES §7.1.17 — but Array.join overrides ToString for
/// undefined / null to the empty string, so this layer special-
/// cases the two sentinels before the helper call to skip the
/// "undefined" / "null" literal that `anyv_to_str` would produce
/// and to elide the alloc+drop round-trip.
///
/// Per-element Str alloc must be dropped after copy — the joined
/// result owns the final bytes; the temporaries are transient.
/// Holds the temp ptrs in a heap Vec (rather than a second pass
/// recomputing each ToString) so each slot's ToString runs once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_any(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        // RFC 20260810 刀 D — a sparse tail would ToString ~len
        // slots; loud reject until join grows real sparse support.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.join\0".as_ptr(),
        ) {
            return alloc_join_out(0, true);
        }
        let len = arr_len(arr);
        let sep_units = str_units(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return alloc_join_out(0, true);
        }
        // RFC 20260707 chunk 624 — a typed block behind the static
        // Arr<Any> view reboxes per elem kind (a raw bit-1-clear i64
        // passed anyv_to_str's cell predicate: deref'd, SIGSEGV).
        let is_any = (*(arr as *const torajs_rc::HeapHeader)).flags & torajs_rc::FLAG_ARR_ANY != 0;
        // 刀 5 G3 — exotic receivers take the kind-aware slow read
        // per element (accessor getters run, holes consult the
        // prototype digit keys); the answer is owned and drops after
        // its ToString.
        let exotic = is_exotic(arr);
        // pass 1: ToString each slot, cache the resulting Str ptrs
        // (NULL for undefined/null which contribute the empty string).
        let tmp_bytes = (len as usize) * core::mem::size_of::<*mut c_void>();
        let tmp = malloc(tmp_bytes) as *mut *mut c_void;
        let mut total: u64 = 0;
        let mut out_latin1 = sep_latin1_folded(sep, sep_units, len);
        for i in 0..len {
            let (av, owned) = if exotic {
                (
                    crate::index_any::__torajs_arr_index_get(arr as *const c_void, i as i64),
                    true,
                )
            } else if is_any {
                (*(slot_addr(arr, i) as *const u64), false)
            } else {
                (
                    crate::any_typed_bridge::typed_slot_anyvalue_borrowed(arr, i),
                    false,
                )
            };
            if av == VALUE_NULL_IMM || av == VALUE_UNDEFINED_IMM {
                *tmp.add(i as usize) = core::ptr::null_mut();
            } else {
                let s = __torajs_anyv_to_str(av);
                *tmp.add(i as usize) = s;
                let units = str_units(s as *const u8);
                total += units;
                if units > 0 {
                    out_latin1 &= str_is_latin1(s as *const u8);
                }
            }
            if owned {
                __torajs_value_drop_heap(av as *mut c_void);
            }
            // §23.1.3.18 step 5.d does ToString(element), and that
            // runs user code: an own `toString` can throw anything,
            // and §7.1.17 step 2 makes a Symbol element throw by
            // itself. Walking on past a pending throw finished the
            // join and handed back a string, so the element's
            // exception was swallowed and the loop kept calling more
            // user methods after it. Stop where the spec stops, drop
            // what pass 1 already owns, and answer a type-correct
            // empty Str — the caller's throw check unwinds.
            if __torajs_throw_check() != 0 {
                for k in 0..=i {
                    let t = *tmp.add(k as usize);
                    if !t.is_null() {
                        str_drop(t);
                    }
                }
                free(tmp as *mut c_void);
                return alloc_join_out(0, true);
            }
        }
        total += sep_units * (len - 1);
        let p = alloc_join_out(total, out_latin1);
        let p_data = p.add(STR_DATA_OFF);
        let sep_latin1 = str_is_latin1(sep);
        let mut cursor: u64 = 0;
        for i in 0..len {
            if i > 0 && sep_units > 0 {
                emit_units(p_data, out_latin1, cursor, sep_data, sep_units, sep_latin1);
                cursor += sep_units;
            }
            let s = *tmp.add(i as usize);
            if !s.is_null() {
                let s_u8 = s as *const u8;
                let units = str_units(s_u8);
                if units > 0 {
                    emit_units(
                        p_data,
                        out_latin1,
                        cursor,
                        str_data(s_u8),
                        units,
                        str_is_latin1(s_u8),
                    );
                    cursor += units;
                }
                str_drop(s);
            }
        }
        free(tmp as *mut c_void);
        p
    }
}

// ============================================================
// arr_join_substr — Array<Substr>
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_substr(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_units = str_units(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return alloc_join_out(0, true);
        }
        // pass 1: total units + fold the parents' encodings.
        let mut total: u64 = 0;
        let mut out_latin1 = sep_latin1_folded(sep, sep_units, len);
        for i in 0..len {
            let v = *(slot_addr(arr, i) as *const *const u8);
            let units = *(v.add(SUBSTR_LEN_OFF) as *const u64);
            total += units;
            if units > 0 {
                let parent = *(v.add(SUBSTR_PARENT_OFF) as *const *const u8);
                out_latin1 &= str_is_latin1(parent);
            }
        }
        total += sep_units * (len - 1);
        let p = alloc_join_out(total, out_latin1);
        let p_data = p.add(STR_DATA_OFF);
        let sep_latin1 = str_is_latin1(sep);
        let mut cursor: u64 = 0;
        for i in 0..len {
            if i > 0 && sep_units > 0 {
                emit_units(p_data, out_latin1, cursor, sep_data, sep_units, sep_latin1);
                cursor += sep_units;
            }
            let v = *(slot_addr(arr, i) as *const *const u8);
            let units = *(v.add(SUBSTR_LEN_OFF) as *const u64);
            if units > 0 {
                let parent = *(v.add(SUBSTR_PARENT_OFF) as *const *const u8);
                let cu_off = *(v.add(SUBSTR_OFFSET_OFF) as *const u64) as usize;
                let parent_latin1 = str_is_latin1(parent);
                // Byte position of the view start recovers through
                // the PARENT's stride (Substr offset/len are
                // code-unit values, P11.1-S5).
                let stride = if parent_latin1 { 1 } else { 2 };
                let src = parent.add(STR_DATA_OFF + cu_off * stride);
                emit_units(p_data, out_latin1, cursor, src, units, parent_latin1);
                cursor += units;
            }
        }
        p
    }
}
