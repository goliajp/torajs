//! The typed-array prototypes' own entries tr installs at mint time
//! (RFC 20260916-typedarray-intrinsic): §23.2.3.38
//! `get %TypedArray%.prototype[@@toStringTag]` — the one symbol-keyed
//! ACCESSOR on %TypedArray.prototype% — and §23.2.7.1
//! `<Kind>Array.prototype.BYTES_PER_ELEMENT`.
//!
//! It is an accessor rather than the data property the collections
//! carry because the answer depends on the receiver: a typed array
//! answers its [[TypedArrayName]], anything else answers undefined
//! (never a TypeError — `Object.prototype.toString` asks it of every
//! object). Installed as a real AccessorPair own entry, the
//! %Iterator.prototype% shape (`iter_proto_accessors`): the symbol
//! walk finds it one link above a per-kind prototype and invokes the
//! getter with the original receiver, so `ta[Symbol.toStringTag]` and
//! the `[object Int8Array]` badge are the same read.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use super::iter_proto::mint_symbol_method_cell;
use super::{__torajs_dynobj_define_plain, ANY_HEAP};
use crate::method_value::{mint_immortal_str, symbol_static};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};

/// `symbol_static::WELL_KNOWN_NAMES` position of `toStringTag`.
const WK_TO_STRING_TAG: i64 = 13;

/// `torajs_dynobj::accessor::ACC_KIND_BOXED` on the getter face; the
/// setter face is absent.
const ACC_KINDS_GETTER_ONLY: u64 = 5;

/// §23.2.3.38 gives `{ enumerable: false, configurable: true }` with
/// both faces present (the setter undefined) — the flag word
/// `iter_proto_accessors` uses for the same attribute set.
const ACCESSOR_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 7) | (1 << 8) | (1 << 4) | (1 << 5) | (1 << 2);

unsafe extern "C" {
    fn __torajs_accessor_pair_new(get: *mut c_void, set: *mut c_void, kinds: u64) -> *mut c_void;
    fn __torajs_typedarray_kind(av: u64) -> i64;
}

/// The twelve `[[TypedArrayName]]`s, in the
/// `torajs_buffer::typedarray::Kind` discriminant order (wire format;
/// the substrate's `name()` is the same table on its side).
fn typedarray_name(kind: i64) -> Option<&'static [u8]> {
    Some(match kind {
        0 => b"Int8Array",
        1 => b"Uint8Array",
        2 => b"Uint8ClampedArray",
        3 => b"Int16Array",
        4 => b"Uint16Array",
        5 => b"Int32Array",
        6 => b"Uint32Array",
        7 => b"Float32Array",
        8 => b"Float64Array",
        9 => b"BigInt64Array",
        10 => b"BigUint64Array",
        11 => b"Float16Array",
        _ => return None,
    })
}

/// One immortal name cell per kind, minted on first read — the getter
/// runs on every badge and every `ta[Symbol.toStringTag]`, so it must
/// not allocate per call.
static NAME_CELLS: [AtomicU64; 12] = [const { AtomicU64::new(0) }; 12];

/// §23.2.3.38 steps 1-4.
unsafe extern "C" fn tag_get(_env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    let recv: AnyValue = if argc > 0 && !argv.is_null() {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    // Steps 2-3 — not an object, or no [[TypedArrayName]].
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live heap pointer with a header.
    let tag = unsafe { ptr.cast::<u8>().add(4).cast::<u16>().read() };
    if tag != torajs_rc::Tag::TypedArray as u16 {
        return VALUE_UNDEFINED;
    }
    let kind = unsafe { __torajs_typedarray_kind(recv) };
    let Some(name) = typedarray_name(kind) else {
        return VALUE_UNDEFINED;
    };
    let slot = &NAME_CELLS[kind as usize];
    let mut cell = slot.load(Ordering::Relaxed) as *mut u8;
    if cell.is_null() {
        cell = mint_immortal_str(name);
        slot.store(cell as u64, Ordering::Relaxed);
    }
    unsafe { crate::nanbox_encode::__torajs_anyv_box_pointer(cell.cast()) }
}

/// §23.2.7.1 — install `BYTES_PER_ELEMENT` as a
/// `{ writable: false, enumerable: false, configurable: false }` data
/// entry on a freshly minted per-kind prototype (slots 20-31); a no-op
/// for every other slot. A real dict entry, so the own-property faces
/// (hasOwnProperty, gOPD, the own-key listing, delete / write refusal)
/// read it like any other data property.
///
/// # Safety
/// `proto` is the freshly allocated dynobj for builtin-proto slot `idx`.
pub(crate) unsafe fn install_bytes_per_element(proto: *mut c_void, idx: i64) {
    let Some(n) = torajs_rc::builtin_proto::typedarray_bytes_per_element(idx) else {
        return;
    };
    // Value present, W / E / C all present and false.
    const CONSTANT_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3);
    unsafe {
        let key = super::interned_key(&BPE_KEY_CELL, b"BYTES_PER_ELEMENT");
        let mut slot = proto;
        __torajs_dynobj_define_plain(&mut slot, key.cast(), 2, n as u64, CONSTANT_ENTRY_FLAGS);
    }
}

static BPE_KEY_CELL: AtomicU64 = AtomicU64::new(0);

/// Install the accessor own entry into the freshly minted slot-39
/// singleton, after its `[Symbol.iterator]` (§23.2.3.37 precedes
/// .38, which is the order `getOwnPropertySymbols` answers in) and
/// before its address is published.
///
/// # Safety
/// `proto` is the freshly allocated dynobj.
pub(crate) unsafe fn install(proto: *mut c_void) {
    unsafe {
        let get_cell = mint_symbol_method_cell(tag_get, b"get [Symbol.toStringTag]", 0);
        let pair = __torajs_accessor_pair_new(
            get_cell.cast(),
            core::ptr::null_mut(),
            ACC_KINDS_GETTER_ONLY,
        );
        let key = symbol_static::well_known_singleton(WK_TO_STRING_TAG) as *mut c_void;
        let mut slot = proto;
        __torajs_dynobj_define_plain(&mut slot, key, ANY_HEAP, pair as u64, ACCESSOR_ENTRY_FLAGS);
    }
}
