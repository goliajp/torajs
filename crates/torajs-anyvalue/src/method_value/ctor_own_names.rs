//! A builtin constructor's own STRING keys beyond `length` / `name` —
//! the enumeration face of what [`super::ns_static_ctor::ctor_own_read_cell`]
//! already answers one key at a time: `prototype`, the table statics
//! (`Array.from`, `Promise.all`, …), the data constants (§21.1.2's
//! Number values, §23.2.6.1 `BYTES_PER_ELEMENT`) and the Symbol
//! constructor's well-known data properties.
//!
//! `getOwnPropertyNames(Array)` answered `["length", "name"]` while
//! every one of those keys read, `in`-tested and gOPD'd as an own
//! property — the reflection list was the one face nobody asked.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use super::mint_immortal_str;

/// Every data-constant key [`super::ns_static_ctor::ctor_data_constant`]
/// can answer, in the order the list reports them. A constructor lists
/// the ones that table answers for it, so the values stay single-sourced.
const DATA_CONSTANT_NAMES: [&str; 9] = [
    "EPSILON",
    "MAX_SAFE_INTEGER",
    "MAX_VALUE",
    "MIN_SAFE_INTEGER",
    "MIN_VALUE",
    "NaN",
    "NEGATIVE_INFINITY",
    "POSITIVE_INFINITY",
    "BYTES_PER_ELEMENT",
];

/// Interned key cells for `prototype` + [`DATA_CONSTANT_NAMES`].
static EXTRA_KEY_CELLS: [AtomicU64; 1 + DATA_CONSTANT_NAMES.len()] =
    [const { AtomicU64::new(0) }; 1 + DATA_CONSTANT_NAMES.len()];

fn extra_key_cell(slot: usize, name: &str) -> u64 {
    let s = &EXTRA_KEY_CELLS[slot];
    let p = s.load(Ordering::Relaxed);
    if p != 0 {
        return p;
    }
    let cell = mint_immortal_str(name.as_bytes()) as u64;
    s.store(cell, Ordering::Relaxed);
    cell
}

/// Call `f` with each own string key cell of the builtin constructor
/// `tag`, after its `length` / `name` pair. The cells are immortal.
fn for_each_own_name(tag: i64, mut f: impl FnMut(u64)) {
    f(extra_key_cell(0, "prototype"));
    let ns = super::ns_static_ctor::ctor_ns_name(tag);
    for (id, row) in torajs_rc::ns_static::NS_STATIC_TABLE.iter().enumerate() {
        if !ns.is_empty() && row.ns == ns && !torajs_rc::ns_static_is_deleted(id as i64) {
            f(super::ns_static::ns_static_name_cell(id as i64));
        }
    }
    for (i, name) in DATA_CONSTANT_NAMES.iter().enumerate() {
        if super::ns_static_ctor::ctor_data_constant(tag, name).is_some() {
            f(extra_key_cell(1 + i, name));
        }
    }
    if tag == super::symbol_static::SYMBOL_PROTO_TAG {
        super::symbol_static::for_each_well_known_key(&mut f);
    }
}

/// The own-name list of an interned builtin constructor cell, for
/// torajs-meta's closure own-keys arm: writes up to `cap` borrowed
/// immortal key cells into `out` and answers the total count (call
/// once with `cap` 0 to size the buffer). 0 for every other cell.
///
/// # Safety
/// `cell` is any pointer value (only compared); `out` holds `cap`
/// writable slots when `cap` > 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_ctor_own_names(
    cell: *const c_void,
    out: *mut u64,
    cap: u64,
) -> u64 {
    let Some(tag) = super::ctor::ctor_tag_of_cell(cell) else {
        return 0;
    };
    let mut n = 0u64;
    for_each_own_name(tag, |key| {
        if n < cap && !out.is_null() {
            // SAFETY: n < cap and the caller's buffer holds cap slots.
            unsafe { out.add(n as usize).write(key) };
        }
        n += 1;
    });
    n
}
