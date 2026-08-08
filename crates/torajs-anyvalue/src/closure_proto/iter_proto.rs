//! `%Iterator.prototype%`'s own well-known-symbol entries —
//! §27.1.2.1 `[Symbol.iterator]` (return this) and §27.1.4.1
//! `[Symbol.dispose]` (GetMethod "return", call, answer undefined),
//! installed as REAL symbol-keyed dict entries when torajs-rc's
//! builtin-proto mint first materializes the tag-15 singleton (RFC
//! 20260809 B6, generator leg).
//!
//! Why real entries rather than the native-tag reify the iterator
//! CELLS ride (`method_value::symbol_lookup`): a generator instance
//! is a `__Gen_<name>` struct whose symbol-keyed reads walk its real
//! prototype chain (instance → class proto → shared gen_proto →
//! %Iterator.prototype%, `member_get_symbol`'s Obj arm), so the only
//! place the whole family can inherit these faces from is the
//! prototype object itself. The cell reify stays as the chain-root
//! shortcut for MapIter / ArrIter / IterHelper receivers, whose
//! shapes carry no dict walk.
//!
//! A child module of `closure_proto` for the same reason `gen_step`
//! is one — the mint plumbing (`__torajs_dynobj_define`,
//! `interned_key`, `ANY_HEAP`) stays reachable with zero visibility
//! changes.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use super::{__torajs_dynobj_alloc, __torajs_dynobj_define, ANY_HEAP, interned_key};
use crate::method_value::{mint_immortal_str, mint_reject_closure_cell, symbol_static};
use crate::nanbox::VALUE_UNDEFINED;

/// Alphabetical well-known indices (`symbol_static::WELL_KNOWN_NAMES`).
const WK_DISPOSE: i64 = 2;
const WK_ITERATOR: i64 = 5;

/// Entry attrs {W:1, E:0, C:1} — §27.1.2.1 / §27.1.4.1 both carry
/// the standard method-property attributes. Value present + all
/// three flags present + writable + configurable.
const METHOD_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2) | 1;

/// Interned `"return"` name cell for the GetMethod dispatch below —
/// resolution is by NAME (a generator's `return` is a class method,
/// a user object's is a dict entry; both answer the by-name lane).
static RETURN_NAME_CELL: AtomicU64 = AtomicU64::new(0);

fn return_name_cell() -> *mut u8 {
    let p = RETURN_NAME_CELL.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    let cell = mint_immortal_str(b"return");
    RETURN_NAME_CELL.store(cell as u64, Ordering::Relaxed);
    cell
}

/// §27.1.2.1 `%Iterator.prototype%[Symbol.iterator]` — return this.
/// Owned-return convention: a cell receiver goes back with its own
/// fresh stake.
unsafe extern "C" fn iter_proto_self_entry(_env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    let recv = if argc > 0 && !argv.is_null() {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    if crate::nanbox::is_cell(recv) {
        unsafe { torajs_rc::__torajs_rc_inc(crate::nanbox::as_void_ptr(recv)) };
    }
    recv
}

/// §27.1.4.1 `%Iterator.prototype%[Symbol.dispose]` — GetMethod
/// (this, "return"): absent answers undefined with no call; present
/// runs it and DROPS the iter-result the spec ignores (a generator's
/// `return()` unwinds its suspended frame — `finally` blocks run —
/// then the instance answers done). A throw from the close stays in
/// the pending-throw channel and propagates through the caller's
/// check; the entry still answers undefined so the slot is settled.
unsafe extern "C" fn iter_proto_dispose_entry(
    _env: *mut c_void,
    argv: *const u64,
    argc: i64,
) -> u64 {
    let recv = if argc > 0 && !argv.is_null() {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    // §7.3.10 GetMethod step 3 — absent (or present-but-undefined)
    // answers undefined with no call. The presence probe has to come
    // FIRST: the by-name dispatch below THROWS on a receiver with no
    // such method (its fallthrough is the o.m() TypeError, not a
    // float), and `Object.create(Iterator.prototype)[Symbol.dispose]()`
    // is required to be a quiet no-op.
    let tag =
        unsafe { crate::member_get::__torajs_any_member_get_tag(recv, return_name_cell().cast()) };
    if tag == 5 {
        return VALUE_UNDEFINED;
    }
    let r = unsafe {
        crate::method_call::any_method_call_inner(
            recv,
            torajs_rc::any_method::ANY_METHOD_ITER_RETURN,
            return_name_cell(),
            core::ptr::null_mut(),
            core::ptr::null(),
            0,
        )
    };
    if r != crate::method_call::ANY_METHOD_NO_SUCH {
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(r) };
    }
    VALUE_UNDEFINED
}

/// Mint one prototype-method cell: reject-closure body + receiver in
/// argv[0] + `name` / `length` own reflection entries (the
/// `gen_step_method_cell` posture; both spec functions have length 0
/// and bracketed names that never intern back to a string key).
unsafe fn mint_symbol_method_cell(
    entry: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64,
    name: &[u8],
) -> *mut u8 {
    let cell = mint_reject_closure_cell(entry);
    unsafe {
        // Bit 12 — dispatcher passes the call-site `this` in argv[0].
        *(cell.add(6) as *mut u16) |= torajs_rc::FLAG_CLOSURE_RECV_FIRST;
        let props_slot = cell.add(24) as *mut *mut c_void;
        *props_slot = __torajs_dynobj_alloc();
        let name_cell = mint_immortal_str(name);
        __torajs_dynobj_define(
            props_slot,
            interned_key(&NAME_KEY_CELL, b"name"),
            ANY_HEAP,
            name_cell as u64,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_dynobj_define(
            props_slot,
            interned_key(&LENGTH_KEY_CELL, b"length"),
            ANY_I64,
            0,
            REFLECT_ENTRY_FLAGS,
        );
    }
    cell
}

/// Builtin-proto mint face — install both symbol-keyed entries into
/// the fresh tag-15 singleton before its address is published (the
/// same pre-CAS posture as `__torajs_object_proto_install`; a race
/// loser leaks a fully-formed dynobj).
///
/// The define takes its own stake on key and value; the minted cells
/// are immortal statics, the symbol keys are the process-lifetime
/// singletons.
///
/// # Safety
/// FFI face; `proto` is the freshly allocated dynobj (or null, a
/// no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iterator_proto_install(proto: *mut c_void) {
    if proto.is_null() {
        return;
    }
    for (idx, entry, name) in [
        (
            WK_ITERATOR,
            iter_proto_self_entry as unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64,
            b"[Symbol.iterator]" as &[u8],
        ),
        (WK_DISPOSE, iter_proto_dispose_entry, b"[Symbol.dispose]"),
    ] {
        let cell = unsafe { mint_symbol_method_cell(entry, name) };
        let key = symbol_static::well_known_singleton(idx) as *mut c_void;
        let mut slot = proto;
        unsafe {
            __torajs_dynobj_define(&mut slot, key, ANY_HEAP, cell as u64, METHOD_ENTRY_FLAGS)
        };
    }
}

/// `ANY_I64` slot tag (torajs-dynobj `layout.rs` mirror).
const ANY_I64: u64 = 2;
/// Reflection entry: value present + all three present, W0/E0/C1.
const REFLECT_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2);

static NAME_KEY_CELL: AtomicU64 = AtomicU64::new(0);
static LENGTH_KEY_CELL: AtomicU64 = AtomicU64::new(0);
