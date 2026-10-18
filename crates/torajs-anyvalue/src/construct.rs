//! S-NEW 刀 2 — `new <expr>(args…)`, where the constructor is a value
//! rather than a name.
//!
//! A `new C(…)` whose target is spelled out never reaches here: the
//! desugar binds it to the synthesized `__new_<C>` factory at compile
//! time and calls that directly. This is the other half — the callee
//! arrived as an `any` and only the running program knows what it is.
//!
//! Two pieces:
//!
//! - a registry filled once per class at module init, mapping the
//!   `__class_<C>` singleton to the boxed adapter of its `__new_<C>`
//!   factory. The adapter is the same `(env, argv, argc) -> AnyValue`
//!   dual entry every any-world call already dispatches through
//!   ([`crate::method_call_closure_dispatch::invoke_boxed`]); no new
//!   calling convention is introduced.
//! - [`__torajs_anyv_construct`], which checks IsConstructor and then
//!   calls that adapter.
//!
//! The registry is an open-addressed table keyed on the class cell
//! pointer, not an array indexed by class tag. Going the other way —
//! from a class value back to its tag — would mean scanning the tag
//! table on every construct, and `new` inside a loop cannot afford a
//! scan when the object it allocates costs tens of nanoseconds.
//! Slots are `AtomicU64` per the multi-thread-ready substrate rule
//! (design principles §6.2), which also matches how torajs-throw
//! holds its native-error factories.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{FLAG_FN_PROTO, HeapHeader, Tag};

use crate::method_call_closure_dispatch::{invoke_boxed, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_object_create_link_proto(obj: *mut c_void, proto: u64);
    fn __torajs_throw_check() -> i64;
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
}

/// §7.3.22 OrdinaryHasInstance for the compile-time class lane's
/// DynObj receiver arm (405-01 face 2) — an `Object.create(
/// C.prototype)` shape carries no `class_tag@+8`, so the tag
/// comparison in `__torajs_instanceof_class_any_tag` can never see
/// it; the spec answer is whether the class's reified prototype sits
/// on the receiver's [[Prototype]] chain. Called once per descendant
/// tag by ssa_lower's existing OR-chain, so a single-tag identity
/// check suffices (a subclass prototype matches through its own
/// tag's call). A tag that never registered a prototype answers
/// false.
///
/// # Safety
/// `v` is a valid AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_instanceof_proto_chain_any_tag(
    v: u64,
    expected_tag: i64,
) -> bool {
    unsafe {
        let pval = __torajs_proto_cell_raw(expected_tag);
        if pval == 0 {
            return false;
        }
        crate::construct_has_instance::proto_chain_reaches(v, pval)
    }
}

/// Power of two, and comfortably above the 256 class tags
/// `torajs-meta` allows, so the table never fills and probe chains
/// stay at one or two steps.
const CTOR_SLOTS: usize = 512;
const CTOR_MASK: u64 = CTOR_SLOTS as u64 - 1;

/// Class cell pointer bits; 0 means the slot was never claimed.
static CTOR_KEY: [AtomicU64; CTOR_SLOTS] = [const { AtomicU64::new(0) }; CTOR_SLOTS];
/// Address of that class's boxed factory adapter.
static CTOR_ENTRY: [AtomicU64; CTOR_SLOTS] = [const { AtomicU64::new(0) }; CTOR_SLOTS];
/// MurmurHash3 finalizer, same mixer `fnprops` uses on fn pointers.
/// Heap pointers are 8-aligned, so the low bits alone would collide
/// every eighth class.
#[inline]
pub(crate) fn mix(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    k ^ (k >> 33)
}

/// Record the boxed factory adapter for a class object.
///
/// Emitted by the lowerer right after `__torajs_anyv_class_register`,
/// with the same class value that call received — so the pointer is
/// the module-scope singleton and stays valid for the process. The
/// table holds the bits only; it takes no reference and releases
/// none.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_ctor_register(class_av: AnyValue, entry: *mut c_void) {
    if !is_cell(class_av) || entry.is_null() {
        return;
    }
    let key = as_void_ptr(class_av) as u64;
    let mut i = (mix(key) & CTOR_MASK) as usize;
    for _ in 0..CTOR_SLOTS {
        let held = CTOR_KEY[i].load(Ordering::Relaxed);
        if held == 0 || held == key {
            CTOR_KEY[i].store(key, Ordering::Relaxed);
            CTOR_ENTRY[i].store(entry as u64, Ordering::Relaxed);
            return;
        }
        i = (i + 1) & (CTOR_SLOTS - 1);
    }
}

/// Look up a class object's factory adapter; 0 if it has none.
pub(crate) fn ctor_entry(key: u64) -> u64 {
    let mut i = (mix(key) & CTOR_MASK) as usize;
    for _ in 0..CTOR_SLOTS {
        let held = CTOR_KEY[i].load(Ordering::Relaxed);
        if held == key {
            return CTOR_ENTRY[i].load(Ordering::Relaxed);
        }
        if held == 0 {
            return 0;
        }
        i = (i + 1) & (CTOR_SLOTS - 1);
    }
    0
}

/// ES §7.2.3 IsCallable — does this value have a [[Call]] internal
/// method? Three cell shapes carry one in tr, and this is the only
/// place that enumerates them:
///
/// - a `Closure` cell — every ordinary function, arrow and method;
/// - a `__class_<C>` singleton dynobj — a class constructor HAS
///   [[Call]] (that is why §10.2.1 step 2 can refuse it by name);
///   builtin constructors read as values are these too, so
///   `String(Error)` asks this question;
/// - a Proxy whose target is callable (§10.5.14 step 7 fixes the
///   answer at creation).
///
/// §13.5.3 step 9 defines `typeof` as this predicate, so
/// [`crate::inspect::typeof_any`] answers "function" from here
/// rather than re-listing the shapes. Prefer this over
/// re-deriving the list: the class-constructor row was missing
/// from every hand-rolled copy in this crate, which is how
/// `String(Error)` came to throw.
///
/// # Safety
/// `av` must be a live AnyValue.
pub unsafe fn is_callable(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    let ptr = as_void_ptr(av);
    if ptr.is_null() {
        return false;
    }
    if crate::proxy::is_proxy(av) {
        return unsafe { crate::proxy_callable::proxy_is_callable(av) };
    }
    // SAFETY: caller guarantees a live heap block.
    let hdr = unsafe { &*(ptr as *const HeapHeader) };
    hdr.type_tag == Tag::Closure as u16 || unsafe { is_class_ctor_cell(ptr) }
}

/// ES §7.2.4 IsConstructor, over the shapes tr can answer for today.
///
/// A class object is a dynobj carrying `FLAG_DYNOBJ_CLASS_CTOR` — the
/// same bit that makes `typeof C` answer `"function"`. An ordinary
/// function reaches any-world as a closure cell, and the compiler
/// stamps `FLAG_FN_PROTO` on exactly the declaration forms that own a
/// `.prototype` — which per §10.2.4 MakeConstructor are exactly the
/// forms with [[Construct]] (arrows and methods carry neither), so
/// that one bit answers both questions.
///
/// # Safety
/// `av` must be a live AnyValue.
// one shared copy: inlined, the loop is copied into every caller
#[inline(never)]
pub unsafe fn is_constructor(av: AnyValue) -> bool {
    let mut av = av;
    loop {
        if !is_cell(av) {
            return false;
        }
        let ptr = as_void_ptr(av);
        if ptr.is_null() {
            return false;
        }
        // §10.5.14 step 3 — a Proxy has [[Construct]] exactly when its
        // target does; the handler has no say in it.
        if crate::proxy::is_proxy(av) {
            return unsafe { crate::proxy_callable::proxy_is_constructor(av) };
        }
        match unsafe { constructor_cell_shape(ptr) } {
            CellCtor::Answer(yes) => return yes,
            // A bound function's [[Construct]] exists exactly when its
            // target's does (§10.4.1.2: BoundFunctionCreate copies the
            // slot iff the target has one). The target is a whole
            // AnyValue (a cell pointer is its own encoding), asked the
            // same question again — Proxy row included — so a chain of
            // any depth is one loop, not one native frame per layer.
            CellCtor::Bound(target) => av = target,
        }
    }
}

/// What the cell level of [`is_constructor`] can say about one cell.
enum CellCtor {
    Answer(bool),
    /// A kind-1 bound cell: the answer is its target's.
    Bound(AnyValue),
}

/// Cell-level half of [`is_constructor`]. A kind-0 bound target is a
/// builtin prototype method — no [[Construct]]; a kind-1 bound cell
/// hands its target back to the value-level loop, not to this
/// function: a Proxy target is a question no cell shape answers yes
/// to, however constructible the Proxy's own target.
///
/// # Safety
/// `ptr` is a live heap cell.
unsafe fn constructor_cell_shape(ptr: *mut c_void) -> CellCtor {
    if unsafe { is_class_ctor_cell(ptr) } {
        return CellCtor::Answer(true);
    }
    // SAFETY: caller guarantees a live heap block, whose header is
    // the first `HeapHeader` bytes.
    let hdr = unsafe { &*(ptr as *const HeapHeader) };
    if hdr.type_tag != Tag::Closure as u16 {
        return CellCtor::Answer(false);
    }
    if hdr.flags & FLAG_FN_PROTO != 0 {
        return CellCtor::Answer(true);
    }
    // A builtin-constructor cell (`Number` / `Array` … read as a
    // value) — [[Construct]] is real for the wrapper / container
    // families; the per-tag dispatch raises the loud TypeError for
    // the rest, so answering true here never turns a refusal silent.
    if crate::method_value::ctor::ctor_tag_of_cell(ptr).is_some() {
        return CellCtor::Answer(true);
    }
    match unsafe { crate::method_bind::bound_cell_meta(ptr) } {
        Some((1, target, _)) => CellCtor::Bound(target),
        _ => CellCtor::Answer(false),
    }
}

/// A CLASS object — tr models a class constructor as a dynobj
/// carrying `FLAG_DYNOBJ_CLASS_CTOR` (the bit that makes `typeof C`
/// answer `"function"`); the closure-shaped probes never see it.
///
/// # Safety
/// `ptr` is a live heap cell.
pub(crate) unsafe fn is_class_ctor_cell(ptr: *mut c_void) -> bool {
    // SAFETY: caller guarantees a live heap block, whose header is
    // the first `HeapHeader` bytes.
    let hdr = unsafe { &*(ptr as *const HeapHeader) };
    hdr.type_tag == Tag::DynObj as u16 && hdr.flags & torajs_rc::FLAG_DYNOBJ_CLASS_CTOR != 0
}

/// ES §7.2.4 IsConstructor as a value-level predicate.
///
/// Exposed because test262's `isConstructor.js` needs exactly this
/// question answered without constructing anything — the stock harness
/// gets it from `Reflect.construct(function(){}, [], f)`, whose only
/// purpose is to probe `f`'s [[Construct]] without running `f`.
///
/// # Safety
/// `v` must be a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_is_constructor(v: AnyValue) -> bool {
    unsafe { is_constructor(v) }
}

/// §10.2.1 step 2 over a class object that arrived in a CALL
/// position — tr models one as a dynobj, so the any-lane dispatch
/// reaches its `not_callable()` fall-through rather than any rule
/// about classes. `None` when `av` is no class object at all.
///
/// Two answers, and which one depends on whether the compiler
/// injected the class: the §20.5 Error hierarchy is injected, and
/// every member of it is callable with `new`'s behaviour
/// (§20.5.1.1 "Error(message) … performs the same action" and its
/// NativeError / AggregateError twins), so `Error("m")` constructs.
/// A class the user wrote is step 2's refusal.
///
/// The refusal used to exist in exactly one place — the bound-call
/// path in `method_bind` — which is why `C.bind(null)()` named the
/// rule and a plain `C()` said "value is not a function".
///
/// # Safety
/// `argv` points at `argc` readable AnyValue slots, borrowed.
pub(crate) unsafe fn call_class_ctor(
    av: AnyValue,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    if !is_cell(av) {
        return None;
    }
    let ptr = as_void_ptr(av);
    if ptr.is_null() || !unsafe { is_class_ctor_cell(ptr) } {
        return None;
    }
    if unsafe { class_call_constructs(ptr) } {
        return Some(unsafe { __torajs_anyv_construct(av, argv, argc) });
    }
    unsafe {
        __torajs_throw_type_error(c"class constructor cannot be invoked without 'new'".as_ptr());
    }
    Some(VALUE_UNDEFINED)
}

/// torajs-meta's §20.5.1.1 registry — does a plain call of this
/// class CONSTRUCT? That is the Error hierarchy and nothing else,
/// which is NOT the same set as "injected intrinsic" (the disposable
/// stacks are intrinsics for the §10.1.13 prototype question, and
/// their plain call throws). The unit-test binary links without the
/// staticlib graph, so tests see every class as user-written (same
/// posture as `ctor_method`'s source-table stub).
#[cfg(not(test))]
#[inline]
unsafe fn class_call_constructs(ptr: *mut c_void) -> bool {
    unsafe extern "C" {
        fn __torajs_class_call_constructs_cell(cell: *const c_void) -> i32;
    }
    unsafe { __torajs_class_call_constructs_cell(ptr) != 0 }
}

#[cfg(test)]
#[inline]
unsafe fn class_call_constructs(_ptr: *mut c_void) -> bool {
    false
}

/// `new callee(argv[0..argc])` where `callee` is a runtime value.
///
/// Answers the constructed object, owned by the caller. A callee that
/// is not a constructor raises a TypeError and answers undefined, per
/// §13.3.5.1 step 7.
///
/// # Safety
/// `argv` must point at `argc` readable AnyValue slots, borrowed for
/// the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_construct(
    callee: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe { construct_with_nt(callee, argv, argc, callee) }
}

/// [`__torajs_anyv_construct`] with an explicit newTarget, which a
/// plain function reading `new.target` receives in its body
/// (`construct_new_target.rs`); `Reflect.construct` passes its third
/// operand, `new F()` the callee itself.
///
/// # Safety
/// As [`__torajs_anyv_construct`]; `new_target` is a live AnyValue.
pub(crate) unsafe fn construct_with_nt(
    callee: AnyValue,
    argv: *const u64,
    argc: i64,
    new_target: AnyValue,
) -> AnyValue {
    if !unsafe { is_constructor(callee) } {
        unsafe {
            __torajs_throw_type_error(c"value is not a constructor".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    // §10.5.13 — a Proxy runs its `construct` trap (or forwards to
    // the target) instead of the ordinary shapes below, with the
    // newTarget it was given: a bound layer above it may have swapped
    // one in, and `Reflect.construct` names its own.
    if crate::proxy::is_proxy(callee) {
        return unsafe { crate::proxy_callable::construct_proxy(callee, argv, argc, new_target) };
    }
    // A plain function constructs per §10.2.2 (base kind): fresh
    // `this` from its `.prototype`, then the ordinary call channel.
    let cell = as_void_ptr(callee);
    // SAFETY: is_constructor above proved a live cell header.
    let hdr = unsafe { &*(cell as *const HeapHeader) };
    if hdr.type_tag == Tag::Closure as u16 {
        // §10.4.1.2 — a bound function constructs its TARGET with
        // boundArgs ++ args; the bound this plays no part.
        if unsafe { crate::method_bind::bound_cell_meta(cell) }.is_some_and(|m| m.0 == 1) {
            return unsafe {
                crate::construct_bound::construct_bound(callee, argv, argc, new_target)
            };
        }
        // A builtin-constructor cell — the per-family [[Construct]]
        // (wrapper mints, containers, the loud reject for the rest).
        if let Some(tag) = crate::method_value::ctor::ctor_tag_of_cell(cell) {
            return unsafe { crate::method_value::ctor::ctor_construct(tag, argv, argc) };
        }
        return unsafe { construct_plain_fn(cell, argv, argc, new_target) };
    }
    let entry = ctor_entry(as_void_ptr(callee) as u64);
    if entry == 0
        && let Some(v) = unsafe { crate::construct_unit::construct_unit(callee, argv, argc) }
    {
        return v;
    }
    if entry == 0 {
        // The class is real but its factory has no boxed adapter —
        // an argument or return type outside the boxable set. Say so
        // rather than let the miss read as "not a constructor",
        // which would be a false statement about the program.
        unsafe {
            __torajs_throw_type_error(
                c"this constructor cannot be reached through a runtime value yet".as_ptr(),
            );
        }
        return VALUE_UNDEFINED;
    }
    unsafe { invoke_boxed(core::ptr::null_mut(), entry, argv, argc) }
}

/// §10.2.2 [[Construct]], base kind, over a plain-fn closure cell:
/// `this` is OrdinaryCreateFromConstructor(callee, "%Object.prototype%")
/// — a fresh dynobj linked to the callee's `.prototype` (minted on
/// first touch, `constructor` back-ref included) — the body runs
/// through the same receiver-routing dispatcher every any-world call
/// uses, and a non-object completion answers the fresh `this` (step
/// 10.b). A body that reads `new.target` is entered through its
/// registered construct face with `new_target`
/// (`construct_new_target.rs`); any other body ignores it.
///
/// # Safety
/// `cell` is a live closure cell carrying `FLAG_FN_PROTO`; `argv`
/// points at `argc` readable AnyValue slots.
unsafe fn construct_plain_fn(
    cell: *mut c_void,
    argv: *const u64,
    argc: i64,
    new_target: AnyValue,
) -> AnyValue {
    unsafe {
        let Some((_ptag, pval)) = crate::closure_proto::fn_prototype_pair(cell) else {
            // Unreachable behind the is_constructor gate, but a loud
            // answer beats UB if the gate ever drifts.
            __torajs_throw_type_error(c"value is not a constructor".as_ptr());
            return VALUE_UNDEFINED;
        };
        let this_obj = __torajs_dynobj_alloc();
        __torajs_object_create_link_proto(this_obj, __torajs_anyv_box_pointer(pval as *mut c_void));
        let this_av = __torajs_anyv_box_pointer(this_obj);
        let entry = crate::method_call_closure_dispatch::boxed_entry_of(cell as *const u8);
        let ctor_face = crate::construct_new_target::ctor_face_entry(entry);
        let result = if ctor_face != 0 {
            crate::construct_new_target::invoke_ctor_face(
                cell, ctor_face, this_av, new_target, argv, argc,
            )
        } else {
            invoke_with_this(cell, entry, this_av, argv, argc)
        };
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(this_av);
            return VALUE_UNDEFINED;
        }
        if crate::construct_has_instance::is_heap_object(result) {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(this_av);
            return result;
        }
        if is_cell(result) && !as_void_ptr(result).is_null() {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(result);
        }
        this_av
    }
}
