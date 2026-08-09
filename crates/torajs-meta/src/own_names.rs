//! W-N-b — `Object.getOwnPropertyNames(arr)` Arr-receiver path.
//! Spec §22.1.3.5: returns `["0", ..., "<len-1>", "length"]`. tora's
//! SSA-lower handles `Object.getOwnPropertyNames(struct-obj)` at
//! compile time (static field list) but Arr<T> requires a runtime
//! walk because len is dynamic. Builds an owned `Arr<Str>` cell via
//! `__torajs_arr_alloc(len + 1)` + N pushes of `__torajs_i64_to_str`
//! results + a final push of the `"length"` key. Caller receives a
//! +1-rc Arr<Str> ptr (returned through `Type::Arr<Str>` at the SSA
//! layer; LLVM ABI keeps it interchangeable with `Type::Ptr`).

use core::ffi::{c_char, c_void};

use crate::obj_own_keys::{
    ARR_PROPS_OFF, TAG_ARR_CELL, TAG_BOOLEAN_WRAPPER, TAG_CLOSURE_CELL, TAG_NUMBER_WRAPPER,
    TAG_STRING_WRAPPER, WRAPPER_PROPS_OFF, heap_type_tag_local, is_dynobj_imm,
};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    /// torajs-dynobj iteration surface — keys are BORROWED.
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    /// §10.1.11.1's symbol bucket (own symbol keys, insertion order).
    fn __torajs_dynobj_iter_symbol_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    /// Per-entry W/E/C descriptor flags — the enumerable filter.
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_i64_to_str(n: i64) -> *mut u8;
    /// torajs-arr — per-index attribute flags (chunk C filter).
    fn __torajs_arr_index_flags(arr: *const c_void, idx: u64) -> u64;
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_at(s: *const u8, i: i64) -> *mut u8;
    fn __torajs_rc_inc(p: *mut c_void);
}

#[inline]
unsafe fn alloc_str_literal(name: &[u8]) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(name.len() as u64) };
    if !name.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), s.add(16), name.len()) };
    }
    s
}

/// Build the `Arr<Str>` name list for `Object.getOwnPropertyNames(arr)`.
/// SSA-lower pre-Loads `arr.len` from `ARR_LEN_OFF=8` so this helper
/// only needs the length to do its alloc-and-push loop.
///
/// # Safety
///
/// `extern "C"` ABI. Returned pointer owns a fresh `+1`-rc `Arr<Str>`
/// of length `len + 1`. The caller transfers that ownership into the
/// SSA value flow.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_strs(len: i64) -> *mut c_void {
    let mut out = unsafe { __torajs_arr_alloc((len as u64) + 1) };
    for i in 0..len {
        let s = unsafe { __torajs_i64_to_str(i) };
        out = unsafe { __torajs_arr_push(out, s as i64) };
    }
    let s_len = unsafe { alloc_str_literal(b"length") };
    out = unsafe { __torajs_arr_push(out, s_len as i64) };
    out as *mut c_void
}

/// Pointer-taking twin of [`__torajs_arr_index_strs`] for the gOPN
/// surface (RFC 20260713 chunk C) — an exotic array skips deleted
/// (hole) indices; non-enumerable live indices stay listed (gOPN
/// doesn't filter enumerability). A plain array keeps the zero-probe
/// bulk mint.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_strs_of(arr: *const c_void) -> *mut c_void {
    let len = unsafe { arr_walk_bound(arr) };
    let exotic = unsafe { (arr.cast::<u8>().add(6) as *const u16).read() } & (1 << 15) != 0;
    if !exotic {
        return unsafe { __torajs_arr_index_strs(len) };
    }
    let mut out = unsafe { __torajs_arr_alloc((len.max(0) as u64) + 1) };
    for i in 0..len {
        if unsafe { __torajs_arr_index_flags(arr, i as u64) } & ARR_F_HOLE != 0 {
            continue;
        }
        let s = unsafe { __torajs_i64_to_str(i) };
        out = unsafe { __torajs_arr_push(out, s as i64) };
    }
    let s_len = unsafe { alloc_str_literal(b"length") };
    out = unsafe { __torajs_arr_push(out, s_len as i64) };
    out as *mut c_void
}

/// `arr_index_flags` result bit 3 — deleted index (hole;
/// `torajs_arr::define::F_HOLE` mirror, RFC 20260713 chunk C).
const ARR_F_HOLE: u64 = 1 << 3;

/// `torajs_rc::FLAG_ARR_SPARSE_TAIL` mirror (bit 6, RFC
/// 20260810-arr-sparse-grow) — same local-mirror shape as the exotic
/// bit read below.
const ARR_F_SPARSE_TAIL: u16 = 1 << 6;

/// Materialized-extent bound for the index walks — `len`, unless the
/// cell carries a sparse tail: `[extent, len)` is implicit holes with
/// no own entry, so both key surfaces stop at the extent (`cap`
/// doubles as the extent under the sparse invariant, torajs-arr
/// `layout::arr_live_extent` mirror). Semantically equal to probing
/// the tail per index (the funnel answers F_HOLE for all of it) —
/// this keeps the walk O(extent) instead of O(len).
unsafe fn arr_walk_bound(arr: *const c_void) -> i64 {
    let len = unsafe { (arr.cast::<u8>().add(8) as *const u64).read() };
    let flags = unsafe { (arr.cast::<u8>().add(6) as *const u16).read() };
    if flags & ARR_F_SPARSE_TAIL != 0 {
        let cap = unsafe { (arr.cast::<u8>().add(16) as *const u32).read() } as u64;
        if cap < len {
            return cap as i64;
        }
    }
    len as i64
}

/// `torajs_dynobj::layout::BUCKET_FLAG_ENUMERABLE` mirror (bit 1) —
/// the descriptor bit `Object.assign`'s CopyDataProperties filters on.
const DYNOBJ_FLAG_ENUMERABLE: u64 = 1 << 1;

/// W-N-b' — `Object.keys(arr)` Arr-receiver path. Spec §22.1.3.16 +
/// §10.4.2.OrdinaryOwnPropertyKeys: keys() filters to enumerable own.
/// Array's `length` is non-enumerable (spec §10.4.2.4 step 4 sets
/// configurable/writable but never enumerable), so `Object.keys(arr)`
/// returns `["0", ..., "<len-1>"]` without the trailing "length" —
/// the diff against `getOwnPropertyNames(arr)`, which DOES include it.
/// Same alloc-and-push loop as `__torajs_arr_index_strs` but caps at
/// `len` instead of `len + 1` and skips the final length push.
///
/// # Safety
///
/// `extern "C"` ABI. Returned pointer owns a fresh `+1`-rc `Arr<Str>`
/// of length `len`.
/// RFC 20260712-arr-exotic-define chunk C — pointer-taking twin of
/// [`__torajs_arr_keys_only`]: an exotic array (per-index attribute
/// shadow entries) filters `enumerable: false` indices out of the
/// `Object.keys` / for-in surface; a plain array keeps the zero-probe
/// bulk mint.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_keys_only_of(arr: *const c_void) -> *mut c_void {
    let len = unsafe { arr_walk_bound(arr) };
    let exotic = unsafe { (arr.cast::<u8>().add(6) as *const u16).read() } & (1 << 15) != 0;
    if !exotic {
        return unsafe { __torajs_arr_keys_only(len) };
    }
    let mut out = unsafe { __torajs_arr_alloc(len.max(0) as u64) };
    for i in 0..len {
        if unsafe { __torajs_arr_index_flags(arr, i as u64) } & 0x2 != 0 {
            let s = unsafe { __torajs_i64_to_str(i) };
            out = unsafe { __torajs_arr_push(out, s as i64) };
        }
    }
    out as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_keys_only(len: i64) -> *mut c_void {
    let mut out = unsafe { __torajs_arr_alloc(len.max(0) as u64) };
    for i in 0..len {
        let s = unsafe { __torajs_i64_to_str(i) };
        out = unsafe { __torajs_arr_push(out, s as i64) };
    }
    out as *mut c_void
}

/// W-N-d' — `Object.keys(str)` Str-receiver path. Same shape as the
/// Arr keys-only arm (`["0", ..., "<len-1>"]`, no "length"): String's
/// `length` is non-enumerable per spec §22.1.5.1, so the enumerable-own
/// walk returns indexed chars only. Thin wrapper reads u32 len at
/// STR_LEN_OFF=8 then delegates to `arr_keys_only`.
///
/// # Safety
///
/// `str_ptr` must point at a valid Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_keys_only(str_ptr: *const c_void) -> *mut c_void {
    // SAFETY: STR_LEN_OFF=8 holds the live u32 length per torajs-str layout.
    let len = unsafe { (str_ptr.cast::<u8>().add(8) as *const u32).read() } as i64;
    unsafe { __torajs_arr_keys_only(len) }
}

/// W-N-d — `Object.getOwnPropertyNames(str)` Str-receiver path.
/// Same result shape as the Arr arm (`["0", ..., "<len-1>", "length"]`,
/// spec §22.1.5.2.4: string's own enumerable properties are the index
/// chars + the inherited-but-listed `length`). Thin wrapper that
/// reads the u32 length at `STR_LEN_OFF=8` then delegates.
///
/// # Safety
///
/// `str_ptr` must point at a valid Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_index_strs(str_ptr: *const c_void) -> *mut c_void {
    // SAFETY: STR_LEN_OFF=8 holds the live u32 length per torajs-str layout.
    let len = unsafe { (str_ptr.cast::<u8>().add(8) as *const u32).read() } as i64;
    unsafe { __torajs_arr_index_strs(len) }
}

/// W-O-3-str — `Object.entries(str)` per-character entry pairs.
/// Spec §22.1.5.2 + §20.1.2.5: ToObject on a primitive string +
/// own-keys walk enumerates the per-char indexed view as
/// `[["0", s[0]], ["1", s[1]], ...]`. Outer Arr<Arr<Any>> length
/// = str.length; each inner = `arr_alloc_any(2)` with
/// `[idx_str_as_ANY_HEAP, char_str_as_ANY_HEAP]`. Uses
/// `__torajs_str_at` (fresh Str via alloc_str_slice) so the
/// resulting Strs round-trip cleanly through console.log + dynobj
/// stores — same materialize choice as W-O-2 and W-M-rest.
///
/// # Safety
///
/// `str_ptr` must point at a valid Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_entries(str_ptr: *const c_void) -> *mut c_void {
    // SAFETY: STR_LEN_OFF=8 holds the live u32 length per torajs-str layout.
    let len = unsafe { (str_ptr.cast::<u8>().add(8) as *const u32).read() } as i64;
    let mut outer = unsafe { __torajs_arr_alloc(len.max(0) as u64) };
    for i in 0..len {
        let idx_str = unsafe { __torajs_i64_to_str(i) };
        let ch = unsafe { __torajs_str_at(str_ptr.cast::<u8>(), i) };
        let inner = unsafe { __torajs_arr_alloc_any(2) };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, 4, idx_str as u64) };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, 4, ch as u64) };
        outer = unsafe { __torajs_arr_push(outer, inner as i64) };
    }
    outer as *mut c_void
}

/// W-O-3 — `Object.entries(arr)` runtime entry-pair builder. Spec
/// §20.1.2.5 + ToObject on an Array exotic enumerates the indexed
/// own properties as `[[idx_str, value], ...]`. Builds an outer
/// `Arr<Arr<Any>>` of length `arr.len`; each inner is an
/// `arr_alloc_any(2)` with `[idx_str_as_ANY_HEAP, value_as_<tag>]`.
/// `val_tag` is the NaN-box AnyValue tag for the array's element
/// type (1=Bool, 2=I64, 3=F64, 4=ANY_HEAP); the SSA arm picks the
/// right tag statically from the typed Arr's element type so this
/// helper stays element-type agnostic.
///
/// rc accounting: ANY_HEAP slot values get an extra rc_inc before
/// push_any because each inner Arr<Any> owns its share, matching
/// the push_any owning-ref contract used by every other producer.
///
/// # Safety
///
/// `arr_ptr` must point at a valid `Array<T>` heap block with
/// 8-byte slot stride (NOT `Array<Any>` — that needs an entries_any
/// helper with the 16-byte slot stride).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_entries_by_tag(
    arr_ptr: *const c_void,
    val_tag: i64,
) -> *mut c_void {
    let base = arr_ptr.cast::<u8>();
    // SAFETY: ARR_LEN_OFF=8 u64 + ARR_HEAD_OFF=20 u32 per torajs-arr layout.
    let len = unsafe { (base.add(8) as *const u64).read() } as i64;
    let head = unsafe { (base.add(20) as *const u32).read() } as i64;
    let mut outer = unsafe { __torajs_arr_alloc(len.max(0) as u64) };
    // B1 (RFC 20260706-arr-grow-alias-stability): slots live behind
    // the data pointer at +32; the loop only calls push/push_any on
    // OTHER arrays, so src's data pointer is loop-invariant.
    let data = unsafe { (base.add(32) as *const *const u8).read() };
    for i in 0..len {
        // Raw slot value as i64 (8-byte stride; F64 / Ptr round-trip
        // via i64 bits is ABI-safe since LLVM stores both in the
        // same machine word).
        let slot_off = ((head + i) as usize) * 8;
        let slot = unsafe { (data.add(slot_off) as *const i64).read() };
        if val_tag == 4 {
            // ANY_HEAP — push_any takes an owning ref; bump rc so
            // the original Arr's drop won't tear the cell out.
            unsafe { __torajs_rc_inc(slot as *mut c_void) };
        }
        let idx_str = unsafe { __torajs_i64_to_str(i) };
        let inner = unsafe { __torajs_arr_alloc_any(2) };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, 4, idx_str as u64) };
        let inner =
            unsafe { __torajs_arr_push_any(inner as *mut c_void, val_tag as u64, slot as u64) };
        outer = unsafe { __torajs_arr_push(outer, inner as i64) };
    }
    outer as *mut c_void
}

/// W-O-2 — `Object.values(str)` per-character Str array. Spec
/// §22.1.5.2 + §20.1.2.20: ToObject on a primitive string materializes
/// the indexed-property view, whose values are the per-char fresh
/// Strs. Loops `__torajs_str_at(str_ptr, i)` to mint one fresh Str
/// per code unit (the same materialize path used by W-M-rest's
/// numeric-index descriptor — avoids the Substr view dynobj-store
/// round-trip trap that L3b had to fix separately for FLAG_SUBSTR_VIEW).
///
/// # Safety
///
/// `str_ptr` must point at a valid Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_char_arr(str_ptr: *const c_void) -> *mut c_void {
    // SAFETY: STR_LEN_OFF=8 holds the live u32 length per torajs-str layout.
    let len = unsafe { (str_ptr.cast::<u8>().add(8) as *const u32).read() } as i64;
    let mut out = unsafe { __torajs_arr_alloc(len.max(0) as u64) };
    for i in 0..len {
        let ch = unsafe { __torajs_str_at(str_ptr.cast::<u8>(), i) };
        out = unsafe { __torajs_arr_push(out, ch as i64) };
    }
    out as *mut c_void
}

/// The property dict backing a receiver's own symbol keys, borrowed —
/// the cell itself for a DynObj, else the in-layout expando slot the
/// shape carries (`Arr` / `Closure` at `+24`, primitive wrappers at
/// `+16`). `None` for a non-cell payload or a shape with no dict yet
/// (a bare struct cell degrades to DynObj on its first define, so it
/// cannot be holding a symbol key).
///
/// # Safety
/// `obj_any` carries a valid AnyValue bit pattern.
unsafe fn own_symbol_dict_borrowed(obj_any: u64) -> Option<*const c_void> {
    if is_dynobj_imm(obj_any) {
        return Some(obj_any as *const c_void);
    }
    // Cell-imm shape test, same bar as `is_dynobj_imm`: only a real
    // heap-pointer bit pattern may have its header read.
    if obj_any == 0 || obj_any & 0xFFFF_0000_0000_0000 != 0 || obj_any & 0x2 != 0 {
        return None;
    }
    let cell = obj_any as *const c_void;
    let props_off = match unsafe { heap_type_tag_local(cell) } {
        TAG_ARR_CELL | TAG_CLOSURE_CELL => ARR_PROPS_OFF,
        TAG_NUMBER_WRAPPER | TAG_STRING_WRAPPER | TAG_BOOLEAN_WRAPPER => WRAPPER_PROPS_OFF,
        _ => return None,
    };
    let dict = unsafe { *(cell.cast::<u8>().add(props_off) as *const *const c_void) };
    if dict.is_null() { None } else { Some(dict) }
}

/// W-N-c — `Object.getOwnPropertySymbols(o)` Any-receiver path.
/// §20.1.2.10 → OrdinaryOwnPropertyKeys' third bucket: own **symbol**
/// keys in insertion order, read off
/// `__torajs_dynobj_iter_symbol_order` (the string buckets belong to
/// `iter_order`, which never reports a symbol). Non-enumerable symbol
/// keys are included — gOPS lists keys, it does not filter on
/// enumerability.
///
/// ToObject on `undefined` / `null` still throws per §20.1.2.10 →
/// step 1 (gOPD guard shape; pending-throw model, so a valid array is
/// still returned for the caller's value flow).
///
/// # Safety
///
/// `obj_any` carries a valid AnyValue bit pattern. Returned pointer
/// owns a fresh `+1`-rc `Arr` of Symbol cells, each `+1`-rc'd for the
/// array slot (the dynobj's key share stays its own).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_symbols(obj_any: u64) -> *mut c_void {
    if obj_any == crate::reflect::VALUE_UNDEFINED_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe { __torajs_throw_type_error(c"undefined is not an object".as_ptr()) };
    } else if obj_any == crate::reflect::VALUE_NULL_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe { __torajs_throw_type_error(c"null is not an object".as_ptr()) };
    }
    unsafe { own_symbols_arr(obj_any, 1) }
}

/// §20.1.2.1 step 4.b.i — the own **enumerable** symbol keys, for
/// `Object.assign` / CopyDataProperties, which filters on
/// `[[Enumerable]]` where `getOwnPropertySymbols` does not. Nullish
/// receivers are the caller's business here (assign's step 4.a treats a
/// nullish SOURCE as contributing nothing, without a throw).
///
/// # Safety
///
/// `obj_any` carries a valid AnyValue bit pattern. Returned pointer
/// owns a fresh `+1`-rc `Arr` of Symbol cells, each `+1`-rc'd for the
/// array slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_enum_symbols(obj_any: u64) -> *mut c_void {
    unsafe { own_symbols_arr(obj_any, 0) }
}

/// Shared walk behind both symbol-key faces — §10.1.11.1's third
/// bucket off `__torajs_dynobj_iter_symbol_order`, optionally filtered
/// to enumerable entries. `include_nonenum = 0` keeps only enumerable
/// keys.
///
/// # Safety
/// `obj_any` carries a valid AnyValue bit pattern.
unsafe fn own_symbols_arr(obj_any: u64, include_nonenum: i64) -> *mut c_void {
    let mut out = unsafe { __torajs_arr_alloc(0) };
    let Some(dict) = (unsafe { own_symbol_dict_borrowed(obj_any) }) else {
        return out as *mut c_void;
    };
    let len = unsafe { __torajs_dynobj_iter_len(dict) };
    let mut order = vec![0u64; len as usize];
    let n = unsafe { __torajs_dynobj_iter_symbol_order(dict, order.as_mut_ptr(), len) };
    for &i in order.iter().take(n as usize) {
        if include_nonenum == 0
            && unsafe { __torajs_dynobj_iter_flags(dict, i) } & DYNOBJ_FLAG_ENUMERABLE == 0
        {
            continue;
        }
        let key = unsafe { __torajs_dynobj_iter_key(dict, i) };
        if key.is_null() {
            continue;
        }
        // Borrowed key → the array slot takes its own share.
        unsafe { __torajs_rc_inc(key) };
        out = unsafe { __torajs_arr_push(out, key as i64) };
    }
    out as *mut c_void
}
