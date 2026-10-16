//! `get <Ctor>[@@species]` — the one symbol-keyed ACCESSOR a species-
//! aware builtin constructor owns (§23.1.2.5 Array, §22.2.5.2 RegExp,
//! §27.2.4.8 Promise, §24.1.2.3 Map, §24.2.2.3 Set, §25.1.5.3
//! ArrayBuffer, §23.2.2.4 %TypedArray%).
//!
//! Every one of them is the same function body: return the `this`
//! value. It is an accessor rather than a data property so that a
//! subclass, which inherits it through its constructor's
//! [[Prototype]] chain, answers ITSELF (`class My extends Array {}` —
//! `My[Symbol.species] === My`). The twelve concrete typed-array
//! constructors carry no entry of their own; they reach %TypedArray%'s
//! through `ctor_read_cell_inherited`'s chain.
//!
//! Installed into the interned constructor cell's own property dict
//! before the cell is published, so reads, `in`, `hasOwnProperty`,
//! `getOwnPropertyDescriptor`, `getOwnPropertySymbols` and `delete`
//! all answer it through the dict a program's own
//! `Object.defineProperty(Map, sym, { get })` lands in — no face of
//! its own to keep in step.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use super::iter_proto::{iter_proto_self_entry, mint_symbol_method_cell};
use super::{__torajs_dynobj_alloc, __torajs_dynobj_define_plain, ANY_HEAP};
use crate::method_value::symbol_static;

/// `symbol_static::WELL_KNOWN_NAMES` position of `species`.
const WK_SPECIES: i64 = 10;

/// `torajs_dynobj::accessor::ACC_KIND_BOXED` on the getter face; the
/// setter face is absent.
const ACC_KINDS_GETTER_ONLY: u64 = 5;

/// `{ get, set: undefined, enumerable: false, configurable: true }` —
/// the flag word `typedarray_proto_accessor` uses for the same set.
const ACCESSOR_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 7) | (1 << 8) | (1 << 4) | (1 << 5) | (1 << 2);

unsafe extern "C" {
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_accessor_pair_new(get: *mut c_void, set: *mut c_void, kinds: u64) -> *mut c_void;
}

/// The builtin constructors whose spec text defines `get [@@species]`.
pub(crate) fn owns_species(proto_tag: i64) -> bool {
    use torajs_rc::builtin_proto::{
        ARRAY_PROTO_TAG, MAP_PROTO_TAG, SET_PROTO_TAG, TYPEDARRAY_PROTO_TAG,
    };
    const REGEXP: i64 = 7;
    const PROMISE: i64 = 10;
    const ARRAY_BUFFER: i64 = 19;
    proto_tag == ARRAY_PROTO_TAG as i64
        || proto_tag == REGEXP
        || proto_tag == PROMISE
        || proto_tag == MAP_PROTO_TAG as i64
        || proto_tag == SET_PROTO_TAG as i64
        || proto_tag == ARRAY_BUFFER
        || proto_tag == TYPEDARRAY_PROTO_TAG as i64
}

/// Install the accessor into a freshly minted constructor cell's
/// props slot, allocating the dict.
///
/// # Safety
/// `props_slot` is the unpublished cell's null props slot.
pub(crate) unsafe fn install(proto_tag: i64, props_slot: *mut *mut c_void) {
    unsafe {
        *props_slot = __torajs_dynobj_alloc();
        let get_cell = mint_symbol_method_cell(iter_proto_self_entry, b"get [Symbol.species]", 0);
        let pair = __torajs_accessor_pair_new(
            get_cell.cast(),
            core::ptr::null_mut(),
            ACC_KINDS_GETTER_ONLY,
        );
        let key = symbol_static::well_known_singleton(WK_SPECIES) as *mut c_void;
        __torajs_dynobj_define_plain(props_slot, key, ANY_HEAP, pair as u64, ACCESSOR_ENTRY_FLAGS);
        INSTALLED_PAIR[proto_tag as usize].store(pair as u64, Ordering::Relaxed);
    }
}

/// The accessor pair [`install`] put under each constructor's
/// `@@species` — identity is what tells an untouched entry from a
/// program's redefinition or delete.
static INSTALLED_PAIR: [AtomicU64; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS] =
    [const { AtomicU64::new(0) }; torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS];

/// Whether `ctor_cell`'s `@@species` entry is no longer the installed
/// accessor (redefined, overwritten or deleted). Read-only: no getter
/// runs.
///
/// # Safety
/// `ctor_cell` is the live interned constructor cell for `proto_tag`.
pub unsafe fn species_replaced(proto_tag: i64, ctor_cell: *mut c_void) -> bool {
    unsafe {
        let props = crate::member_get_layout::closure_props(ctor_cell);
        if props.is_null() {
            return true;
        }
        let key = symbol_static::well_known_singleton(WK_SPECIES);
        __torajs_dynobj_get_value(props, key)
            != INSTALLED_PAIR[proto_tag as usize].load(Ordering::Relaxed)
    }
}
