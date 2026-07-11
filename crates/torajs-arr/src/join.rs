//! `arr.join(sep)` family. Port of
//! `runtime_str.c::__torajs_arr_join{,_i64,_f64,_bool,_substr}`
//! (P4.1-h, 2026-05-23; ES2023 `toReversed` / `with` moved to
//! `transform.rs` in the chunk 624 file-size split). Each join is
//! two-pass (sum code units + fold output encoding, then alloc +
//! emit with interleaved sep). Encoding-aware since RFC 20260711 —
//! Str payloads are Latin-1 or UTF-16 LE (P11.1-S2) and the output
//! picks the widest of the inputs; the per-piece emit lives in
//! [`crate::join_enc`]. Output Str allocation goes through
//! cross-tier [`crate::str_bridge::str_alloc_pooled_enc`] (wraps
//! `__torajs_str_alloc_pooled_enc` from libtorajs_str.a).

use core::ffi::c_void;

use crate::join_enc::{
    STR_DATA_OFF, alloc_join_out, emit_units, str_data, str_is_latin1, str_units,
};
use crate::layout::{ARR_LEN_OFF, arr_data};

const ARR_HEAD_OFF: usize = 20;

// Substr layout mirrors (Layer-2 cross-tier). `len` / `offset` are
// code-unit values (P11.1-S5); byte positions recover through the
// parent's encoding stride.
const SUBSTR_LEN_OFF: usize = 8;
const SUBSTR_PARENT_OFF: usize = 16;
const SUBSTR_OFFSET_OFF: usize = 24;

unsafe extern "C" {
    /// torajs-mmalloc libc-compat malloc — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    /// libc-compat free — pair with `malloc` for transient buffers.
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    // v0.7-A4 Step 15-d: 0-libc f64 → shortest decimal + i64 →
    // decimal via torajs-fmt. Replaces libc snprintf %.*g loop +
    // snprintf "%lld" for the i64-join path.
    fn __torajs_fmt_dtoa(d: f64, out_buf: *mut u8, out_cap: usize) -> i32;
    fn __torajs_fmt_itoa(n: i64, out_buf: *mut u8, out_cap: usize) -> i32;
    /// `ToString(v)` per ES §7.1.17. Returns a freshly-owned Str ptr
    /// the caller must drop. Defined in `torajs-anyvalue::nanbox_ffi`.
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    /// Drop a Str (rc_dec; free if rc reaches 0). Defined in
    /// `torajs-str::drop` (Layer-2 sibling).
    #[link_name = "__torajs_str_drop"]
    fn str_drop(s: *mut c_void);
}

// AnyValue NaN-box constants — match `torajs-anyvalue::nanbox`
// (VALUE_NULL = TAG_BIT_TYPE_OTHER, VALUE_UNDEFINED =
// TAG_BIT_TYPE_OTHER | TAG_BIT_UNDEFINED). Spec §22.1.3.15.5:
// undefined / null → empty String. Detect at the tag level to skip
// the alloc+drop round-trip — `__torajs_anyv_to_str` follows ES
// §7.1.17 ToString and returns "undefined" / "null" literally.
const VALUE_NULL_IMM: u64 = 0x0000_0000_0000_0002;
const VALUE_UNDEFINED_IMM: u64 = 0x0000_0000_0000_000A;

// ============================================================
// Helpers
// ============================================================

#[inline]
unsafe fn arr_len(arr: *const u8) -> u64 {
    unsafe { *(arr.add(ARR_LEN_OFF) as *const u64) }
}

#[inline]
unsafe fn arr_head(arr: *const u8) -> u32 {
    unsafe { *(arr.add(ARR_HEAD_OFF) as *const u32) }
}

#[inline]
unsafe fn slot_addr(arr: *const u8, i: u64) -> *const u8 {
    unsafe {
        let head = arr_head(arr) as usize;
        arr_data(arr).add((head + i as usize) * 8)
    }
}

/// Separator's contribution to the output encoding: it only
/// participates when it actually appears (≥ 2 elements, non-empty).
#[inline]
unsafe fn sep_latin1_folded(sep: *const u8, sep_units: u64, len: u64) -> bool {
    if len > 1 && sep_units > 0 {
        unsafe { str_is_latin1(sep) }
    } else {
        true
    }
}

/// f64 → shortest spec-correct decimal. v0.7-A4 Step 15-d:
/// delegates to torajs-fmt's `__torajs_fmt_dtoa` (0-libc;
/// core::fmt Grisu3 + JS-spec post-process). Same shortest-
/// roundtrip + ES §6.1.6.1.13 shape as the prior libc-based
/// implementation, but in a single call instead of try-
/// precisions loop.
unsafe fn f64_shortest(d: f64, buf: *mut u8, cap: usize) -> i32 {
    unsafe { __torajs_fmt_dtoa(d, buf, cap) }
}

// ============================================================
// arr_join — Array<Str>
// ============================================================

unsafe extern "C" {
    /// RFC 20260707 chunk 4 — the immortal `undefined` sentinel Str
    /// cell (torajs-str undef_sentinel.rs). A nullish elem slot
    /// joins as the empty string per ES §23.1.3.18 step 8.c, never
    /// its payload text.
    fn __torajs_str_undef() -> *mut u8;
}

/// Join length of one Str slot: nullish (NULL = JS null, the
/// undefined sentinel cell) contributes nothing per §23.1.3.18;
/// anything else is its code-unit count.
#[inline]
unsafe fn elem_join_units(elem: *const u8) -> u64 {
    if elem.is_null() || elem == unsafe { __torajs_str_undef() } as *const u8 {
        0
    } else {
        unsafe { str_units(elem) }
    }
}

/// `Array<Str>.join(sep)`. Each slot is a `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_units = str_units(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return alloc_join_out(0, true);
        }
        let mut total: u64 = 0;
        let mut out_latin1 = sep_latin1_folded(sep, sep_units, len);
        for i in 0..len {
            let elem = *(slot_addr(arr, i) as *const *const u8);
            let units = elem_join_units(elem);
            total += units;
            if units > 0 {
                out_latin1 &= str_is_latin1(elem);
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
            let elem = *(slot_addr(arr, i) as *const *const u8);
            let units = elem_join_units(elem);
            if units > 0 {
                emit_units(
                    p_data,
                    out_latin1,
                    cursor,
                    str_data(elem),
                    units,
                    str_is_latin1(elem),
                );
                cursor += units;
            }
        }
        p
    }
}

// ============================================================
// arr_join_i64 — Array<I64>
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_i64(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_units = str_units(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return alloc_join_out(0, true);
        }
        let out_latin1 = sep_latin1_folded(sep, sep_units, len);
        let sep_latin1 = str_is_latin1(sep);
        let mut buf = [0u8; 24];
        // pass 1: total (decimal digits are ASCII → Latin-1 pieces)
        let mut total: u64 = 0;
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const i64);
            let n = __torajs_fmt_itoa(e, buf.as_mut_ptr(), 24);
            total += n.max(0) as u64;
        }
        total += sep_units * (len - 1);
        let p = alloc_join_out(total, out_latin1);
        let p_data = p.add(STR_DATA_OFF);
        let mut cursor: u64 = 0;
        for i in 0..len {
            if i > 0 && sep_units > 0 {
                emit_units(p_data, out_latin1, cursor, sep_data, sep_units, sep_latin1);
                cursor += sep_units;
            }
            let e = *(slot_addr(arr, i) as *const i64);
            let n = __torajs_fmt_itoa(e, buf.as_mut_ptr(), 24);
            let n = n.max(0) as u64;
            emit_units(p_data, out_latin1, cursor, buf.as_ptr(), n, true);
            cursor += n;
        }
        p
    }
}

// ============================================================
// arr_join_f64 — Array<F64>
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_f64(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_units = str_units(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return alloc_join_out(0, true);
        }
        let out_latin1 = sep_latin1_folded(sep, sep_units, len);
        let sep_latin1 = str_is_latin1(sep);
        let mut buf = [0u8; 32];
        // pass 1: total (decimal / NaN / Infinity are ASCII pieces)
        let mut total: u64 = 0;
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const f64);
            total += if e.is_nan() {
                3 // "NaN"
            } else if e == f64::INFINITY {
                8 // "Infinity"
            } else if e == f64::NEG_INFINITY {
                9 // "-Infinity"
            } else {
                let n = f64_shortest(e, buf.as_mut_ptr(), 32);
                n.max(0) as u64
            };
        }
        total += sep_units * (len - 1);
        let p = alloc_join_out(total, out_latin1);
        let p_data = p.add(STR_DATA_OFF);
        let mut cursor: u64 = 0;
        for i in 0..len {
            if i > 0 && sep_units > 0 {
                emit_units(p_data, out_latin1, cursor, sep_data, sep_units, sep_latin1);
                cursor += sep_units;
            }
            let e = *(slot_addr(arr, i) as *const f64);
            let piece: (*const u8, u64) = if e.is_nan() {
                (b"NaN".as_ptr(), 3)
            } else if e == f64::INFINITY {
                (b"Infinity".as_ptr(), 8)
            } else if e == f64::NEG_INFINITY {
                (b"-Infinity".as_ptr(), 9)
            } else {
                let n = f64_shortest(e, buf.as_mut_ptr(), 32);
                (buf.as_ptr(), n.max(0) as u64)
            };
            emit_units(p_data, out_latin1, cursor, piece.0, piece.1, true);
            cursor += piece.1;
        }
        p
    }
}

// ============================================================
// arr_join_bool — Array<Bool>
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_join_bool(arr: *const u8, sep: *const u8) -> *mut u8 {
    unsafe {
        let len = arr_len(arr);
        let sep_units = str_units(sep);
        let sep_data = str_data(sep);
        if len == 0 {
            return alloc_join_out(0, true);
        }
        let out_latin1 = sep_latin1_folded(sep, sep_units, len);
        let sep_latin1 = str_is_latin1(sep);
        let mut total: u64 = 0;
        for i in 0..len {
            let e = *(slot_addr(arr, i) as *const i64);
            total += if e != 0 { 4 } else { 5 };
        }
        total += sep_units * (len - 1);
        let p = alloc_join_out(total, out_latin1);
        let p_data = p.add(STR_DATA_OFF);
        let mut cursor: u64 = 0;
        for i in 0..len {
            if i > 0 && sep_units > 0 {
                emit_units(p_data, out_latin1, cursor, sep_data, sep_units, sep_latin1);
                cursor += sep_units;
            }
            let e = *(slot_addr(arr, i) as *const i64);
            let (piece, units): (*const u8, u64) = if e != 0 {
                (b"true".as_ptr(), 4)
            } else {
                (b"false".as_ptr(), 5)
            };
            emit_units(p_data, out_latin1, cursor, piece, units, true);
            cursor += units;
        }
        p
    }
}

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
        // pass 1: ToString each slot, cache the resulting Str ptrs
        // (NULL for undefined/null which contribute the empty string).
        let tmp_bytes = (len as usize) * core::mem::size_of::<*mut c_void>();
        let tmp = malloc(tmp_bytes) as *mut *mut c_void;
        let mut total: u64 = 0;
        let mut out_latin1 = sep_latin1_folded(sep, sep_units, len);
        for i in 0..len {
            let av = if is_any {
                *(slot_addr(arr, i) as *const u64)
            } else {
                crate::any_typed_bridge::typed_slot_anyvalue_borrowed(arr, i)
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
