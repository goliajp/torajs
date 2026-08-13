//! §13.10.2 InstanceofOperator(V, target) where `target` is a plain
//! VALUE rather than a declared class — `x instanceof Odd` with
//! `const Odd = {}` behind the name.
//!
//! The compile-time lane in torajs-core's `ssa_lower_instanceof`
//! answers class membership from the heap tag, which is why a name
//! that resolves to no class at all used to constant-fold to `false`.
//! That fold is only right when the target carries no `@@hasInstance`
//! — and any object can carry one, installed by `defineProperty` or
//! written as a computed key in an object literal. The fold cannot
//! see either, so those spellings answered `false` while the handler
//! sat there unread.
//!
//! Spec order, and why each step is where it is:
//!
//! 1. **target is not an Object → TypeError.** Ahead of the handler
//!    probe, so `x instanceof 42` throws rather than reading a
//!    property off a number.
//! 2. **GetMethod(target, @@hasInstance).** The symbol face already
//!    resolves the dict / monkey-patch / reify layers
//!    ([`crate::member_get_symbol::symbol_key_pair`]) — the same
//!    lookup `Odd[Symbol.hasInstance]` performs as an expression, so
//!    the two spellings cannot disagree.
//! 3. **Call(handler, target, « V »)** with the handler's `this`
//!    bound to the target, then ToBoolean on the result — the return
//!    value is coerced, not required to be a boolean (`return "yes"`
//!    answers true).
//! 4. **IsCallable(target) is false → TypeError**, reached only when
//!    no handler was found.
//! 5. **OrdinaryHasInstance** — delegated to the existing prototype
//!    walk in [`crate::construct::__torajs_instanceof_fn_value`], so
//!    a callable target keeps the answer it already had.
//!
//! A handler that throws leaves the exception pending and answers
//! `false`; the call site's throw check propagates it before the
//! boolean is ever read.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::construct::is_heap_object;
use crate::member_get_symbol::symbol_key_pair;
use crate::method_call_closure_dispatch::invoke_with_this;
use crate::method_value::symbol_static::well_known_singleton;
use crate::nanbox::{AnyValue, as_void_ptr};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `AnySlotTag::Heap` in the member-pair protocol.
const TAG_HEAP: u64 = 4;

/// §6.1.5.1 well-known index of `@@hasInstance` — the table is
/// alphabetical by property name (torajs-str `WELL_KNOWN_DESCS`).
const WK_HAS_INSTANCE: i64 = 3;

/// Closure-env boxed-entry slot — `method_value::CLOSURE_BOXED_ENTRY_OFF`
/// mirror (the universal closure layout).
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;

/// §13.10.2 `v instanceof target` for a runtime-resolved target.
///
/// # Safety
/// `v` and `target` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_instanceof_dynamic(v: AnyValue, target: AnyValue) -> bool {
    unsafe {
        // Step 1 — non-objects have no property face to consult.
        if !is_heap_object(target) {
            __torajs_throw_type_error(c"Right-hand side of 'instanceof' is not an object".as_ptr());
            return false;
        }
        // Step 2 — GetMethod(target, @@hasInstance).
        let (tag, value) = symbol_key_pair(target, well_known_singleton(WK_HAS_INSTANCE));
        if tag == TAG_HEAP && value != 0 {
            return call_has_instance(v, target, value as *mut c_void);
        }
        // Steps 4-5 — no handler: the target must itself be callable,
        // and then the answer is the ordinary prototype walk.
        let cell = as_void_ptr(target);
        if cell_tag(cell) == Tag::Closure as u16 {
            return crate::construct::__torajs_instanceof_fn_value(v, cell);
        }
        // A CLASS object is callable in the spec sense — `typeof C`
        // answers "function" — but tr models it as a dynobj, so the
        // closure walk above cannot read its `.prototype`. Reached
        // whenever a class is used as a VALUE (`const T: any = C`,
        // `{ cls: C }`, a call returning one) rather than named, which
        // is exactly what a general right-hand side allows.
        if let Some(r) = crate::construct::class_ctor_has_instance(v, cell) {
            return r;
        }
        __torajs_throw_type_error(c"Right-hand side of 'instanceof' is not callable".as_ptr());
        false
    }
}

/// Step 3 — invoke the handler with `this` = target and the single
/// argument V, then ToBoolean the (owned) result. A handler that is
/// not callable is the step-2 GetMethod TypeError, not a silent miss.
unsafe fn call_has_instance(v: AnyValue, target: AnyValue, cell: *mut c_void) -> bool {
    unsafe {
        if cell_tag(cell) != Tag::Closure as u16 {
            __torajs_throw_type_error(c"Symbol.hasInstance handler is not callable".as_ptr());
            return false;
        }
        let entry = *(cell.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == 0 {
            __torajs_throw_type_error(c"Symbol.hasInstance handler is not callable".as_ptr());
            return false;
        }
        let argv = [v];
        let raw = invoke_with_this(cell, entry, target, argv.as_ptr(), 1);
        // The dispatch sentinel means the adapter declined; a body
        // that threw leaves the pending throw for the call site.
        if raw == crate::method_call::ANY_METHOD_NO_SUCH {
            return false;
        }
        let b = crate::nanbox_ffi::__torajs_anyv_to_bool(raw);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(raw);
        b
    }
}

/// The heap header's type tag; caller guarantees a live cell.
unsafe fn cell_tag(cell: *mut c_void) -> u16 {
    unsafe { (cell.cast::<u8>().add(4) as *const u16).read() }
}
