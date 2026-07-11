//! RC-4 F1c — runtime chooser for `Object.keys` /
//! `Object.getOwnPropertyNames` / `Reflect.ownKeys` on struct-typed
//! receivers.
//!
//! `Object.defineProperty` converts a struct receiver to a DynObj and
//! rebinds the binding (`emit_any_dynobj_writeback`), so the
//! compile-time field list the SSA arm emits can be stale — a
//! runtime-defined property (test262 gOPN accessor family) was
//! invisible to reflection. The SSA arm still builds the static list
//! (correct for a plain struct cell, zero-cost reflection), then
//! routes through [`__torajs_obj_own_keys`]:
//!
//! - receiver is a DynObj cell → drop the static list and build the
//!   key array from the live entry walk in ES §10.1.11.1 order
//!   (array-index keys ascending, then insertion order).
//!   `include_nonenum = 0` (`Object.keys`) filters enumerable-only;
//!   `1` (`getOwnPropertyNames` / `ownKeys`) includes every key.
//! - anything else → return the static list as-is.

use core::ffi::c_void;

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    fn __torajs_rc_inc(p: *mut c_void);
    /// `runtime_str.c` universal-drop dispatcher (settles the unused
    /// static list on the DynObj path).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-dynobj iteration surface — keys are BORROWED; values
    /// are the bucket's NaN-box AnyValue (borrowed).
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    /// torajs-throw — ToObject on null / undefined (§20.1.2.17 step 1).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-anyvalue box decode — `unbox_value` is a borrow read;
    /// `unbox_value_owned` fuses materialize + rc_inc so the slot
    /// owns its share (chunk 610 owned-unbox protocol).
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_unbox_value_owned(v: u64) -> i64;
    /// torajs-dynobj accessor — runs the getter, answers an OWNED box.
    fn __torajs_accessor_invoke_getter(pair: *const c_void) -> u64;
}

/// `HeapHeader::type_tag` mirror of `torajs_rc::Tag::DynObj` (locked
/// there); header field lives at byte offset 4.
const TAG_DYNOBJ: u16 = 14;
const HDR_TYPE_TAG_OFF: usize = 4;

/// `torajs_rc::Tag` mirrors for the ToObject dispatch arms
/// (chunk B1 — for-in RFC): Str / Arr / Closure / Obj cells each get
/// their own own-keys shape instead of the former non-struct throw.
const TAG_STR_CELL: u16 = 0;
const TAG_OBJ_CELL: u16 = 1;
const TAG_ARR_CELL: u16 = 2;
const TAG_CLOSURE_CELL: u16 = 3;

/// torajs-arr layout mirrors — `len` u64 at +8, inline props-dynobj
/// slot at +24 (`torajs_arr::layout::ARR_PROPS_OFF`).
const ARR_LEN_OFF: usize = 8;
const ARR_PROPS_OFF: usize = 24;
/// Closure env-cell props-dynobj slot (T-27 Function-as-Object,
/// mirror `torajs_anyvalue::member_get::CLOSURE_PROPS_OFF`).
const CLOSURE_PROPS_OFF: usize = 24;
/// Str payload length u32 at +8 (`torajs-str` layout).
const STR_LEN_OFF: usize = 8;

/// ShortStr NaN-box marker (`top16 == 0x0001`) + len bits 47..40
/// (mirror `torajs_anyvalue::nanbox` SSO layout).
const SHORT_STR_TOP16: u64 = 0x0001;

/// `torajs_dynobj::layout::BUCKET_FLAG_ENUMERABLE` mirror (bit 1).
const FLAG_ENUMERABLE: u64 = 1 << 1;

/// `AnySlotTag` heap tag (mirror torajs-anyvalue) — the tag
/// `unbox_tag` reports for any heap cell, including an AccessorPair.
const ANY_HEAP_TAG: i64 = 4;
/// `torajs_dynobj::accessor::TAG_ACCESSOR_PAIR` mirror.
const TAG_ACCESSOR_PAIR: u16 = 18;
/// Elem-kind chain stamping the entries outer array (`Arr<Arr<Any>>`:
/// heap elem = 4, inner FLAG_ARR_ANY blocks self-describe) so
/// kind-aware borrow readers (Object.fromEntries) decode the slots.
const KIND_CHAIN_HEAP: u64 = 4;

/// Runtime chooser — see module doc. Returns a +1-rc `Arr<Str>`.
///
/// # Safety
/// `obj` is null or a live heap ptr with a universal header;
/// `static_names` is an owned +1 `Arr<Str>` this call consumes-or-
/// returns.
/// `true` iff the heap cell's `type_tag` is DynObj.
#[inline]
unsafe fn is_dynobj(obj: *const c_void) -> bool {
    unsafe { *((obj as *const u8).add(HDR_TYPE_TAG_OFF) as *const u16) == TAG_DYNOBJ }
}

/// Append a live DynObj walk's keys onto `arr` in ES §10.1.11.1
/// order. `include_nonenum = 0` filters enumerable-only. Returns the
/// (possibly reallocated) array.
unsafe fn dynobj_keys_append(
    obj: *const c_void,
    include_nonenum: i64,
    mut arr: *mut u8,
) -> *mut u8 {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let mut order = vec![0u64; len as usize];
    let n = unsafe { __torajs_dynobj_iter_order(obj, order.as_mut_ptr(), len) };
    for &i in order.iter().take(n as usize) {
        if include_nonenum == 0 {
            let flags = unsafe { __torajs_dynobj_iter_flags(obj, i) };
            if flags & FLAG_ENUMERABLE == 0 {
                continue;
            }
        }
        let key = unsafe { __torajs_dynobj_iter_key(obj, i) };
        if key.is_null() {
            continue;
        }
        // Borrowed key → the array slot takes its own share.
        unsafe { __torajs_rc_inc(key) };
        arr = unsafe { __torajs_arr_push(arr, key as i64) };
    }
    arr
}

/// Build the key `Arr<Str>` from a live DynObj walk in ES
/// §10.1.11.1 order. `include_nonenum = 0` filters enumerable-only.
unsafe fn dynobj_keys_walk(obj: *const c_void, include_nonenum: i64) -> *mut c_void {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let arr = unsafe { __torajs_arr_alloc(len) };
    unsafe { dynobj_keys_append(obj, include_nonenum, arr) as *mut c_void }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_own_keys(
    obj: *const c_void,
    static_names: *mut c_void,
    include_nonenum: i64,
) -> *mut c_void {
    if obj.is_null() || !unsafe { is_dynobj(obj) } {
        return static_names;
    }
    unsafe { __torajs_value_drop_heap(static_names) };
    unsafe { dynobj_keys_walk(obj, include_nonenum) }
}

/// Index-key list `["0", ..., "<len-1>"]`, plus a trailing
/// `"length"` on the gOPN surface (`include_nonenum = 1`) — shared by
/// the Str and Arr ToObject arms.
unsafe fn index_keys(len: i64, include_nonenum: i64) -> *mut c_void {
    if include_nonenum == 0 {
        unsafe { crate::own_names::__torajs_arr_keys_only(len) }
    } else {
        unsafe { crate::own_names::__torajs_arr_index_strs(len) }
    }
}

/// Arr-cell own keys: index keys (+ `"length"` for gOPN, §10.4.2)
/// followed by expando keys from the inline props dynobj (insertion
/// order — `length` predates any expando write, matching the ES
/// OrdinaryOwnPropertyKeys creation-order tail).
unsafe fn arr_cell_keys(cell: *const c_void, include_nonenum: i64) -> *mut c_void {
    let len = unsafe { (cell.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() } as i64;
    let out = unsafe { index_keys(len, include_nonenum) };
    let props =
        unsafe { (cell.cast::<u8>().add(ARR_PROPS_OFF) as *const u64).read() } as *const c_void;
    if props.is_null() {
        return out;
    }
    unsafe { dynobj_keys_append(props, include_nonenum, out as *mut u8) as *mut c_void }
}

/// `Object.keys` / `getOwnPropertyNames` / `Reflect.ownKeys` arm for
/// an `any`-typed receiver — full ES §20.1.2.17 ToObject dispatch
/// (chunk B1, for-in RFC):
///
/// - DynObj cell → live-entry walk (enumerable filter per surface).
/// - Str cell / ShortStr imm → index keys (+ `"length"` for gOPN,
///   §22.1.5.2.4).
/// - Arr cell → index keys (+ `"length"` for gOPN) + expando keys.
/// - Closure cell → expando keys only (`length` / `name` own props
///   are not materialized — recorded divergence, both non-enumerable
///   per spec so `Object.keys` is exact).
/// - Obj (struct) cell → static-layout field walk (`struct_enum`).
/// - null / undefined → catchable TypeError (ToObject throws).
/// - every other receiver (Num imm / Bool / BigInt / Map / Set /
///   boxed primitives) → empty array: their ToObject wrappers carry
///   no own enumerable string keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_keys(v: u64, include_nonenum: i64) -> *mut c_void {
    // Cell-imm check mirrors `struct_enum::is_cell_imm` — the tag
    // read below is only sound for a real heap ptr bit pattern.
    if is_dynobj_imm(v) {
        return unsafe { dynobj_keys_walk(v as *const c_void, include_nonenum) };
    }
    if v == crate::reflect::VALUE_NULL_IMM || v == crate::reflect::VALUE_UNDEFINED_IMM {
        unsafe {
            __torajs_throw_type_error(c"cannot convert undefined or null to object".as_ptr());
        }
        return unsafe { __torajs_arr_alloc(0) as *mut c_void };
    }
    // ShortStr imm — len lives in bits 47..40 (SSO layout).
    if v >> 48 == SHORT_STR_TOP16 {
        let len = ((v >> 40) & 0xFF) as i64;
        return unsafe { index_keys(len, include_nonenum) };
    }
    if crate::reflect::is_cell_imm(v) {
        let cell = v as *const c_void;
        return match unsafe { heap_type_tag_local(cell) } {
            TAG_STR_CELL => {
                let len =
                    unsafe { (cell.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() } as i64;
                unsafe { index_keys(len, include_nonenum) }
            }
            TAG_ARR_CELL => unsafe { arr_cell_keys(cell, include_nonenum) },
            TAG_CLOSURE_CELL => {
                let props =
                    unsafe { (cell.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64).read() }
                        as *const c_void;
                let out = unsafe { __torajs_arr_alloc(0) };
                if props.is_null() {
                    out as *mut c_void
                } else {
                    unsafe { dynobj_keys_append(props, include_nonenum, out) as *mut c_void }
                }
            }
            TAG_OBJ_CELL => unsafe { crate::struct_enum::__torajs_anyv_struct_keys(v) },
            _ => unsafe { __torajs_arr_alloc(0) as *mut c_void },
        };
    }
    // Number imm / Bool sentinel / any other non-cell — ToObject
    // wrapper with no own enumerable string keys.
    unsafe { __torajs_arr_alloc(0) as *mut c_void }
}

/// Universal-header `type_tag` read (u16 at +4) — local twin of
/// `is_dynobj`'s read for the ToObject dispatch arms.
#[inline]
unsafe fn heap_type_tag_local(cell: *const c_void) -> u16 {
    unsafe { *((cell as *const u8).add(HDR_TYPE_TAG_OFF) as *const u16) }
}

/// Cell-imm + DynObj tag test on a raw NaN-box value (mirrors
/// `struct_enum::is_cell_imm` — the header read is only sound for a
/// real heap ptr bit pattern).
fn is_dynobj_imm(v: u64) -> bool {
    let top16_zero = v & 0xFFFF_0000_0000_0000 == 0;
    let not_sentinel = v & 0x2 == 0;
    top16_zero && not_sentinel && v != 0 && unsafe { is_dynobj(v as *const c_void) }
}

/// The `i`-th entry's value as an owned `(tag, value)` pair for an
/// array slot. A plain data value owned-unboxes (the bucket keeps its
/// share, the slot takes a fresh one). An accessor entry (heap box at
/// an `AccessorPair` cell) runs the getter — ES §7.3.24
/// EnumerableOwnProperties calls `[[Get]]` — and the OWNED answer's
/// share transfers to the slot via a borrow unbox.
unsafe fn entry_value_pair(obj: *const c_void, i: u64) -> (u64, u64) {
    let b = unsafe { __torajs_dynobj_iter_value(obj, i) };
    let t = unsafe { __torajs_anyv_unbox_tag(b) };
    if t == ANY_HEAP_TAG {
        let p = unsafe { __torajs_anyv_unbox_value(b) } as *const c_void;
        if !p.is_null()
            && unsafe { *((p as *const u8).add(HDR_TYPE_TAG_OFF) as *const u16) }
                == TAG_ACCESSOR_PAIR
        {
            let g = unsafe { __torajs_accessor_invoke_getter(p) };
            let gt = unsafe { __torajs_anyv_unbox_tag(g) };
            return (gt as u64, unsafe { __torajs_anyv_unbox_value(g) } as u64);
        }
    }
    (t as u64, unsafe { __torajs_anyv_unbox_value_owned(b) }
        as u64)
}

/// Build the value `Arr<Any>` from a live DynObj walk — ES order,
/// enumerable-only (`Object.values`, §20.1.2.22).
unsafe fn dynobj_values_walk(obj: *const c_void) -> *mut c_void {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let mut order = vec![0u64; len as usize];
    let n = unsafe { __torajs_dynobj_iter_order(obj, order.as_mut_ptr(), len) };
    let mut arr = unsafe { __torajs_arr_alloc_any(n) };
    for &i in order.iter().take(n as usize) {
        let flags = unsafe { __torajs_dynobj_iter_flags(obj, i) };
        if flags & FLAG_ENUMERABLE == 0 {
            continue;
        }
        if unsafe { __torajs_dynobj_iter_key(obj, i) }.is_null() {
            continue;
        }
        let (tag, val) = unsafe { entry_value_pair(obj, i) };
        arr = unsafe { __torajs_arr_push_any(arr as *mut c_void, tag, val) };
    }
    arr as *mut c_void
}

/// Build the `[key, value]` pair `Arr<Arr<Any>>` from a live DynObj
/// walk — ES order, enumerable-only (`Object.entries`, §20.1.2.5).
/// The outer array is elem-kind stamped so kind-aware borrow readers
/// (Object.fromEntries) decode its slots.
unsafe fn dynobj_entries_walk(obj: *const c_void) -> *mut c_void {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let mut order = vec![0u64; len as usize];
    let n = unsafe { __torajs_dynobj_iter_order(obj, order.as_mut_ptr(), len) };
    let mut outer = unsafe { __torajs_arr_alloc(n) };
    for &i in order.iter().take(n as usize) {
        let flags = unsafe { __torajs_dynobj_iter_flags(obj, i) };
        if flags & FLAG_ENUMERABLE == 0 {
            continue;
        }
        let key = unsafe { __torajs_dynobj_iter_key(obj, i) };
        if key.is_null() {
            continue;
        }
        // Borrowed key → the inner pair takes its own share.
        unsafe { __torajs_rc_inc(key) };
        let (tag, val) = unsafe { entry_value_pair(obj, i) };
        let inner = unsafe { __torajs_arr_alloc_any(2) };
        let inner =
            unsafe { __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP_TAG as u64, key as u64) };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, tag, val) };
        outer = unsafe { __torajs_arr_push(outer, inner as i64) };
    }
    unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
    outer as *mut c_void
}

/// `Object.values` arm for an `any`-typed receiver: a DynObj cell
/// walks its live entries; everything else delegates to the struct
/// arm (loud non-struct TypeError for non-struct cells).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_values(v: u64) -> *mut c_void {
    if is_dynobj_imm(v) {
        return unsafe { dynobj_values_walk(v as *const c_void) };
    }
    unsafe { crate::struct_enum::__torajs_anyv_struct_values(v) }
}

/// `Object.entries` arm for an `any`-typed receiver — same chooser
/// shape as [`__torajs_anyv_own_values`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_entries(v: u64) -> *mut c_void {
    if is_dynobj_imm(v) {
        return unsafe { dynobj_entries_walk(v as *const c_void) };
    }
    unsafe { crate::struct_enum::__torajs_anyv_struct_entries(v) }
}
