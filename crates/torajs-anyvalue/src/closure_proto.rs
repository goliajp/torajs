//! User-function `.prototype` materialization (RFC
//! 20260721-builtin-method-reflection 刀 9) — a plain `function`
//! form's cell (compiler-stamped `FLAG_FN_PROTO`) owns a lazily
//! minted §10.2.5 MakeConstructor `.prototype` object: an ordinary
//! dynobj whose `constructor` entry points back at the closure
//! (writable, non-enumerable, configurable) — the back-reference is
//! an rc CYCLE by design; the cycle collector already walks the
//! closure `+24` props slot (torajs-cycle `collect.rs`), so the pair
//! collects like any user `fun.self = fun` loop. The `prototype`
//! entry itself is writable / non-enumerable / non-configurable, so
//! it never shows in `Object.keys(fun)` and materializing is
//! observation-order invisible.
//!
//! Identity: the object is minted ONCE into the closure's props
//! dynobj — both member-get channels probe props first, so every
//! later read answers the same cell and `fun.prototype ===
//! fun.prototype` holds. Arrows, async forms, builtin cells and
//! generator factories carry no `FLAG_FN_PROTO` and keep their
//! undefined / substrate-specific answers.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::FLAG_FN_PROTO;

use crate::member_get_layout::closure_props;
use crate::method_value::mint_immortal_str;

/// The `%GeneratorPrototype%` / `%AsyncGeneratorPrototype%` step
/// trio. A child module so it reaches this one's private externs,
/// `interned_key` and `ANY_HEAP` unchanged; its own `no_mangle`
/// faces keep their symbols across the move.
mod gen_step;

/// `%Iterator.prototype%`'s own `[Symbol.iterator]` / `[Symbol.dispose]`
/// entries (RFC 20260809 B6) — a child for the same reach into the
/// private mint plumbing.
mod iter_proto;

unsafe extern "C" {
    /// torajs-dynobj — fresh empty entry table.
    fn __torajs_dynobj_alloc() -> *mut c_void;
    /// torajs-dynobj — §10.1.6.3 define with explicit W/E/C flags
    /// (`flags_byte` low 3 = values, bits 3-5 = present, bit 6 =
    /// value present). Consumes one rc of a heap `value`.
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    /// torajs-dynobj — entry probes (tag 5 = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — peek the pending tag (does NOT clear `active`;
    /// the contract is take_tag before take).
    fn __torajs_throw_take_tag() -> i64;
    /// torajs-throw — read the pending value and clear `active`.
    fn __torajs_throw_take() -> i64;
    /// torajs-throw — re-arm a pending throw (the fallback path when
    /// the taken value cannot settle a promise).
    fn __torajs_throw_set(tag: i64, value: i64);
    /// torajs-meta — §20.1.2.12 [[GetPrototypeOf]] over any receiver
    /// shape; answers an OWNED AnyValue (0 when the chain ends).
    fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
    /// torajs-promise — a settled-rejected cell; the caller transfers
    /// ONE refcount on the heap `reason`.
    fn __torajs_promise_alloc_rejected_heap(reason: i64) -> *mut c_void;
    /// torajs-promise — stamp the cell's `value_repr` so the any-lane
    /// `.then` / `.catch` bridge knows the payload's storage form.
    fn __torajs_promise_stamp_repr(p: *mut c_void, repr: i64);
}

/// `ANY_HEAP` slot tag (torajs-dynobj `layout.rs` mirror).
const ANY_HEAP: u64 = 4;
/// `prototype` entry: value present + all three flags present,
/// writable true / enumerable false / configurable false (§10.2.5).
const PROTO_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | 1;
/// `constructor` entry: value present + all three present, writable
/// true / enumerable false / configurable true (§10.2.5 step 3).
const CTOR_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2) | 1;

/// Interned key cells — `mint_immortal_str` MINTS (one fresh
/// immortal cell per call, the ns-name-cell pattern), so the two
/// keys are lazily minted ONCE and cached (a per-materialization
/// mint leaked ~80B × every plain-fn cell, churn-probe visible).
static PROTO_KEY_CELL: AtomicU64 = AtomicU64::new(0);
static CTOR_KEY_CELL: AtomicU64 = AtomicU64::new(0);

fn interned_key(slot: &AtomicU64, name: &[u8]) -> *mut c_void {
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut c_void;
    }
    let cell = mint_immortal_str(name);
    slot.store(cell as u64, Ordering::Relaxed);
    cell.cast::<c_void>()
}

/// True when the closure cell was minted from a plain `function`
/// form — the compiler stamped `FLAG_FN_PROTO` into the header flags
/// half-word at env alloc.
unsafe fn has_fn_proto_flag(ptr: *const c_void) -> bool {
    let flags = unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() };
    flags & FLAG_FN_PROTO != 0
}

/// Materialize (or answer the already-minted) `.prototype` of a
/// plain-fn closure as a borrow pair; `None` for every cell without
/// the flag. Probes the props dynobj itself before minting (rotation
/// 345): the original contract left the probe to the member-get
/// callers, and the two non-member-get consumers that arrived since
/// (`construct_plain_fn`, the instanceof fn-value walk) called
/// straight in — every call minted a FRESH prototype and overwrote
/// the entry, so `o instanceof C` compared o's chain against a
/// just-minted twin and repeated `Reflect.construct(C)` products
/// diverged. The probe makes the mint once-per-cell wherever the
/// call comes from; the member-get pre-probes stay as fast paths.
pub(crate) unsafe fn fn_prototype_pair(ptr: *mut c_void) -> Option<(u64, u64)> {
    if !unsafe { has_fn_proto_flag(ptr) } {
        return None;
    }
    unsafe {
        let props = closure_props(ptr);
        if !props.is_null() {
            let key = interned_key(&PROTO_KEY_CELL, b"prototype");
            let tag = __torajs_dynobj_get_tag(props, key);
            if tag != 5 {
                return Some((tag, __torajs_dynobj_get_value(props, key)));
            }
        }
        // Fresh prototype object with the `constructor` back-ref.
        // The closure ref is inc'd because define consumes one rc of
        // its heap value; the resulting cycle is collector-walked.
        let mut proto = __torajs_dynobj_alloc();
        torajs_rc::__torajs_rc_inc(ptr);
        __torajs_dynobj_define(
            &mut proto,
            interned_key(&CTOR_KEY_CELL, b"constructor"),
            ANY_HEAP,
            ptr as u64,
            CTOR_ENTRY_FLAGS,
        );
        // Install into the closure's props dynobj (minting the table
        // on first expando, the member-set precedent). The define
        // consumes the alloc's +1 — the entry owns the prototype.
        let props_slot = ptr.cast::<u8>().add(24) as *mut *mut c_void;
        if (*props_slot).is_null() {
            *props_slot = __torajs_dynobj_alloc();
        }
        __torajs_dynobj_define(
            props_slot,
            interned_key(&PROTO_KEY_CELL, b"prototype"),
            ANY_HEAP,
            proto as u64,
            PROTO_ENTRY_FLAGS,
        );
        Some((ANY_HEAP, proto as u64))
    }
}

/// Compiler face for the typed lane (`fun.prototype` on a
/// closure-typed receiver) — the materialized object as an OWNED
/// AnyValue (+1 for the caller), or undefined for a cell without the
/// flag.
///
/// # Safety
/// `env` is a live closure cell pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_prototype_any(env: *mut c_void) -> u64 {
    unsafe {
        let props = closure_props(env);
        if !props.is_null() {
            let proto_key = interned_key(&PROTO_KEY_CELL, b"prototype");
            let tag = __torajs_dynobj_get_tag(props, proto_key);
            // An existing entry wins — including a user overwrite
            // (`fun.prototype = 42`), which can carry any slot tag.
            if tag != 5 {
                let v = __torajs_dynobj_get_value(props, proto_key);
                return own_pair(tag, v);
            }
        }
        match fn_prototype_pair(env) {
            Some((tag, v)) => own_pair(tag, v),
            None => crate::nanbox::VALUE_UNDEFINED,
        }
    }
}

/// Slot pair → OWNED boxed AnyValue: the typed lane's consumer drops
/// its result, so a heap payload takes +1 here (immediates don't).
unsafe fn own_pair(tag: u64, v: u64) -> u64 {
    if tag == ANY_HEAP {
        unsafe { torajs_rc::__torajs_rc_inc(v as *mut c_void) };
    }
    unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, v as i64) }
}

/// Install a generator factory's `.prototype` face at fncell mint
/// (G2, rotation 178) — the factory's prototype IS the `__Gen_<name>`
/// class proto (identical to `Object.getPrototypeOf(g())`), so the
/// mint defines that object into the fresh cell's props dynobj and
/// every member-get channel answers it from the ordinary props probe
/// (no flag-gated lazy path — the class proto already exists).
/// `proto` is a BORROWED boxed Any holding the proto dynobj; the
/// entry takes its own stake.
///
/// # Safety
/// `env` is a live closure cell fresh from the mint (props empty);
/// `proto` holds a live heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_install_gen_proto(env: *mut c_void, proto: u64) {
    unsafe {
        let ptr = crate::nanbox::as_void_ptr(proto);
        if ptr.is_null() {
            return;
        }
        torajs_rc::__torajs_rc_inc(ptr);
        let props_slot = env.cast::<u8>().add(24) as *mut *mut c_void;
        if (*props_slot).is_null() {
            *props_slot = __torajs_dynobj_alloc();
        }
        __torajs_dynobj_define(
            props_slot,
            interned_key(&PROTO_KEY_CELL, b"prototype"),
            ANY_HEAP,
            ptr as u64,
            PROTO_ENTRY_FLAGS,
        );
    }
}

/// Compiler face for the typed lane (`fun.constructor` on a
/// closure-typed receiver, RFC 20260721 刀 4) — an own `constructor`
/// expando shadows; otherwise the flavor-keyed interned ctor cell:
/// %AsyncFunction% for an async-form cell (`FLAG_FN_ASYNC`),
/// %Function% for every other closure. The cells are immortal
/// (static-flagged), so the box carries no rc traffic.
///
/// # Safety
/// `env` is a live closure cell pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_ctor_value(env: *mut c_void) -> u64 {
    unsafe {
        let props = closure_props(env);
        if !props.is_null() {
            let ctor_key = interned_key(&CTOR_KEY_CELL, b"constructor");
            let tag = __torajs_dynobj_get_tag(props, ctor_key);
            if tag != 5 {
                return own_pair(tag, __torajs_dynobj_get_value(props, ctor_key));
            }
        }
        let flags = (env.cast::<u8>().add(6) as *const u16).read();
        let tag = if flags & torajs_rc::FLAG_FN_ASYNC != 0 {
            14
        } else {
            13
        };
        crate::nanbox::box_void_ptr(crate::method_value::builtin_ctor_cell(tag) as *mut c_void)
    }
}
