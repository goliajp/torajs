//! Which layout rows a struct enumeration walks, and in what order.
//!
//! `struct_enum` owns the three walks (`Object.keys` / `values` /
//! `entries`); this file answers the two questions every walk asks
//! before it emits a row: the §10.1.11.1 order of the rows
//! (integer-index names first, ascending), and whether a row is the
//! second slot of a `get`/`set` pair that an earlier row already
//! emitted. torajs-anyvalue's JSON.stringify walk reads the same order
//! through `__torajs_struct_field_order_fill`.

use crate::struct_enum::StrSlice;
use crate::struct_reflect::slot_key;
use core::ffi::c_void;

unsafe extern "C" {
    // torajs-structmeta — read-side over __torajs_class_layouts.
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StrSlice;
}

/// §10.1.11.1 OrdinaryOwnPropertyKeys over the layout rows: every
/// integer-index name first in ascending numeric order, then the rest
/// in declaration order (rotation 652 — `class C { b = 1; 10 = 2 }`
/// enumerated `b,10`; bun and the spec say `10,b`). The three walks
/// below and the expando fold's integer merge read the same order, so
/// an instance with and without an expando bag agrees with itself.
pub(crate) unsafe fn layout_field_order(layout: *const c_void, n: u32) -> Vec<u32> {
    let mut ints: Vec<(u64, u32)> = Vec::new();
    let mut rest: Vec<u32> = Vec::with_capacity(n as usize);
    for i in 0..n {
        let name = unsafe { __torajs_struct_field_name(layout, i) };
        let (kp, klen) = unsafe { slot_key(name.ptr, name.len) };
        let bytes = unsafe { core::slice::from_raw_parts(kp, klen) };
        match crate::arr_reflect::canonical_index(bytes) {
            Some(v) => ints.push((v, i)),
            None => rest.push(i),
        }
    }
    ints.sort_by_key(|(v, _)| *v);
    ints.into_iter().map(|(_, i)| i).chain(rest).collect()
}

/// The same order handed across the ABI: `out` receives the `n` slot
/// indices in §10.1.11.1 order. torajs-anyvalue's JSON.stringify walk
/// serializes declared fields and cannot see this crate's Rust items.
///
/// # Safety
/// `layout` is a live layout entry with `n` rows; `out` points at `n`
/// writable `u32`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_field_order_fill(
    layout: *const c_void,
    n: u32,
    out: *mut u32,
) {
    for (k, i) in unsafe { layout_field_order(layout, n) }
        .into_iter()
        .enumerate()
    {
        unsafe { out.add(k).write(i) };
    }
}

/// Whether an earlier slot already enumerated `key` — a `get`/`set`
/// pair occupies two layout slots but is one own property, so the
/// second one is skipped.
pub(crate) unsafe fn key_already_emitted(
    layout: *const c_void,
    upto: u32,
    key: (*const u8, usize),
) -> bool {
    let mut j = 0;
    while j < upto {
        let prev = unsafe { __torajs_struct_field_name(layout, j) };
        let (p, plen) = unsafe { slot_key(prev.ptr, prev.len) };
        if plen == key.1
            && (key.1 == 0
                || unsafe {
                    core::slice::from_raw_parts(p, plen)
                        == core::slice::from_raw_parts(key.0, key.1)
                })
        {
            return true;
        }
        j += 1;
    }
    false
}
