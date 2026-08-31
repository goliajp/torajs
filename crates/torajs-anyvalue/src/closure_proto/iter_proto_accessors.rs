//! §27.1.2's two ACCESSOR properties on %Iterator.prototype% —
//! `constructor` (§27.1.2.1) and `[Symbol.toStringTag]`
//! (§27.1.2.2).
//!
//! Every other builtin prototype's `constructor` is a data property,
//! and tr synthesizes it from the interned ctor table
//! (`method_support_proto`). This one prototype is different on
//! purpose: it sits under every iterator in the language, so a plain
//! writable data property would let `it.constructor = x` — an
//! ordinary-looking assignment on an ordinary-looking object —
//! rewrite the shared root. §27.1.2 answers that with a getter and a
//! setter, and the setter is
//! [SetterThatIgnoresPrototypeProperties](https://tc39.es/ecma262/#sec-SetterThatIgnoresPrototypeProperties):
//! writing THROUGH an instance defines the property on the instance,
//! writing directly ON the home object throws.
//!
//! Before this, `Iterator.prototype.constructor = 7` silently
//! replaced the root's constructor for the rest of the process, and
//! `[Symbol.toStringTag]` was not there at all — so
//! `Object.prototype.toString.call(Iterator.prototype)` had no badge
//! to read.
//!
//! The mechanics are all pre-existing: a real AccessorPair own entry
//! (the `Object.prototype.__proto__` shape from RFC
//! 20260718-accessor-reify), faces minted as receiver-first closure
//! cells (the `[Symbol.iterator]` shape next door), and the walk in
//! `method_support_proto::proto_level_value` that already invokes an
//! own accessor entry with the singleton as receiver. What is new is
//! only which entries the tag-15 mint installs.

use core::ffi::{c_char, c_void};
use core::sync::atomic::AtomicU64;

use super::iter_proto::mint_symbol_method_cell;
use super::{__torajs_dynobj_define_plain, ANY_HEAP, interned_key};
use crate::method_value::{builtin_ctor_cell, mint_immortal_str, symbol_static};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell};
use crate::nanbox_encode::{
    __torajs_anyv_box_pointer, __torajs_anyv_unbox_tag, __torajs_anyv_unbox_value,
};

/// `torajs_rc::builtin_proto::ITERATOR_PROTO_TAG`.
const ITERATOR_PROTO_TAG: i64 = 15;
/// `symbol_static::WELL_KNOWN_NAMES` position of `toStringTag`.
const WK_TO_STRING_TAG: i64 = 13;

/// `torajs_dynobj::accessor::ACC_KIND_BOXED` on both faces — each is
/// a receiver-first closure cell, and the invoke path ORs the
/// header flag anyway (`closure_recv_first`).
const ACC_KINDS_BOXED_BOTH: u64 = 5 | (5 << 8);

/// §27.1.2 gives both properties `{ enumerable: false,
/// configurable: true }` with both faces present.
const ACCESSOR_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 7) | (1 << 8) | (1 << 4) | (1 << 5) | (1 << 2);

unsafe extern "C" {
    /// torajs-rc — the builtin-prototype singleton for a tag. Used
    /// to recognize the home object the setter must refuse.
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    /// torajs-dynobj — fresh `+1`-rc AccessorPair (faces transfer;
    /// the minted cells are immortal, so their rc traffic no-ops).
    fn __torajs_accessor_pair_new(get: *mut c_void, set: *mut c_void, kinds: u64) -> *mut c_void;
    fn __torajs_throw_type_error(msg: *const c_char);
}

/// §27.1.2.1 `get Iterator.prototype.constructor` — %Iterator%, the
/// same interned cell the synthesized data property handed out, so
/// `Iterator.prototype.constructor === Iterator` still holds.
unsafe extern "C" fn ctor_get(_env: *mut c_void, _argv: *const u64, _argc: i64) -> u64 {
    let cell = builtin_ctor_cell(ITERATOR_PROTO_TAG);
    unsafe { __torajs_anyv_box_pointer(cell.cast()) }
}

/// §27.1.2.2 `get Iterator.prototype[Symbol.toStringTag]` — the
/// string `"Iterator"`, minted once and immortal.
unsafe extern "C" fn tag_get(_env: *mut c_void, _argv: *const u64, _argc: i64) -> u64 {
    let cell = mint_immortal_str(b"Iterator");
    unsafe { __torajs_anyv_box_pointer(cell.cast()) }
}

/// §27.1.2.1 `set Iterator.prototype.constructor`.
unsafe extern "C" fn ctor_set(_env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    let key = interned_key(&CTOR_KEY_CELL, b"constructor");
    unsafe { setter_ignoring_prototype(argv, argc, key.cast()) }
}

/// §27.1.2.2 `set Iterator.prototype[Symbol.toStringTag]`.
unsafe extern "C" fn tag_set(_env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    let key = symbol_static::well_known_singleton(WK_TO_STRING_TAG) as *mut c_void;
    unsafe { setter_ignoring_prototype(argv, argc, key) }
}

/// SetterThatIgnoresPrototypeProperties(thisValue, home, p, v).
///
/// Steps 4 and 5 — CreateDataPropertyOrThrow when the receiver has
/// no own `p`, an ordinary Set when it does — are the SAME operation
/// on an own property table: a fresh entry gets `{W,E,C}` all true,
/// an existing one keeps its attributes. So one own-face write
/// answers both, and going through the own face rather than through
/// [[Set]] is what keeps this setter from finding itself again on
/// the receiver's prototype chain.
///
/// # Safety
/// Called from the accessor-invoke path: `argv[0]` is the borrowed
/// receiver and `argv[1]` the borrowed value; `key` is a live Str or
/// Symbol cell.
unsafe fn setter_ignoring_prototype(argv: *const u64, argc: i64, key: *mut c_void) -> u64 {
    let recv: AnyValue = if argc > 0 && !argv.is_null() {
        unsafe { *argv }
    } else {
        crate::nanbox::VALUE_UNDEFINED
    };
    let value: AnyValue = if argc > 1 && !argv.is_null() {
        unsafe { *argv.add(1) }
    } else {
        crate::nanbox::VALUE_UNDEFINED
    };
    // Step 1 — a primitive `this` has no property table.
    if !is_cell(recv) {
        unsafe {
            __torajs_throw_type_error(
                c"Iterator.prototype setter requires an object receiver".as_ptr(),
            )
        };
        return crate::nanbox::VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    // Step 2 — writing on the home object itself. The note in the
    // spec says this emulates assignment to a non-writable data
    // property, which is exactly what the shared root needs.
    let home = unsafe { __torajs_get_builtin_prototype(ITERATOR_PROTO_TAG) };
    if ptr == home {
        unsafe {
            __torajs_throw_type_error(
                c"cannot assign to a property of %Iterator.prototype% itself".as_ptr(),
            )
        };
        return crate::nanbox::VALUE_UNDEFINED;
    }
    // Steps 3-5, as one own-face write. The value arrives borrowed;
    // the store takes a stake of its own — AnyValue-shaped inc
    // (546-02): the pair-shaped payload_rc_inc double-staked a
    // ShortStr's materialized Str (unbox_tag lies Heap); the no-op
    // inc + the write's single transfer balance the materialization.
    // SAFETY: `value` is a live AnyValue per the fn contract.
    unsafe { crate::nanbox_ffi::__torajs_anyv_rc_inc(value) };
    let vtag = __torajs_anyv_unbox_tag(value) as u64;
    let vval = __torajs_anyv_unbox_value(value) as u64;
    if !unsafe { crate::member_set_own_face::own_face_write(recv, ptr, key, vtag, vval) } {
        unsafe {
            __torajs_throw_type_error(c"this value cannot hold the assigned property".as_ptr())
        };
    }
    crate::nanbox::VALUE_UNDEFINED
}

static CTOR_KEY_CELL: AtomicU64 = AtomicU64::new(0);

/// Install both accessor own entries into the freshly minted tag-15
/// singleton, before its address is published — the same pre-CAS
/// posture as the symbol entries next door.
///
/// # Safety
/// `proto` is the freshly allocated dynobj.
pub(crate) unsafe fn install(proto: *mut c_void) {
    for (get_entry, set_entry, get_name, set_name, key) in [
        (
            ctor_get as unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64,
            ctor_set as unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64,
            b"get constructor" as &[u8],
            b"set constructor" as &[u8],
            interned_key(&CTOR_KEY_CELL, b"constructor").cast::<c_void>(),
        ),
        (
            tag_get,
            tag_set,
            b"get [Symbol.toStringTag]",
            b"set [Symbol.toStringTag]",
            symbol_static::well_known_singleton(WK_TO_STRING_TAG) as *mut c_void,
        ),
    ] {
        unsafe {
            let get_cell = mint_symbol_method_cell(get_entry, get_name);
            let set_cell = mint_symbol_method_cell(set_entry, set_name);
            let pair =
                __torajs_accessor_pair_new(get_cell.cast(), set_cell.cast(), ACC_KINDS_BOXED_BOTH);
            let mut slot = proto;
            __torajs_dynobj_define_plain(
                &mut slot,
                key.cast(),
                ANY_HEAP,
                pair as u64,
                ACCESSOR_ENTRY_FLAGS,
            );
        }
    }
}
