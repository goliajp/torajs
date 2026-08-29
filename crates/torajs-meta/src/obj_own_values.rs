//! `Object.values` / `Object.entries` runtime choosers for
//! `any`-typed receivers — the values/entries twin of
//! [`crate::obj_own_keys`]'s ToObject dispatch (chunk B1 shape).
//!
//! Every arm mirrors the keys chooser's receiver taxonomy
//! (ES §20.1.2.22 / §20.1.2.5 → §7.3.24 EnumerableOwnProperties):
//!
//! - DynObj cell → live-entry walk (enumerable-only, accessor
//!   entries run their getter).
//! - Str cell / ShortStr imm → per-code-unit fresh Strs
//!   (§22.1.5.2's indexed own properties). A ShortStr materializes
//!   through `__torajs_anyv_to_str` first, then drops the temp cell.
//! - Arr cell → kind-aware element walk via
//!   `__torajs_arr_get_any_boxed` (typed blocks behind an `any` view
//!   rebox per elem kind), then props-dynobj expando tail.
//! - Closure cell → expando entries only (name/length own props are
//!   non-enumerable per §20.2.4, so the enumerable surface is exact).
//! - Obj (struct) cell → static-layout field walk (`struct_enum`).
//! - null / undefined → catchable TypeError (ToObject throws).
//! - every other receiver (Num imm / Bool / BigInt / Map / Set) →
//!   empty array: their ToObject wrappers carry no own enumerable
//!   string keys.
//!
//! Shapes: values builds a true `Arr<Any>` (the SSA arm types the
//! result `Type::Arr(Any)`); entries builds an 8-byte-slot outer
//! array of `Arr<Any>` pairs, elem-kind stamped `KIND_CHAIN_HEAP` so
//! kind-aware borrow readers (Object.fromEntries) decode the slots —
//! both exactly the DynObj walk's shapes.

use core::ffi::c_void;

use crate::obj_own_keys::{
    ANY_HEAP_TAG, ARR_LEN_OFF, ARR_PROPS_OFF, CLOSURE_PROPS_OFF, FLAG_ENUMERABLE, HDR_TYPE_TAG_OFF,
    KIND_CHAIN_HEAP, SHORT_STR_TOP16, TAG_ACCESSOR_PAIR, TAG_ARR_CELL, TAG_BOOLEAN_WRAPPER,
    TAG_CLOSURE_CELL, TAG_NUMBER_WRAPPER, TAG_OBJ_CELL, TAG_PROMISE_CELL, TAG_STR_CELL,
    TAG_STRING_WRAPPER, WRAPPER_INNER_OFF, WRAPPER_PROPS_OFF, heap_type_tag_local, is_dynobj_imm,
};

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    /// torajs-anyvalue — §7.3.24 over a Proxy receiver.
    fn __torajs_proxy_own_values(v: u64, want_entries: i64) -> *mut u8;
    fn __torajs_rc_inc(p: *mut c_void);
    /// torajs-anyvalue ToString — a ShortStr materializes to a fresh
    /// owned heap Str; dropped via the universal dispatcher below.
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-arr kind-aware borrowed whole-box slot read (chunk 625
    /// contract: the slot keeps its share; OOB answers undefined).
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    /// torajs-str per-code-unit fresh Str + index-string mint.
    fn __torajs_str_at(s: *const u8, i: i64) -> *mut u8;
    fn __torajs_i64_to_str(n: i64) -> *mut u8;
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
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// Str payload length: u32 code units at +8 (`torajs-str` layout).
const STR_UNITS_OFF: usize = 8;

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
            let recv = unsafe { __torajs_anyv_box_from_pair(4, obj as i64) };
            let g = unsafe { __torajs_accessor_invoke_getter(p, recv) };
            let gt = unsafe { __torajs_anyv_unbox_tag(g) };
            return (gt as u64, unsafe { __torajs_anyv_unbox_value(g) } as u64);
        }
    }
    (t as u64, unsafe { __torajs_anyv_unbox_value_owned(b) }
        as u64)
}

/// Append a live DynObj walk's values onto the `Arr<Any>` `arr` in ES
/// §10.1.11.1 order, enumerable-only. Returns the (possibly
/// reallocated) array.
pub(crate) unsafe fn dynobj_values_append(obj: *const c_void, mut arr: *mut u8) -> *mut u8 {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let mut order = vec![0u64; len as usize];
    let n = unsafe { __torajs_dynobj_iter_order(obj, order.as_mut_ptr(), len) };
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
    arr
}

/// Append a live DynObj walk's `[key, value]` pairs onto the 8-byte-
/// slot `outer` in ES order, enumerable-only. Returns the (possibly
/// reallocated) array; the caller stamps the elem kind once.
pub(crate) unsafe fn dynobj_entries_append(obj: *const c_void, mut outer: *mut u8) -> *mut u8 {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let mut order = vec![0u64; len as usize];
    let n = unsafe { __torajs_dynobj_iter_order(obj, order.as_mut_ptr(), len) };
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
    outer
}

/// Build the value `Arr<Any>` from a live DynObj walk
/// (`Object.values`, §20.1.2.22).
unsafe fn dynobj_values_walk(obj: *const c_void) -> *mut c_void {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let arr = unsafe { __torajs_arr_alloc_any(len) };
    unsafe { dynobj_values_append(obj, arr) as *mut c_void }
}

/// Build the `[key, value]` pair `Arr<Arr<Any>>` from a live DynObj
/// walk (`Object.entries`, §20.1.2.5), elem-kind stamped.
unsafe fn dynobj_entries_walk(obj: *const c_void) -> *mut c_void {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let outer = unsafe { __torajs_arr_alloc(len) };
    let outer = unsafe { dynobj_entries_append(obj, outer) };
    unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
    outer as *mut c_void
}

/// Str-cell values: one fresh per-code-unit Str each (§22.1.5.2's
/// indexed own properties), as a true `Arr<Any>`.
unsafe fn str_cell_values(cell: *const c_void) -> *mut c_void {
    let len = unsafe { (cell.cast::<u8>().add(STR_UNITS_OFF) as *const u32).read() } as i64;
    let mut arr = unsafe { __torajs_arr_alloc_any(len.max(0) as u64) };
    for i in 0..len {
        // Fresh owned Str → the Any slot takes the share as-is.
        let ch = unsafe { __torajs_str_at(cell.cast::<u8>(), i) };
        arr = unsafe { __torajs_arr_push_any(arr as *mut c_void, ANY_HEAP_TAG as u64, ch as u64) };
    }
    arr as *mut c_void
}

/// Str-cell entries: `[["0", ch], ...]` pairs, elem-kind stamped.
unsafe fn str_cell_entries(cell: *const c_void) -> *mut c_void {
    let len = unsafe { (cell.cast::<u8>().add(STR_UNITS_OFF) as *const u32).read() } as i64;
    let mut outer = unsafe { __torajs_arr_alloc(len.max(0) as u64) };
    for i in 0..len {
        let idx_str = unsafe { __torajs_i64_to_str(i) };
        let ch = unsafe { __torajs_str_at(cell.cast::<u8>(), i) };
        let inner = unsafe { __torajs_arr_alloc_any(2) };
        let inner = unsafe {
            __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP_TAG as u64, idx_str as u64)
        };
        let inner =
            unsafe { __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP_TAG as u64, ch as u64) };
        outer = unsafe { __torajs_arr_push(outer, inner as i64) };
    }
    unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
    outer as *mut c_void
}

/// Arr-cell values: kind-aware element walk (borrowed box read →
/// owned unbox per slot), then the props-dynobj expando tail.
unsafe fn arr_cell_values(cell: *const c_void) -> *mut c_void {
    let len = unsafe { (cell.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let mut arr = unsafe { __torajs_arr_alloc_any(len) };
    for i in 0..len {
        let b = unsafe { __torajs_arr_get_any_boxed(cell, i) };
        let t = unsafe { __torajs_anyv_unbox_tag(b) };
        let val = unsafe { __torajs_anyv_unbox_value_owned(b) };
        arr = unsafe { __torajs_arr_push_any(arr as *mut c_void, t as u64, val as u64) };
    }
    let props =
        unsafe { (cell.cast::<u8>().add(ARR_PROPS_OFF) as *const u64).read() } as *const c_void;
    if props.is_null() {
        return arr as *mut c_void;
    }
    unsafe { dynobj_values_append(props, arr) as *mut c_void }
}

/// Arr-cell entries: `[[idx_str, elem], ...]` pairs plus the expando
/// tail, elem-kind stamped.
unsafe fn arr_cell_entries(cell: *const c_void) -> *mut c_void {
    let len = unsafe { (cell.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let mut outer = unsafe { __torajs_arr_alloc(len) };
    for i in 0..len {
        let idx_str = unsafe { __torajs_i64_to_str(i as i64) };
        let b = unsafe { __torajs_arr_get_any_boxed(cell, i) };
        let t = unsafe { __torajs_anyv_unbox_tag(b) };
        let val = unsafe { __torajs_anyv_unbox_value_owned(b) };
        let inner = unsafe { __torajs_arr_alloc_any(2) };
        let inner = unsafe {
            __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP_TAG as u64, idx_str as u64)
        };
        let inner = unsafe { __torajs_arr_push_any(inner as *mut c_void, t as u64, val as u64) };
        outer = unsafe { __torajs_arr_push(outer, inner as i64) };
    }
    let props =
        unsafe { (cell.cast::<u8>().add(ARR_PROPS_OFF) as *const u64).read() } as *const c_void;
    if !props.is_null() {
        outer = unsafe { dynobj_entries_append(props, outer) };
    }
    unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
    outer as *mut c_void
}

/// The closure env-cell's props dynobj, if any.
unsafe fn closure_props(cell: *const c_void) -> *const c_void {
    unsafe { (cell.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64).read() as *const c_void }
}

/// A primitive-wrapper cell's lazy expando dynobj, if any.
unsafe fn wrapper_props(cell: *const c_void) -> *const c_void {
    unsafe { (cell.cast::<u8>().add(WRAPPER_PROPS_OFF) as *const u64).read() as *const c_void }
}

/// A StringWrapper's inner `[[StringData]]` Str cell — NULL for the
/// `new String()` empty-string sentinel.
unsafe fn wrapper_str_inner(cell: *const c_void) -> *const c_void {
    unsafe { (cell.cast::<u8>().add(WRAPPER_INNER_OFF) as *const u64).read() as *const c_void }
}

/// ToObject throw shared by both choosers (§20.1.2.17 step 1); the
/// pending-throw model still needs a valid array for the value flow.
unsafe fn throw_to_object(any_shape: bool) -> *mut c_void {
    unsafe {
        __torajs_throw_type_error(c"cannot convert undefined or null to object".as_ptr());
        if any_shape {
            __torajs_arr_alloc_any(0) as *mut c_void
        } else {
            __torajs_arr_alloc(0) as *mut c_void
        }
    }
}

/// `Object.values` arm for an `any`-typed receiver — see module doc.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern; the caller owns the
/// returned `+1`-rc array and runs a `throw_check`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_values(v: u64) -> *mut c_void {
    // §7.3.24 — a Proxy composes its ownKeys /
    // getOwnPropertyDescriptor / get traps for this surface
    // (RFC 20260823-proxy-substrate 刀 4).
    if crate::reflect::is_cell_imm(v)
        && unsafe { heap_type_tag_local(v as *const c_void) } == crate::reflect::TAG_PROXY
    {
        return unsafe { __torajs_proxy_own_values(v, 0) } as *mut c_void;
    }
    if is_dynobj_imm(v) {
        return unsafe { dynobj_values_walk(v as *const c_void) };
    }
    if v == crate::reflect::VALUE_NULL_IMM || v == crate::reflect::VALUE_UNDEFINED_IMM {
        return unsafe { throw_to_object(true) };
    }
    if v >> 48 == SHORT_STR_TOP16 {
        let cell = unsafe { __torajs_anyv_to_str(v) };
        let out = unsafe { str_cell_values(cell) };
        unsafe { __torajs_value_drop_heap(cell as *mut c_void) };
        return out;
    }
    if crate::reflect::is_cell_imm(v) {
        let cell = v as *const c_void;
        let htag = unsafe { heap_type_tag_local(cell) };
        return match htag {
            TAG_STR_CELL => unsafe { str_cell_values(cell) },
            TAG_ARR_CELL => unsafe { arr_cell_values(cell) },
            TAG_CLOSURE_CELL => {
                let props = unsafe { closure_props(cell) };
                let arr = unsafe { __torajs_arr_alloc_any(0) };
                if props.is_null() {
                    arr as *mut c_void
                } else {
                    unsafe { dynobj_values_append(props, arr) as *mut c_void }
                }
            }
            TAG_OBJ_CELL => unsafe { crate::struct_enum::__torajs_anyv_struct_values(v) },
            // Buffer family — keys-face value twins (bodies in
            // `obj_own_values_buffer.rs`).
            crate::obj_own_keys_layout::TAG_TYPEDARRAY_CELL => unsafe {
                crate::obj_own_values_buffer::typedarray_cell_values(v, cell)
            },
            crate::obj_own_keys_layout::TAG_ARRAYBUFFER_CELL => unsafe {
                crate::obj_own_values_buffer::arraybuffer_cell_values(cell)
            },
            // DataView shares the ArrayBuffer twin (same +32 bag).
            crate::obj_own_keys_layout::TAG_DATAVIEW_CELL => unsafe {
                crate::obj_own_values_buffer::arraybuffer_cell_values(cell)
            },
            // Bag-only receivers — twin of
            // `obj_own_keys_bag::bag_cell_keys`, whose doc says which
            // shapes and why. `lastIndex` is non-enumerable, so the
            // bag is the whole surface here even for RegExp.
            TAG_PROMISE_CELL
            | crate::obj_own_keys_layout::TAG_MAP_CELL
            | crate::obj_own_keys_layout::TAG_SET_CELL
            | crate::obj_own_keys_layout::TAG_DATE_CELL
            | crate::obj_own_keys_layout::TAG_REGEXP_CELL
            | crate::obj_own_keys_layout::TAG_MAP_ITER_CELL
            | crate::obj_own_keys_layout::TAG_ARR_ITER_CELL
            | crate::obj_own_keys_layout::TAG_ITER_HELPER_CELL => {
                let props = unsafe { crate::obj_own_keys_layout::expando_props(cell, htag) };
                let arr = unsafe { __torajs_arr_alloc_any(0) };
                if props.is_null() {
                    arr as *mut c_void
                } else {
                    unsafe { dynobj_values_append(props, arr) as *mut c_void }
                }
            }
            // §10.4.3.3 — StringWrapper's [[StringData]] per-index
            // chars first, then the expando values (keys-face twin).
            TAG_STRING_WRAPPER => {
                let inner = unsafe { wrapper_str_inner(cell) };
                let arr = if inner.is_null() {
                    unsafe { __torajs_arr_alloc_any(0) as *mut c_void }
                } else {
                    unsafe { str_cell_values(inner) }
                };
                let props = unsafe { wrapper_props(cell) };
                if props.is_null() {
                    arr
                } else {
                    unsafe { dynobj_values_append(props, arr as *mut u8) as *mut c_void }
                }
            }
            // Number / Boolean wrappers carry no inherent own keys —
            // the expando walk is the whole surface (keys-face twin).
            TAG_NUMBER_WRAPPER | TAG_BOOLEAN_WRAPPER => {
                let props = unsafe { wrapper_props(cell) };
                let arr = unsafe { __torajs_arr_alloc_any(0) };
                if props.is_null() {
                    arr as *mut c_void
                } else {
                    unsafe { dynobj_values_append(props, arr) as *mut c_void }
                }
            }
            _ => unsafe { __torajs_arr_alloc_any(0) as *mut c_void },
        };
    }
    // Number imm / Bool sentinel / any other non-cell — ToObject
    // wrapper with no own enumerable string keys.
    unsafe { __torajs_arr_alloc_any(0) as *mut c_void }
}

/// `Object.entries` arm for an `any`-typed receiver — same chooser
/// shape as [`__torajs_anyv_own_values`].
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern; the caller owns the
/// returned `+1`-rc array and runs a `throw_check`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_entries(v: u64) -> *mut c_void {
    // §7.3.24 — a Proxy composes its ownKeys /
    // getOwnPropertyDescriptor / get traps for this surface
    // (RFC 20260823-proxy-substrate 刀 4).
    if crate::reflect::is_cell_imm(v)
        && unsafe { heap_type_tag_local(v as *const c_void) } == crate::reflect::TAG_PROXY
    {
        return unsafe { __torajs_proxy_own_values(v, 1) } as *mut c_void;
    }
    if is_dynobj_imm(v) {
        return unsafe { dynobj_entries_walk(v as *const c_void) };
    }
    if v == crate::reflect::VALUE_NULL_IMM || v == crate::reflect::VALUE_UNDEFINED_IMM {
        return unsafe { throw_to_object(false) };
    }
    if v >> 48 == SHORT_STR_TOP16 {
        let cell = unsafe { __torajs_anyv_to_str(v) };
        let out = unsafe { str_cell_entries(cell) };
        unsafe { __torajs_value_drop_heap(cell as *mut c_void) };
        return out;
    }
    if crate::reflect::is_cell_imm(v) {
        let cell = v as *const c_void;
        let htag = unsafe { heap_type_tag_local(cell) };
        return match htag {
            TAG_STR_CELL => unsafe { str_cell_entries(cell) },
            TAG_ARR_CELL => unsafe { arr_cell_entries(cell) },
            TAG_CLOSURE_CELL => {
                let props = unsafe { closure_props(cell) };
                let outer = unsafe { __torajs_arr_alloc(0) };
                let outer = if props.is_null() {
                    outer
                } else {
                    unsafe { dynobj_entries_append(props, outer) }
                };
                unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
                outer as *mut c_void
            }
            TAG_OBJ_CELL => unsafe { crate::struct_enum::__torajs_anyv_struct_entries(v) },
            // Buffer family — keys-face pair twins.
            crate::obj_own_keys_layout::TAG_TYPEDARRAY_CELL => unsafe {
                crate::obj_own_values_buffer::typedarray_cell_entries(v, cell)
            },
            crate::obj_own_keys_layout::TAG_ARRAYBUFFER_CELL => unsafe {
                crate::obj_own_values_buffer::arraybuffer_cell_entries(cell)
            },
            // DataView shares the ArrayBuffer twin (same +32 bag).
            crate::obj_own_keys_layout::TAG_DATAVIEW_CELL => unsafe {
                crate::obj_own_values_buffer::arraybuffer_cell_entries(cell)
            },
            // Bag-only receivers — pairs twin of the values arm above.
            TAG_PROMISE_CELL
            | crate::obj_own_keys_layout::TAG_MAP_CELL
            | crate::obj_own_keys_layout::TAG_SET_CELL
            | crate::obj_own_keys_layout::TAG_DATE_CELL
            | crate::obj_own_keys_layout::TAG_REGEXP_CELL
            | crate::obj_own_keys_layout::TAG_MAP_ITER_CELL
            | crate::obj_own_keys_layout::TAG_ARR_ITER_CELL
            | crate::obj_own_keys_layout::TAG_ITER_HELPER_CELL => {
                let props = unsafe { crate::obj_own_keys_layout::expando_props(cell, htag) };
                let outer = unsafe { __torajs_arr_alloc(0) };
                let outer = if props.is_null() {
                    outer
                } else {
                    unsafe { dynobj_entries_append(props, outer) }
                };
                unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
                outer as *mut c_void
            }
            // §10.4.3.3 — StringWrapper index pairs first, then the
            // expando pairs (keys-face twin).
            TAG_STRING_WRAPPER => {
                let inner = unsafe { wrapper_str_inner(cell) };
                let outer = if inner.is_null() {
                    unsafe { __torajs_arr_alloc(0) }
                } else {
                    unsafe { str_cell_entries(inner) as *mut u8 }
                };
                let props = unsafe { wrapper_props(cell) };
                let outer = if props.is_null() {
                    outer
                } else {
                    unsafe { dynobj_entries_append(props, outer) }
                };
                unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
                outer as *mut c_void
            }
            TAG_NUMBER_WRAPPER | TAG_BOOLEAN_WRAPPER => {
                let props = unsafe { wrapper_props(cell) };
                let outer = unsafe { __torajs_arr_alloc(0) };
                let outer = if props.is_null() {
                    outer
                } else {
                    unsafe { dynobj_entries_append(props, outer) }
                };
                unsafe { __torajs_arr_mark_kind(outer as *mut c_void, KIND_CHAIN_HEAP) };
                outer as *mut c_void
            }
            _ => unsafe { __torajs_arr_alloc(0) as *mut c_void },
        };
    }
    unsafe { __torajs_arr_alloc(0) as *mut c_void }
}
