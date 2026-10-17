//! Struct-cell own-keys merge — RFC 20260714-struct-dynamic-props
//! blade 3 (enumeration face over blade 2's +24 expando dict).
//!
//! The typed keys / for-in / gOPN emits mint the LAYOUT field-name
//! list at compile time and hand it to `__torajs_obj_own_keys`; a
//! struct cell that grew expandos must fold those in per ES
//! §10.1.11.1 OrdinaryOwnPropertyKeys:
//!
//! 1. every integer-like key — layout field names spelled as
//!    canonical indices (`{0:"a"}` anon structs) AND numeric
//!    expandos — in ascending numeric order,
//! 2. the remaining layout names in declaration order (all predate
//!    every expando: fields exist from construction),
//! 3. the remaining expando keys in insertion order.
//!
//! A NULL props slot short-circuits to the static list untouched, so
//! the no-expando path stays allocation-free.
//!
//! 628-05 — a runtime-computed instance field is an expando with a
//! DECLARATION position among the layout rows
//! (`classmeta::computed_fields`). Every walk here takes an
//! [`ExpandoLane`]: the entries that go before row `i`, the ones
//! after the last row, and the rest — so `class C { foo = 1; [x] =
//! 42; bar = 2 }` lists `foo,b,bar`, not `foo,bar,b`.

use core::ffi::c_void;

use crate::obj_own_keys::FLAG_ENUMERABLE;
use crate::obj_own_keys_key_shape::{key_canonical_index, key_is_hidden_own};

use crate::classmeta::computed_fields::{BEFORE_EVERY_ROW, ComputedField, computed_fields, key_eq};

mod push;
pub(crate) use push::{push_expando_entries, push_expando_values};

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    // fnprops' spelling — the key param is the live Str CELL pointer.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const u8) -> u64;
}

/// `torajs-anyvalue` AnySlotTag::Heap mirror — the value tag of a heap
/// payload in an expando pair (`struct_enum` carries the same twin).
const ANY_HEAP: u64 = 4;

/// `torajs-core ssa_lower::OBJ_PROPS_OFF` mirror — the struct cell's
/// lazily-allocated expando dict (blade 1).
pub(crate) const OBJ_PROPS_OFF: usize = 24;

/// The struct cell's props dict, NULL when no expando was written.
pub(crate) unsafe fn struct_props(cell: *const c_void) -> *const c_void {
    unsafe { *(cell.cast::<u8>().add(OBJ_PROPS_OFF) as *const u64) as *const c_void }
}

/// Which expando entries a walk wants at one point of its layout-row
/// loop (628-05).
#[derive(Clone, Copy)]
pub(crate) enum ExpandoLane {
    /// The computed fields declared right before row `i` (before
    /// every row for `i == 0`). Called at the top of each row.
    Before(u32),
    /// The computed fields declared after the last row. Called once
    /// past the loop.
    Tail,
    /// Every other expando, in insertion order.
    Rest,
}

/// The expando dict iteration indices `lane` selects, in dict order —
/// a class with no computed fields answers nothing for `Before` /
/// `Tail` and the whole dict for `Rest`, so the common path is one
/// empty-slice test.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell whose props dict is non-NULL.
unsafe fn lane_order(cell: *const c_void, props: *const c_void, lane: ExpandoLane) -> Vec<u64> {
    let n_props = unsafe { __torajs_dynobj_iter_len(props) };
    let mut order = vec![0u64; n_props as usize];
    let n = unsafe { __torajs_dynobj_iter_order(props, order.as_mut_ptr(), n_props) };
    order.truncate(n as usize);
    // A dict entry under a name the layout declares is the field's
    // sidecar (attributes / shadow / deletion record — RFC 20260806 /
    // 20260915), never a second own property: the layout row is the
    // property. Free unless the instance carries a sidecar at all.
    order.retain(|&i| !unsafe { crate::struct_field_attrs::expando_key_declared(cell, props, i) });
    let fields = unsafe { computed_fields(cell) };
    if fields.is_empty() && !matches!(lane, ExpandoLane::Before(0) | ExpandoLane::Rest) {
        return Vec::new();
    }
    let key_at = |i: u64| unsafe { __torajs_dynobj_iter_key(props, i) };
    // §10.1.11.1 step 1 — an integer-like key comes before every
    // string key, ascending, wherever it was declared or inserted;
    // the keys walk merges them the same way, and once a computed
    // field can be one (`[5] = 6`), the row-interleaved walks have
    // to as well.
    let int_of = |i: u64| {
        let k = key_at(i);
        if k.is_null() {
            None
        } else {
            unsafe { key_canonical_index(k) }
        }
    };
    let anchor = match lane {
        ExpandoLane::Before(0) => {
            let mut ints: Vec<(u64, u64)> = order
                .iter()
                .filter_map(|&i| int_of(i).map(|v| (v, i)))
                .collect();
            ints.sort_by_key(|(v, _)| *v);
            let mut out: Vec<u64> = ints.into_iter().map(|(_, i)| i).collect();
            out.extend(
                anchored(fields, BEFORE_EVERY_ROW, &order, key_at)
                    .into_iter()
                    .filter(|&i| int_of(i).is_none()),
            );
            return out;
        }
        ExpandoLane::Before(i) => i - 1,
        ExpandoLane::Tail => {
            let class_tag = unsafe { cell.cast::<u8>().add(8).cast::<u32>().read() };
            let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
            match unsafe { __torajs_struct_field_count(layout) } {
                0 => BEFORE_EVERY_ROW,
                n => n - 1,
            }
        }
        ExpandoLane::Rest => {
            order.retain(|&i| {
                let k = key_at(i);
                k.is_null()
                    || (int_of(i).is_none() && !fields.iter().any(|f| unsafe { key_eq(f.key, k) }))
            });
            return order;
        }
    };
    anchored(fields, anchor, &order, key_at)
        .into_iter()
        .filter(|&i| int_of(i).is_none())
        .collect()
}

/// The dict indices of the computed fields anchored at `anchor`, in
/// declaration order, that the dict currently holds.
fn anchored(
    fields: &[ComputedField],
    anchor: u32,
    order: &[u64],
    key_at: impl Fn(u64) -> *mut c_void,
) -> Vec<u64> {
    fields
        .iter()
        .filter(|f| f.after_row == anchor)
        .filter_map(|f| {
            order.iter().copied().find(|&i| {
                let k = key_at(i);
                !k.is_null() && unsafe { key_eq(f.key, k) }
            })
        })
        .collect()
}

/// Walk the struct cell's ENUMERABLE expando entries `lane` selects,
/// in insertion order, handing each `(key Str cell, value tag, value
/// payload)` to `f`. Every handed pointer/pair is a BORROW — the
/// callback takes its own shares. NULL / empty props is a no-op.
/// Shared by the values / entries walks (torajs-meta `struct_enum`).
pub(crate) unsafe fn for_each_enumerable_expando(
    cell: *const c_void,
    lane: ExpandoLane,
    mut f: impl FnMut(*mut c_void, u64, u64),
) {
    let props = unsafe { struct_props(cell) };
    if props.is_null() {
        return;
    }
    for i in unsafe { lane_order(cell, props, lane) } {
        if unsafe { __torajs_dynobj_iter_flags(props, i) } & FLAG_ENUMERABLE == 0 {
            continue;
        }
        let key = unsafe { __torajs_dynobj_iter_key(props, i) };
        if key.is_null() || unsafe { key_is_hidden_own(key) } {
            continue;
        }
        // RFC 20260806-declared-field-redefine — a declared name in
        // the dict is the field's ATTRIBUTE SIDECAR, not a second own
        // property, and its `value_anyv` is filler. The field itself
        // is enumerated from the layout.
        if unsafe { crate::struct_field_attrs::layout_declares(cell, key) } {
            continue;
        }
        let tag = unsafe { __torajs_dynobj_get_tag(props, key.cast::<u8>()) };
        let value = unsafe { __torajs_dynobj_get_value(props, key.cast::<u8>()) };
        f(key, tag, value);
    }
}

/// Fold a struct cell's expando keys into its static layout-name
/// list per the module-doc order. Consumes `static_names` (an owned
/// fresh `Arr<Str>`); answers an owned array either way.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell; `static_names` is an owned
/// heap-pointer-kind `Arr<Str>` this call may consume.
pub(crate) unsafe fn struct_keys_with_expandos(
    cell: *const c_void,
    static_names: *mut c_void,
    include_nonenum: i64,
) -> *mut c_void {
    let props = unsafe { struct_props(cell) };
    // the expando keys in ES dict order (BORROWED cells)
    let Some(order) = (unsafe { crate::struct_absent::expando_order(cell, props) }) else {
        return static_names;
    };
    let mut exp_ints: Vec<(u64, *mut c_void)> = Vec::new();
    let mut exp_tail: Vec<*mut c_void> = Vec::new();
    for &i in &order {
        if include_nonenum == 0 {
            let flags = unsafe { __torajs_dynobj_iter_flags(props, i) };
            if flags & FLAG_ENUMERABLE == 0 {
                continue;
            }
        }
        let key = unsafe { __torajs_dynobj_iter_key(props, i) };
        if key.is_null() || unsafe { key_is_hidden_own(key) } {
            continue;
        }
        // A declared name in the dict is the field's attribute
        // sidecar (RFC 20260806-declared-field-redefine) — the field
        // is already in `static_names`, and listing the sidecar
        // beside it would be the same key twice.
        if unsafe { crate::struct_field_attrs::layout_declares(cell, key) } {
            continue;
        }
        if let Some(idx) = unsafe { key_canonical_index(key) } {
            exp_ints.push((idx, key));
        } else {
            exp_tail.push(key);
        }
    }
    // Split the static list the same way (BORROWED cells into the
    // owned source array — the rebuilt array takes fresh shares and
    // the source drops whole at the end).
    let s_len = unsafe {
        static_names
            .cast::<u8>()
            .add(crate::obj_own_keys::ARR_LEN_OFF)
            .cast::<u64>()
            .read()
    };
    let s_data = unsafe {
        static_names
            .cast::<u8>()
            .add(crate::obj_own_keys::ARR_DATA_PTR_OFF)
            .cast::<*const u64>()
            .read()
    };
    let mut int_keys: Vec<(u64, *mut c_void)> = Vec::new();
    let mut static_tail: Vec<*mut c_void> = Vec::new();
    let mut dropped_static = false;
    for i in 0..s_len {
        let key = unsafe { s_data.add(i as usize).read() } as *mut c_void;
        if key.is_null() {
            continue;
        }
        // a deleted field (or one the literal that built the cell left
        // out, or an accessor a store replaced) is no layout name
        let k = unsafe { crate::str_wtf8::StrWtf8::of(key) };
        if unsafe { crate::struct_field_attrs::layout_member_gone(cell, k.as_ptr(), k.len()) } {
            dropped_static = true;
            continue;
        }
        // A field redefined to `enumerable: false` leaves Object.keys
        // and for-in, but `getOwnPropertyNames` still wants it — the
        // sidecar is the only place that distinction is recorded, the
        // layout row having no room for it.
        if include_nonenum == 0
            && unsafe { crate::struct_field_attrs::sidecar_attrs(cell, key) }
                .is_some_and(|attrs| attrs & FLAG_ENUMERABLE == 0)
        {
            dropped_static = true;
            continue;
        }
        if let Some(idx) = unsafe { key_canonical_index(key) } {
            int_keys.push((idx, key));
        } else {
            static_tail.push(key);
        }
    }
    // Nothing folded in and nothing filtered out — hand the static
    // list back untouched rather than rebuild it verbatim.
    if exp_ints.is_empty() && exp_tail.is_empty() && !dropped_static {
        return static_names;
    }
    // 628-05 — a computed field sits right behind the row it was
    // declared after; whatever finds no row (the row filtered out
    // above) stays where the insertion order put it.
    let fields = unsafe { computed_fields(cell) };
    if !fields.is_empty() {
        static_tail = unsafe { interleave_computed(cell, fields, static_tail, &mut exp_tail) };
    }
    int_keys.extend(exp_ints);
    int_keys.sort_by_key(|(v, _)| *v);
    let total = int_keys.len() + static_tail.len() + exp_tail.len();
    let mut out = unsafe { __torajs_arr_alloc(total as u64) };
    for key in int_keys
        .iter()
        .map(|(_, k)| *k)
        .chain(static_tail)
        .chain(exp_tail)
    {
        unsafe { __torajs_rc_inc(key) };
        out = unsafe { __torajs_arr_push(out, key as i64) };
    }
    unsafe { __torajs_value_drop_heap(static_names) };
    out as *mut c_void
}

/// The keys walk's row-anchored merge (628-05): `static_tail` (the
/// non-integer layout names, declaration order) with each computed
/// field pulled out of `exp_tail` and placed behind the name of its
/// anchor row — leading fields first. `exp_tail` keeps what was not
/// placed.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell; every pointer is a live Str cell.
unsafe fn interleave_computed(
    cell: *const c_void,
    fields: &[ComputedField],
    static_tail: Vec<*mut c_void>,
    exp_tail: &mut Vec<*mut c_void>,
) -> Vec<*mut c_void> {
    let class_tag = unsafe { cell.cast::<u8>().add(8).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    let mut take = |anchor: u32, out: &mut Vec<*mut c_void>| {
        for f in fields.iter().filter(|f| f.after_row == anchor) {
            if let Some(pos) = exp_tail.iter().position(|&k| unsafe { key_eq(f.key, k) }) {
                out.push(exp_tail.remove(pos));
            }
        }
    };
    let mut out: Vec<*mut c_void> = Vec::with_capacity(static_tail.len() + fields.len());
    take(BEFORE_EVERY_ROW, &mut out);
    for name in static_tail {
        out.push(name);
        let view = unsafe { crate::str_wtf8::StrWtf8::of(name) };
        let bytes = view.as_bytes();
        let row = unsafe { __torajs_struct_field_find(layout, bytes.as_ptr(), bytes.len() as u32) };
        if row != u32::MAX {
            take(row, &mut out);
        }
    }
    out
}

/// JSON.stringify's spelling of [`lane_order`] (torajs-anyvalue
/// `json_stringify::struct_expando`): `lane` is `-1` for the rest,
/// `-2` for the tail, a row index for the entries before that row.
/// Fills `out` with dict iteration indices and answers the count
/// (capped at `cap`); NULL props answers 0.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell; `out` has `cap` writable slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_expando_order(
    cell: *const c_void,
    lane: i64,
    out: *mut u64,
    cap: u64,
) -> u64 {
    let props = unsafe { struct_props(cell) };
    if props.is_null() {
        return 0;
    }
    let lane = match lane {
        -1 => ExpandoLane::Rest,
        -2 => ExpandoLane::Tail,
        i => ExpandoLane::Before(i as u32),
    };
    let order = unsafe { lane_order(cell, props, lane) };
    let n = order.len().min(cap as usize);
    for (i, &pi) in order.iter().take(n).enumerate() {
        unsafe { out.add(i).write(pi) };
    }
    n as u64
}
