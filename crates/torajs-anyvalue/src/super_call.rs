//! Value-shaped-parent `super(…)` dispatch (RFC 20260815 knife 2b).
//!
//! A class whose heritage is an expression (`class D extends box.cls`)
//! lowers on the capturing lane: `D` is a function value, its instance
//! a dynobj, and `super(…)` must run the PARENT's constructor logic on
//! that externally-allocated `this`. The parent is only known at run
//! time, so the dispatch is:
//!
//! - a CLASS object → its `__ctorany_<P>` receiver-polymorphic ctor
//!   twin (405-01: head `(__this, __new_target, …user)`, every param
//!   `any`, field writes through the any-member lane — a dynobj
//!   receiver is exactly what it exists for). Reached through a
//!   registry keyed on the class cell, filled at module init beside
//!   the factory-adapter registration ([`crate::construct`]).
//! - a CLOSURE (plain function parent) → the ordinary receiver-honoring
//!   call channel, §10.2.1 [[Call]] with `this` bound.
//! - a PROXY → its [[Construct]] with the subclass's new.target.
//! - anything else → §15.7.14 step 5's TypeError.
//!
//! Arguments arrive as one dense `Arr<Any>` the capturing lane's
//! rewrite packs at the super site (an array literal for an explicit
//! `super(a, b)`, the rest binding itself for the implicit forwarding
//! ctor) — the packed shape is what lets one kernel take both without
//! a spread protocol.
//!
//! A function parent's answer is handed back OWNED — the super site
//! takes §10.2.2 step 13 on it (`torajs-core`
//! `ast/capturing_classes/return_override.rs`: an object becomes the
//! subclass's `this`), and until it did, an object the parent returned
//! was never released. A class parent's twin answers the same way
//! (what its body returned, else the receiver). Errors ride the
//! pending-throw channel.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{FLAG_DYNOBJ_CLASS_CTOR, HeapHeader, Tag};

use crate::construct::mix;
use crate::method_call::MAX_BOXED_ARGS;
use crate::method_call_closure_dispatch::{closure_boxed_entry, invoke_boxed, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_NULL, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-meta — [[ConstructorKind]] of a registered class object.
    fn __torajs_class_kind_derived(cell: *const c_void) -> bool;
}

/// Arr cell length slot — mirror of torajs-arr `layout::ARR_LEN_OFF`.
const ARR_LEN_OFF: usize = 8;

const CTORANY_SLOTS: usize = 512;
const CTORANY_MASK: u64 = CTORANY_SLOTS as u64 - 1;

/// Class cell pointer bits; 0 means the slot was never claimed.
static CTORANY_KEY: [AtomicU64; CTORANY_SLOTS] = [const { AtomicU64::new(0) }; CTORANY_SLOTS];
/// Address of that class's `__ctorany_<C>` boxed adapter.
static CTORANY_ENTRY: [AtomicU64; CTORANY_SLOTS] = [const { AtomicU64::new(0) }; CTORANY_SLOTS];

/// Record the ctor-twin adapter for a class object. Emitted at module
/// init right beside `__torajs_anyv_ctor_register`, with the same
/// module-scope singleton class value — bits only, no reference
/// taken.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_ctorany_register(class_av: AnyValue, entry: *mut c_void) {
    if !is_cell(class_av) || entry.is_null() {
        return;
    }
    let key = as_void_ptr(class_av) as u64;
    let mut i = (mix(key) & CTORANY_MASK) as usize;
    for _ in 0..CTORANY_SLOTS {
        let held = CTORANY_KEY[i].load(Ordering::Relaxed);
        if held == 0 || held == key {
            CTORANY_KEY[i].store(key, Ordering::Relaxed);
            CTORANY_ENTRY[i].store(entry as u64, Ordering::Relaxed);
            return;
        }
        i = (i + 1) & (CTORANY_SLOTS - 1);
    }
}

fn ctorany_entry(key: u64) -> u64 {
    let mut i = (mix(key) & CTORANY_MASK) as usize;
    for _ in 0..CTORANY_SLOTS {
        let held = CTORANY_KEY[i].load(Ordering::Relaxed);
        if held == key {
            return CTORANY_ENTRY[i].load(Ordering::Relaxed);
        }
        if held == 0 {
            return 0;
        }
        i = (i + 1) & (CTORANY_SLOTS - 1);
    }
    0
}

/// §15.7.14 ClassDefinitionEvaluation step 5 — the class-definition-
/// time heritage gate the capturing lane emits before a value-shaped
/// parent's constructor binding: `null` passes (protoParent null is a
/// legal shape of its own), so does any value IsConstructor answers
/// true for (a class object, a plain `function`, a builtin constructor
/// such as `Symbol`, a bound constructor, a Proxy over one), and
/// anything else records the spec's TypeError (JSC's spelling, which
/// bun surfaces).
///
/// # Safety
/// `parent` is a live AnyValue; borrowed for the check.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_heritage_check(parent: AnyValue) {
    if parent == VALUE_NULL {
        return;
    }
    if unsafe { crate::construct::is_constructor(parent) } {
        return;
    }
    unsafe {
        __torajs_throw_type_error(c"The superclass is not a constructor.".as_ptr());
    }
}

/// `super(…args)` against a runtime parent value, `this` already
/// allocated by the subclass; `args_arr` is the dense `Arr<Any>` the
/// lane packed at the site; `new_target` is the subclass constructor's
/// (§13.3.7.1 step 5), borrowed.
///
/// # Safety
/// `args_arr` must be a live `Arr` cell, borrowed for the duration of
/// the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_super_call(
    parent: AnyValue,
    this_v: AnyValue,
    args_arr: AnyValue,
    new_target: AnyValue,
) -> AnyValue {
    let arr = if is_cell(args_arr) {
        as_void_ptr(args_arr)
    } else {
        core::ptr::null_mut()
    };
    if arr.is_null() {
        unsafe {
            __torajs_throw_type_error(c"super argument pack is not an array".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    // SAFETY: caller guarantees a live Arr cell.
    let n = unsafe { *(arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64) } as usize;
    // Element boxes are owned temps (+1 per cell) — released after
    // the invoke; the common small shape stays on the stack (mirror
    // of `apply_list`'s buffer split, minus the head slots below).
    let mut heap: Vec<u64>;
    let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
    let elems: &[u64] = if n > MAX_BOXED_ARGS {
        heap = Vec::with_capacity(n);
        for i in 0..n {
            heap.push(unsafe { __torajs_arr_index_get(arr, i as i64) });
        }
        &heap
    } else {
        for (i, slot) in buf.iter_mut().enumerate().take(n) {
            *slot = unsafe { __torajs_arr_index_get(arr, i as i64) };
        }
        &buf[..n]
    };
    let release = |elems: &[u64]| {
        for b in elems {
            unsafe { __torajs_anyv_rc_dec(*b) };
        }
    };
    // §10.5.13 — a Proxy parent is [[Construct]]ed with this
    // new.target (its trap, or its target under the same newTarget,
    // `this` from newTarget's prototype); the object it answers becomes
    // the subclass's `this` at the site
    if crate::proxy::is_proxy(parent) {
        let out = unsafe {
            crate::reflect_construct::construct_with_new_target(
                parent,
                elems.as_ptr(),
                n as i64,
                new_target,
            )
        };
        release(elems);
        return out;
    }
    if is_cell(parent) {
        let cell = as_void_ptr(parent);
        if !cell.is_null() {
            // SAFETY: a live cell's header is its first HeapHeader
            // bytes.
            let hdr = unsafe { &*(cell as *const HeapHeader) };
            if hdr.type_tag == Tag::DynObj as u16 && hdr.flags & FLAG_DYNOBJ_CLASS_CTOR != 0 {
                // K5-4 — one evaluation of a unit class is not in the
                // by-cell table; its registered template's twin is
                // the same code (`construct_unit`'s reasoning).
                let entry = match ctorany_entry(cell as u64) {
                    0 => unsafe { crate::construct_unit::unit_template_cell(cell) }
                        .map_or(0, ctorany_entry),
                    entry => entry,
                };
                if entry == 0 {
                    release(elems);
                    unsafe {
                        __torajs_throw_type_error(
                            c"this parent class cannot be reached through a runtime value yet"
                                .as_ptr(),
                        );
                    }
                    return VALUE_UNDEFINED;
                }
                // Twin head is `(__this, __new_target, …user)`.
                let mut full = vec![VALUE_UNDEFINED; n + 2];
                full[0] = this_v;
                full[1] = new_target;
                full[2..2 + n].copy_from_slice(elems);
                let out = unsafe {
                    invoke_boxed(core::ptr::null_mut(), entry, full.as_ptr(), (n + 2) as i64)
                };
                release(elems);
                // §10.2.2 step 13.c for the PARENT: this call is where
                // its [[Construct]] step 13 runs (the twin is entered
                // directly), and the site's pick cannot know a value's
                // kind — a DERIVED parent answering a non-object,
                // non-undefined value is a TypeError here.
                if !crate::to_primitive::is_object_value(out)
                    && out != VALUE_UNDEFINED
                    && unsafe { __torajs_throw_check() } == 0
                    && unsafe { __torajs_class_kind_derived(cell) }
                {
                    unsafe {
                        __torajs_anyv_rc_dec(out);
                        __torajs_throw_type_error(
                            c"derived constructor may only return an object or undefined".as_ptr(),
                        );
                    }
                    return VALUE_UNDEFINED;
                }
                return out;
            }
        }
        // §20.4.1.1 / §21.2.1.1 — Symbol (5) and BigInt (6) are
        // constructors that refuse [[Construct]]: a class may extend
        // them, and `super()` against them throws
        if matches!(
            crate::method_value::ctor::ctor_tag_of_cell(cell),
            Some(5 | 6)
        ) {
            release(elems);
            unsafe {
                __torajs_throw_type_error(c"function is not a constructor".as_ptr());
            }
            return VALUE_UNDEFINED;
        }
        if let Some((env, entry)) = unsafe { closure_boxed_entry(parent) } {
            // A plain-function parent reading `new.target` is entered
            // through its construct face (RFC 20260917-plain-fn-new-target
            // knife 4): SuperCall is a [[Construct]] with this new.target.
            let face = crate::construct_new_target::ctor_face_entry(entry);
            unsafe {
                let out = if face != 0 {
                    crate::construct_new_target::invoke_ctor_face(
                        env,
                        face,
                        this_v,
                        new_target,
                        elems.as_ptr(),
                        n as i64,
                    )
                } else {
                    invoke_with_this(env, entry, this_v, elems.as_ptr(), n as i64)
                };
                release(elems);
                return out;
            }
        }
    }
    release(elems);
    unsafe {
        __torajs_throw_type_error(c"Class extends value is not a constructor".as_ptr());
    }
    VALUE_UNDEFINED
}
