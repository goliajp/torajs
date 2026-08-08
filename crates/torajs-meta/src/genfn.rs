//! `%GeneratorFunction%` / `%AsyncGeneratorFunction%` intrinsic
//! reflection cells (RFC 20260713-generator-fn-value-substrate
//! blade 5 cut 4).
//!
//! Per ES §27.3 / §27.4, every generator function's [[Prototype]] is
//! `%GeneratorFunction.prototype%` (an ordinary object, NOT a
//! function), whose `constructor` is the `%GeneratorFunction%`
//! intrinsic and whose `prototype` is `%GeneratorPrototype%` — the
//! shared [[Prototype]] of every per-generator `.prototype` object.
//! Async generators mirror the trio under §27.4.
//!
//! tr mints each kind's trio lazily as dynobj singletons (the same
//! immortal-singleton shape as `builtin_proto.rs`), wired with the
//! spec attribute flags via the DefineOwnProperty kernel:
//!
//! - fn_proto.constructor → ctor            {W:0, E:0, C:1} §27.3.3.1
//! - fn_proto.prototype   → gen_proto       {W:0, E:0, C:1} §27.3.3.2
//! - fn_proto.[[Prototype]] → Function.prototype (internal
//!   PROTO_SLOT_KEY entry)
//! - ctor.name            → "GeneratorFunction" {W:0, E:0, C:1}
//! - ctor.length          → 1               {W:0, E:0, C:1}
//! - ctor.prototype       → fn_proto        {W:0, E:0, C:0} §27.3.2.1
//! - gen_proto.constructor → fn_proto       {W:0, E:0, C:1} §27.5.1.1
//!
//! Known boundary: `%GeneratorFunction%` is a dynobj, not a callable
//! cell — `typeof` answers "object" and dynamic construction
//! (`GeneratorFunction("yield 1")`) is out of scope (eval surface).
//! ctor.__proto__ (%Function%) is unwired for the same reason.

//! Capacity invariant: every dynobj minted or written here stays at
//! ≤ 4 entries (gen_proto: constructor + next/return/throw), under
//! DYNOBJ_INITIAL_CAP's 7-entry dense capacity — no insert below
//! ever relocates a cell, so the raw pointers held in `CELLS` (and
//! the circular cross-links) stay valid without the obj_slot
//! writeback dance.

use core::ffi::c_void;

use crate::reflect::{PROTO_SLOT_ATTRS, PROTO_SLOT_KEY, alloc_str_key, is_cell_imm};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    /// torajs-anyvalue — the interned `%GeneratorPrototype%` step
    /// method cell (`which`: 0 next / 1 return / 2 throw), a
    /// reflection-surface function (RFC 20260721 刀 2).
    fn __torajs_gen_step_method_cell(kind: i64, which: i64) -> *mut u8;
    /// torajs-anyvalue — hand the shared step-method prototype over
    /// so the async call face can recognise its own instances by
    /// walking a receiver's chain to this object (§27.6.1.2 step 3).
    fn __torajs_gen_proto_register(kind: i64, proto: *mut c_void);
    /// torajs-anyvalue — the interned `[Symbol.asyncDispose]` cell
    /// the kind-1 prototype carries (§27.1.6.1 semantics; RFC
    /// 20260809 B6 async leg).
    fn __torajs_gen_asyncdispose_cell() -> *mut u8;
    /// torajs-str — the idx-th well-known symbol singleton (owned
    /// +1; immortal, ledger-free). 0 = asyncDispose (alphabetical).
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
}

const ANY_I64: u64 = 2;
const ANY_HEAP: u64 = 4;

/// `layout.rs` DEFINE_* mirrors (flags-byte ABI): value-present +
/// all-three-attrs-present, with the attr bits themselves cleared
/// except as noted.
const PRESENT_ALL_VALUE: u64 = (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6);
const FLAG_CONFIGURABLE: u64 = 1 << 2;
/// {writable:false, enumerable:false, configurable:true}
const ATTRS_WEC_001: u64 = PRESENT_ALL_VALUE | FLAG_CONFIGURABLE;
/// {writable:false, enumerable:false, configurable:false}
const ATTRS_WEC_000: u64 = PRESENT_ALL_VALUE;
/// {writable:true, enumerable:false, configurable:true} — the
/// §27.5.1 step-method attribute set.
const ATTRS_WEC_101: u64 = PRESENT_ALL_VALUE | FLAG_CONFIGURABLE | 1;

/// Builtin-proto singleton tag for Function.prototype
/// (`builtin_proto.rs` tag space).
const FUNCTION_PROTO_TAG: i64 = 13;

/// Builtin-proto singleton tag for %Iterator.prototype%
/// (RFC 20260730-iterator-global 刀 1).
const ITERATOR_PROTO_TAG: i64 = 15;

/// Per-kind trio: [fn_proto, ctor, gen_proto]. kind 0 = generator,
/// kind 1 = async generator. Pointer addresses stored as usize so
/// the statics stay Sync (multi-thread-ready shape); slot 0 doubles
/// as the installed flag, and the mint itself is serialized behind
/// GENFN_LOCK so the trio installs atomically (mirrors fnprops'
/// lock-gated table rather than builtin_proto's single-slot CAS —
/// three cross-linked slots can't CAS-install independently).
#[allow(clippy::declare_interior_mutable_const)]
const CELL_INIT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static CELLS: [[core::sync::atomic::AtomicUsize; 3]; 2] = [[CELL_INIT; 3], [CELL_INIT; 3]];
static GENFN_LOCK: torajs_mutex::Mutex<()> = torajs_mutex::Mutex::new(());

#[inline]
fn heap_anyv(p: *mut c_void) -> u64 {
    // Heap pointers ARE the cell-encoded AnyValue (48-bit user VA,
    // top bits clear) — same encoding box_pair_imm(ANY_HEAP, p)
    // produces for cells.
    p as u64
}

unsafe fn define_heap(obj: *mut c_void, key: &[u8], value: *mut c_void, flags: u64) {
    let mut slot = obj;
    let k = unsafe { alloc_str_key(key) };
    // define consumes one rc of an ANY_HEAP value — the singletons
    // are immortal, so the extra inc keeps the circular graph alive
    // by design (mirrors builtin_proto's leaked singletons).
    unsafe { __torajs_rc_inc(value) };
    unsafe { __torajs_dynobj_define(&mut slot, k, ANY_HEAP, heap_anyv(value), flags) };
    unsafe { __torajs_str_drop(k) };
}

unsafe fn mint_kind(kind: usize) {
    let fn_proto = unsafe { __torajs_dynobj_alloc() };
    let ctor = unsafe { __torajs_dynobj_alloc() };
    let gen_proto = unsafe { __torajs_dynobj_alloc() };

    // fn_proto: constructor / prototype / __proto__.
    unsafe { define_heap(fn_proto, b"constructor", ctor, ATTRS_WEC_001) };
    unsafe { define_heap(fn_proto, b"prototype", gen_proto, ATTRS_WEC_001) };
    let func_proto = unsafe { __torajs_get_builtin_prototype(FUNCTION_PROTO_TAG) };
    if !func_proto.is_null() {
        unsafe { define_heap(fn_proto, PROTO_SLOT_KEY, func_proto, PROTO_SLOT_ATTRS) };
    }

    // ctor: name / length / prototype.
    let name: &[u8] = if kind == 0 {
        b"GeneratorFunction"
    } else {
        b"AsyncGeneratorFunction"
    };
    let name_str = unsafe { alloc_str_key(name) };
    {
        let mut slot = ctor;
        let k = unsafe { alloc_str_key(b"name") };
        unsafe {
            __torajs_dynobj_define(
                &mut slot,
                k,
                ANY_HEAP,
                heap_anyv(name_str as *mut c_void),
                ATTRS_WEC_001,
            )
        };
        unsafe { __torajs_str_drop(k) };
    }
    {
        let mut slot = ctor;
        let k = unsafe { alloc_str_key(b"length") };
        unsafe { __torajs_dynobj_define(&mut slot, k, ANY_I64, 1, ATTRS_WEC_001) };
        unsafe { __torajs_str_drop(k) };
    }
    unsafe { define_heap(ctor, b"prototype", fn_proto, ATTRS_WEC_000) };

    // gen_proto: constructor back-link (§27.5.1.1 / §27.6.1.1).
    unsafe { define_heap(gen_proto, b"constructor", fn_proto, ATTRS_WEC_001) };
    // §27.1.2 — %GeneratorPrototype%.[[Prototype]] is
    // %Iterator.prototype% (sync kind only; the async trio's parent
    // is %AsyncIteratorPrototype%, which tr does not have — that
    // link stays absent, recorded boundary). RFC
    // 20260730-iterator-global 刀 1.
    if kind == 0 {
        let iter_proto = unsafe { __torajs_get_builtin_prototype(ITERATOR_PROTO_TAG) };
        if !iter_proto.is_null() {
            unsafe { define_heap(gen_proto, PROTO_SLOT_KEY, iter_proto, PROTO_SLOT_ATTRS) };
        }
    }
    // §27.1.6.1 — the async family's [@@asyncDispose]. Spec hangs it
    // on %AsyncIteratorPrototype%, which tr does not have (recorded
    // boundary above); the kind-1 shared prototype is the chain root
    // every async-generator instance walks, so the face lives here.
    // Symbol-keyed define: the singleton key is immortal, the extra
    // value inc keeps the immortal cell's circular graph alive
    // (define_heap posture).
    if kind == 1 {
        let cell = unsafe { __torajs_gen_asyncdispose_cell() } as *mut c_void;
        let sym = unsafe { __torajs_symbol_well_known(0) };
        if !sym.is_null() && !cell.is_null() {
            unsafe { __torajs_rc_inc(cell) };
            let mut slot = gen_proto;
            unsafe {
                __torajs_dynobj_define(
                    &mut slot,
                    sym as *const u8,
                    ANY_HEAP,
                    heap_anyv(cell),
                    ATTRS_WEC_101,
                )
            };
        }
    }
    // gen_proto: next / return / throw step methods (§27.5.1.2-4 /
    // §27.6.1.2-4, {W:1, E:0, C:1}) — interned reflection cells
    // (RFC 20260721 刀 2; the call face records a loud reject, live
    // stepping rides the generator instance's class methods).
    // Register BEFORE minting the cells: the async face reads this
    // slot on every detached call, and the first such call can only
    // happen once `gen_proto` is reachable from user code.
    unsafe { __torajs_gen_proto_register(kind as i64, gen_proto) };
    for (which, name) in [(0i64, b"next" as &[u8]), (1, b"return"), (2, b"throw")] {
        let cell = unsafe { __torajs_gen_step_method_cell(kind as i64, which) };
        unsafe { define_heap(gen_proto, name, cell as *mut c_void, ATTRS_WEC_101) };
    }

    use core::sync::atomic::Ordering;
    CELLS[kind][1].store(ctor as usize, Ordering::Release);
    CELLS[kind][2].store(gen_proto as usize, Ordering::Release);
    CELLS[kind][0].store(fn_proto as usize, Ordering::Release);
}

#[inline]
unsafe fn cell(kind: i64, idx: usize) -> u64 {
    use core::sync::atomic::Ordering;
    let k = if kind == 1 { 1usize } else { 0usize };
    {
        let _g = GENFN_LOCK.lock();
        if CELLS[k][0].load(Ordering::Acquire) == 0 {
            unsafe { mint_kind(k) };
        }
    }
    let p = CELLS[k][idx].load(Ordering::Acquire) as *mut c_void;
    unsafe { __torajs_rc_inc(p) };
    heap_anyv(p)
}

/// `%GeneratorFunction.prototype%` (kind 0) /
/// `%AsyncGeneratorFunction.prototype%` (kind 1) as an owned
/// AnyValue — what `Object.getPrototypeOf(<generator fn>)` answers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_genfn_proto(kind: i64) -> u64 {
    unsafe { cell(kind, 0) }
}

/// Chain a per-generator `__proto___Gen_<name>` object (passed as
/// the AnyValue its module-scope binding holds) to the shared
/// `%GeneratorPrototype%` of `kind`: writes the internal
/// [`PROTO_SLOT_KEY`] entry the `get_proto_of_any` dynobj arm reads.
/// Non-cell / null input is a no-op (misordered toolchain
/// resilience, mirrors classmeta).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_genfn_chain(proto_anyv: u64, kind: i64) -> u64 {
    if !is_cell_imm(proto_anyv) {
        return 0;
    }
    let obj = proto_anyv as *mut c_void;
    let gen_proto_anyv = unsafe { cell(kind, 2) };
    let mut slot = obj;
    let k = unsafe { alloc_str_key(PROTO_SLOT_KEY) };
    unsafe { __torajs_dynobj_define(&mut slot, k, ANY_HEAP, gen_proto_anyv, PROTO_SLOT_ATTRS) };
    unsafe { __torajs_str_drop(k) };
    0
}
