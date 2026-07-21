//! Symbol well-known data statics reflection (RFC
//! 20260722-builtin-proto-reflection 刀 2) — §6.1.5.1: the 13
//! well-known symbols are string-named DATA own properties of the
//! `Symbol` constructor, `{ writable: false, enumerable: false,
//! configurable: false }`, value = the process-lifetime symbol
//! singleton (torajs-str's slot table, index-lockstep with the
//! alphabetical name list here). Not ns-static rows — that table
//! interns FUNCTION cells (its recorded boundary).

use core::ffi::c_void;

/// §6.1.5.1 property names, ALPHABETICAL — index-lockstep with
/// torajs-str's `WELL_KNOWN_SLOTS` / `WELL_KNOWN_DESCS`.
const WELL_KNOWN_NAMES: [&str; 15] = [
    "asyncDispose",
    "asyncIterator",
    "dispose",
    "hasInstance",
    "isConcatSpreadable",
    "iterator",
    "match",
    "matchAll",
    "replace",
    "search",
    "species",
    "split",
    "toPrimitive",
    "toStringTag",
    "unscopables",
];

unsafe extern "C" {
    /// torajs-str — the idx-th well-known singleton (owned +1;
    /// immortal, callers may treat the cell as ledger-free).
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
}

/// The builtin-proto tag the `Symbol` ctor keys (`torajs-rc
/// builtin_proto` order).
const SYMBOL_PROTO_TAG: i64 = 5;

/// True when `cell` is the `Symbol` ctor cell and `key` names a
/// well-known symbol static — the write / delete refusal gate (the
/// props are {writable: false, configurable: false}).
///
/// # Safety
/// `cell` is null or a live heap cell; `key` is null or a live Str
/// cell.
pub(crate) unsafe fn is_wellknown_on_symbol_ctor(cell: *const c_void, key: *const c_void) -> bool {
    unsafe { ctor_wellknown_symbol_idx(cell, key) }.is_some()
}

/// The well-known table index `key` names on the `Symbol` ctor cell
/// — `None` for every other cell / key.
///
/// # Safety
/// Same contract as [`is_wellknown_on_symbol_ctor`].
pub(crate) unsafe fn ctor_wellknown_symbol_idx(
    cell: *const c_void,
    key: *const c_void,
) -> Option<i64> {
    let tag = super::ctor::ctor_tag_of_cell(cell)?;
    if tag != SYMBOL_PROTO_TAG {
        return None;
    }
    let name = unsafe { super::ns_static_ctor::key_utf8(key) }?;
    WELL_KNOWN_NAMES
        .iter()
        .position(|n| *n == name)
        .map(|i| i as i64)
}

/// The well-known symbol singleton `key` names on the `Symbol` ctor
/// cell (owned +1; immortal) — `None` on every miss.
///
/// # Safety
/// Same contract as [`is_wellknown_on_symbol_ctor`].
pub(crate) unsafe fn ctor_wellknown_symbol(
    cell: *const c_void,
    key: *const c_void,
) -> Option<*mut c_void> {
    let idx = unsafe { ctor_wellknown_symbol_idx(cell, key) }?;
    let sym = unsafe { __torajs_symbol_well_known(idx) };
    if sym.is_null() { None } else { Some(sym) }
}

/// gOPD's well-known-symbol probe — NULL on every miss; torajs-meta
/// assembles the §6.1.5.1 `{ writable: false, enumerable: false,
/// configurable: false }` data descriptor around the answer.
///
/// # Safety
/// Same contract as [`is_wellknown_on_symbol_ctor`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_wellknown_symbol(
    cell: *const c_void,
    key: *const c_void,
) -> *mut c_void {
    unsafe { ctor_wellknown_symbol(cell, key) }.unwrap_or(core::ptr::null_mut())
}
