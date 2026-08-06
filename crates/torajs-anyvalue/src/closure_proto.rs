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
/// the flag. Caller (both member-get channels) has already probed the
/// props dynobj and missed — the mint below writes the entry there,
/// so the second channel's probe hits and this runs at most once per
/// cell.
pub(crate) unsafe fn fn_prototype_pair(ptr: *mut c_void) -> Option<(u64, u64)> {
    if !unsafe { has_fn_proto_flag(ptr) } {
        return None;
    }
    unsafe {
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

/// `%GeneratorPrototype%` step-method cells (RFC 20260721 刀 2) —
/// `next` / `return` / `throw` reified for the REFLECTION surface
/// (typeof / name / length / gOPD): interned per (kind, which),
/// `name` / `length` carried as own W0/E0/C1 props entries so the
/// existing closure member/gOPD chains answer them with zero new
/// plumbing. A detached CALL raises the recorded loud TypeError —
/// live stepping rides the generator instance's class methods, and
/// a receiver-generic re-dispatch is the recorded face.
static GEN_STEP_CELLS: [[AtomicU64; 3]; 2] = [
    [const { AtomicU64::new(0) }; 3],
    [const { AtomicU64::new(0) }; 3],
];
static GEN_STEP_NAMES: [&[u8]; 3] = [b"next", b"return", b"throw"];

unsafe extern "C" fn gen_step_reject_entry(
    _env: *mut c_void,
    _argv: *const u64,
    _argc: i64,
) -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"generator prototype step method called through a detached value is not supported"
                .as_ptr(),
        );
    }
    crate::nanbox::VALUE_UNDEFINED
}

/// The shared `%GeneratorPrototype%` (kind 0) /
/// `%AsyncGeneratorPrototype%` (kind 1) object, handed over by
/// torajs-meta's genfn mint as it wires the step cells in. Borrowed
/// pointers into immortal mint state — never released, only compared.
static GEN_PROTO_CELLS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// genfn-mint face — record the shared step-method prototype of
/// `kind` so [`receiver_is_gen_instance`] has a chain root to match.
///
/// # Safety
/// FFI face; `proto` is the mint-owned dynobj (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_gen_proto_register(kind: i64, proto: *mut c_void) {
    let k = if kind == 1 { 1usize } else { 0usize };
    GEN_PROTO_CELLS[k].store(proto as u64, Ordering::Relaxed);
}

/// Chain-walk bound — an instance sits exactly two hops under its
/// kind's shared prototype; the cap only stops a cyclic user chain.
const GEN_CHAIN_MAX_HOPS: usize = 32;

/// Does `recv` carry the `[[AsyncGeneratorState]]` slot §27.6.1.2
/// step 3 tests for (resp. `[[GeneratorState]]` for kind 0)?
///
/// tr keeps no internal-slot table, so the structural proxy is the
/// prototype chain. A `__Gen_<name>` INSTANCE reaches the shared
/// prototype in two hops (instance → its class proto → the shared
/// proto); the class proto itself (`g.prototype`) reaches it in one,
/// and the shared proto in zero. Requiring depth ≥ 2 therefore
/// accepts exactly the instances and rejects `g`, `g.prototype`,
/// `%AsyncGeneratorPrototype%` itself, sync-generator instances
/// (a different kind's root) and every non-generator value.
///
/// Boundary: a hand-built `Object.create(g.prototype)` has no state
/// slot yet answers `true`. That direction is deliberate — it keeps
/// the loud TypeError of the not-yet-implemented detached-step face
/// instead of quietly handing back a rejected promise that reads as
/// "this was never a generator".
unsafe fn receiver_is_gen_instance(recv: u64, kind: usize) -> bool {
    let target = GEN_PROTO_CELLS[kind].load(Ordering::Relaxed);
    if target == 0 || !crate::nanbox::is_cell(recv) {
        return false;
    }
    unsafe {
        let mut cur = __torajs_anyv_get_proto_of_any(recv);
        let mut depth = 1usize;
        while crate::nanbox::is_cell(cur) && depth <= GEN_CHAIN_MAX_HOPS {
            let hit = crate::nanbox::as_void_ptr(cur) as u64 == target;
            let next = if hit {
                0
            } else {
                __torajs_anyv_get_proto_of_any(cur)
            };
            crate::nanbox_ffi::__torajs_anyv_rc_dec(cur);
            if hit {
                return depth >= 2;
            }
            cur = next;
            depth += 1;
        }
    }
    false
}

/// A fresh already-rejected promise carrying a real TypeError
/// instance — §27.6.1.2-4 route a bad receiver through
/// AsyncGeneratorEnqueue step 3 (reject the capability's promise)
/// instead of throwing, so the caller's `.then` observes it.
///
/// The instance comes from the same native-error factory registry
/// that backs every runtime TypeError: raise it, then take it back
/// out of the pending-throw slot so it can settle the promise rather
/// than unwind. Taking also clears the active flag, which this path
/// requires — it returns a value, so the caller's `emit_throw_check`
/// must find a clean slot.
unsafe fn rejected_type_error_promise() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"AsyncGenerator.prototype step method called on a receiver that is not an async generator"
                .as_ptr(),
        );
        let tag = __torajs_throw_take_tag();
        let reason = __torajs_throw_take();
        // No factory registered (no Error class reachable in this
        // program) leaves a non-heap slot: there is nothing sound to
        // reject WITH, so re-arm and stay loud.
        if tag != ANY_HEAP as i64 || reason == 0 {
            __torajs_throw_set(tag, reason);
            return crate::nanbox::VALUE_UNDEFINED;
        }
        let p = __torajs_promise_alloc_rejected_heap(reason);
        if p.is_null() {
            __torajs_throw_set(tag, reason);
            return crate::nanbox::VALUE_UNDEFINED;
        }
        __torajs_promise_stamp_repr(p, crate::promise_with_resolvers::REPR_ANY as i64);
        crate::nanbox::box_void_ptr(p)
    }
}

/// The async trio's call face (§27.6.1.2-4). Unlike the sync kind,
/// a bad receiver REJECTS rather than throws, so this entry has to
/// tell a genuine async generator from everything else.
unsafe extern "C" fn async_gen_step_reject_entry(
    _env: *mut c_void,
    argv: *const u64,
    argc: i64,
) -> u64 {
    // FLAG_CLOSURE_RECV_FIRST puts the call-site `this` in slot 0; a
    // bare `AGP.next()` lands `undefined` there, which is exactly the
    // non-object receiver step 3 rejects.
    let recv = if argc > 0 && !argv.is_null() {
        unsafe { *argv }
    } else {
        crate::nanbox::VALUE_UNDEFINED
    };
    if unsafe { receiver_is_gen_instance(recv, 1) } {
        // A real async generator: stepping it through the detached
        // face needs the receiver-generic re-dispatch tr does not
        // have yet. Keep the loud TypeError — answering a rejected
        // promise here would misreport a live generator as an
        // incompatible receiver.
        unsafe {
            __torajs_throw_type_error(
                c"generator prototype step method called through a detached value is not supported"
                    .as_ptr(),
            );
        }
        return crate::nanbox::VALUE_UNDEFINED;
    }
    unsafe { rejected_type_error_promise() }
}

/// The interned `%GeneratorPrototype%.next/return/throw` cell —
/// torajs-meta's genfn trio mint defines these into the shared
/// gen_proto. `kind` 0 = generator / 1 = async generator; `which`
/// indexes [`GEN_STEP_NAMES`]. Answers an immortal cell (borrow).
///
/// # Safety
/// FFI face; indices are clamped by the callers (genfn mint).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_gen_step_method_cell(kind: i64, which: i64) -> *mut u8 {
    let k = if kind == 1 { 1usize } else { 0usize };
    let w = (which as usize).min(2);
    let slot = &GEN_STEP_CELLS[k][w];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    // §27.5.1.2-4 THROW on a bad receiver; §27.6.1.2-4 REJECT (the
    // async trio routes through AsyncGeneratorEnqueue), so the two
    // kinds carry different call faces.
    let cell = if k == 1 {
        crate::method_value::mint_reject_closure_cell(async_gen_step_reject_entry)
    } else {
        crate::method_value::mint_reject_closure_cell(gen_step_reject_entry)
    };
    unsafe {
        if k == 1 {
            // The async face must SEE the receiver to separate a live
            // generator from the values step 3 rejects; bit 12 makes
            // the dispatcher pass the call-site `this` in argv[0].
            *(cell.add(6) as *mut u16) |= torajs_rc::FLAG_CLOSURE_RECV_FIRST;
        }
        // name / length as own props entries ({W:0, E:0, C:1},
        // §27.5.1.2-4: every step method's length is 1).
        let props_slot = cell.add(24) as *mut *mut c_void;
        *props_slot = __torajs_dynobj_alloc();
        let name_cell = mint_immortal_str(GEN_STEP_NAMES[w]);
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
            1,
            REFLECT_ENTRY_FLAGS,
        );
    }
    slot.store(cell as u64, Ordering::Relaxed);
    cell
}

/// `ANY_I64` slot tag (torajs-dynobj `layout.rs` mirror).
const ANY_I64: u64 = 2;
/// Reflection entry: value present + all three present, writable
/// false / enumerable false / configurable true.
const REFLECT_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2);

static NAME_KEY_CELL: AtomicU64 = AtomicU64::new(0);
static LENGTH_KEY_CELL: AtomicU64 = AtomicU64::new(0);

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
