//! `Tag::Closure` receiver arm for `__torajs_any_method_call`
//! (chunk 710) — `Function.prototype.call` / `apply` on closure
//! values that travel through the `any` world.
//!
//! The thisArg rides the closure's receiver channel: a recv-first
//! closure (RFC 20260717-objlit-anylane-recv — an any-lane literal
//! method whose body says `this`) takes it in argv[0], every other
//! closure body cannot reference `this` (class methods dispatch
//! through the vtable tier, never as bare closure cells) so the
//! thisArg drops:
//!
//! - `f.call(thisArg, a, b)` → `invoke_with_this(env, entry, thisArg, argv[1..])`.
//! - `f.apply(thisArg, list)` → the list unpacks per
//!   CreateListFromArrayLike (ES §7.3.19): `undefined` / `null` is
//!   an empty list; an `Arr` cell reads element-by-element through
//!   the kind-aware `__torajs_arr_index_get` (owned boxes, released
//!   after the call); anything else is a catchable TypeError.
//! - An expando property (chunk 529's lazy props bag) shadows the
//!   builtin per ES own-property order — `f.call = …` wins — and
//!   dispatches through the dynobj arm.
//! - A reified builtin method cell (chunk 711, `method_value`)
//!   short-circuits: `.call` / `.apply` re-dispatch the ORIGINAL
//!   method id with the thisArg as the receiver — `f.call(s)` where
//!   `f = s.toUpperCase` runs the string method on `s`. No receiver
//!   slot travels (a grow-relocating method reached through `.call`
//!   cannot write the caller's variable back — recorded boundary).
//! - `bind` (chunk 714) mints a bound-function cell (`method_bind`).
//! - A closure without a boxed dual entry cannot dispatch
//!   dynamically ([`not_callable`], same as the bare any-call lane).
//! - `toString` (RFC 20260719-fn-tostring-source B4) answers the
//!   type-erased source text from the fn-addr registry; reified
//!   builtin cells and source-less rows mint the JSC native form.
//! - Every other method id floats the no-such sentinel.
//!
//! Argument ledger: identical to the dispatcher — argv slots are
//! BORROWED; the apply unpacking's element boxes are this arm's own
//! temps and drop before returning.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_APPLY, ANY_METHOD_BIND, ANY_METHOD_CALL, ANY_METHOD_TO_LOCALE_STRING,
    ANY_METHOD_TO_STRING,
};

use crate::method_call::{closure_cell_entry, invoke_with_this, method_no_such, not_callable};
// The apply tail (CreateListFromArrayLike) lives beside this arm;
// re-exported so reflect_apply keeps its import face.
pub(crate) use crate::method_call_closure_apply_like::apply_list;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    /// torajs-fnname — Function.prototype.toString kernels (RFC
    /// 20260719-fn-tostring-source B4): erased-source mint keyed on
    /// the registry fn_addr, and the named JSC native form for
    /// out-of-registry entries (reified builtin method cells).
    fn __torajs_fn_source_str(fn_addr: u64) -> *mut u8;
    fn __torajs_fn_native_form_str(name_ptr: *const u8, name_len: u32) -> *mut u8;
    /// torajs-dynobj — own-property probe (5 = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-str — release an owned Str temp.
    fn __torajs_str_drop(s: *mut c_void);
}

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const CLOSURE_PROPS_OFF: usize = 24;

/// `Tag::Closure` arm — see module doc.
///
/// # Safety
/// `ptr` is a valid `Tag::Closure` heap pointer; `argv` points at
/// `argc` AnyValue slots the caller keeps alive across the call;
/// `name_str` is NULL or a live Str cell.
pub(crate) unsafe fn closure_method(
    ptr: *mut c_void,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // ES own-property order: an expando shadows the builtin.
        let props = *(ptr.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64) as *const c_void;
        if !props.is_null()
            && !name_str.is_null()
            && __torajs_dynobj_get_tag(props, name_str as *const c_void) != 5
        {
            // A closure stored here takes the FUNCTION as its `this`,
            // not the bag the property happens to live in.
            if let Some(r) = crate::method_call_closure_expando::expando_this_is_the_function(
                ptr, props, name_str, argv, argc,
            ) {
                return r;
            }
            // NULL recv_slot — the props side-table cell is not the
            // caller's variable (no relocation writeback target).
            return crate::method_call_dynobj::dynobj_method(
                props as *mut c_void,
                mid,
                name_str,
                core::ptr::null_mut(),
                argv,
                argc,
            );
        }
        // RFC 20260730 blade 2 — Function-subclass method probe: on
        // the spec chain C.prototype sits between own properties and
        // Function.prototype, so a class method (including an
        // override of call/apply/bind) resolves here. Plain closures
        // pay one predicted-clear branch on an already-loaded header
        // word.
        if !name_str.is_null() {
            let flags = (ptr.cast::<u8>().add(6) as *const u16).read();
            if flags & torajs_rc::FLAG_SUBCLASSED != 0
                && let Some(r) =
                    crate::method_call_subclass::subclass_method(ptr, name_str, argv, argc)
            {
                return r;
            }
        }
        match mid {
            m if m == ANY_METHOD_CALL => {
                let Some(target) = call_target(ptr) else {
                    return not_callable();
                };
                let this_arg = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
                if argc <= 1 {
                    return dispatch(&target, this_arg, argv, 0);
                }
                dispatch(&target, this_arg, argv.add(1), argc - 1)
            }
            m if m == ANY_METHOD_APPLY => {
                let Some(target) = call_target(ptr) else {
                    return not_callable();
                };
                let this_arg = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
                let list = if argc >= 2 {
                    *argv.add(1)
                } else {
                    VALUE_UNDEFINED
                };
                apply_list(&target, this_arg, list)
            }
            m if m == ANY_METHOD_BIND => crate::method_bind::bind_cell(ptr, argv, argc),
            // RFC 20260719-fn-tostring-source B4 —
            // Function.prototype.toString answers the type-erased
            // source text from the fn-addr registry; rows with no
            // recorded source (bound wrappers / reified builtin
            // cells whose fn_addr is the throwing native entry)
            // fall to the JSC native form inside the kernel.
            // §20.2.3.5 — toLocaleString is toString's answer.
            m if m == ANY_METHOD_TO_STRING || m == ANY_METHOD_TO_LOCALE_STRING => {
                if let Some(name) = crate::method_value::builtin_method_name(ptr) {
                    // A reified builtin method cell is never in the
                    // registry — mint the named native form directly.
                    return crate::nanbox::box_void_ptr(__torajs_fn_native_form_str(
                        name.as_ptr(),
                        name.len() as u32,
                    ) as *mut c_void);
                }
                // B6c — a class-method face resolves its adapter's
                // registry row (the erased method-shorthand source).
                let fn_addr = crate::method_value_class::registry_addr(ptr);
                crate::nanbox::box_void_ptr(__torajs_fn_source_str(fn_addr) as *mut c_void)
            }
            _ => {
                // RFC 20260721 刀 3 — dynamic-key invoke of a ctor
                // table static (`(Date as any)["now"]()`): resolve
                // the interned ns-static cell and call through its
                // dispatcher entry (this-insensitive statics;
                // this-sensitive arms throw inside the dispatcher).
                if !name_str.is_null()
                    && let Some(cell) = crate::method_value::ctor_static_cell(
                        ptr as *const c_void,
                        name_str as *const c_void,
                    )
                    && let Some(target) = call_target(cell as *mut c_void)
                {
                    return dispatch(&target, VALUE_UNDEFINED, argv, argc);
                }
                // 405-01 substrate — a re-parented function value
                // resolves inherited methods through its user
                // [[Prototype]] chain (the ES5 extends lane's
                // `Object.setPrototypeOf(D, P)` static face).
                if !name_str.is_null()
                    && let Some(r) = crate::method_call_closure_expando::proto_chain_method(
                        ptr, mid, name_str, argv, argc,
                    )
                {
                    return r;
                }
                method_no_such()
            }
        }
    }
}

/// What `.call` / `.apply` re-invokes — a closure's boxed dual
/// entry (the thisArg rides the receiver channel: a recv-first
/// closure takes it in argv[0], a plain closure drops it — RFC
/// 20260717-objlit-anylane-recv knife 2d), or a reified builtin
/// method's original id re-dispatched with the thisArg as the
/// receiver (chunk 711).
pub(crate) enum CallTarget {
    Boxed(*mut c_void, u64),
    /// (mid, family) — the mint family picks the family-generic
    /// lane: §22.1.3 ToString(this) for a String-prototype cell,
    /// the §23.1.3.1 wrapper-seed concat for an Array-prototype
    /// cell on a primitive receiver.
    Builtin(i64, i64),
    /// A reified class method / accessor face (RFC
    /// 20260718-accessor-reify 刀 2) — the carried adapter invokes
    /// with the thisArg in the env slot.
    ClassAdapter(u64),
}

/// Classify the receiver closure cell.
pub(crate) unsafe fn call_target(ptr: *mut c_void) -> Option<CallTarget> {
    unsafe {
        if let Some(target_mid) = crate::method_value::builtin_method_mid(ptr) {
            let fam = crate::method_value::builtin_method_family(ptr);
            return Some(CallTarget::Builtin(target_mid, fam));
        }
        // Blade 3 (RFC 20260804-method-rebind-generic-body) — a
        // METHOD face routes through the Boxed lane so
        // `invoke_with_this`'s receiver guard runs (the face cell in
        // the env slot carries the class tag + twin; the entry is
        // the recognizer sentinel it re-derives the adapter from).
        // Accessor faces keep the direct ClassAdapter invoke — their
        // re-bind guard is the recorded RFC follow-up.
        if crate::method_value_class::class_method_face_adapter(ptr).is_some() {
            return closure_cell_entry(ptr).map(|(env, entry)| CallTarget::Boxed(env, entry));
        }
        if let Some(adapter) = crate::method_value_class::class_method_adapter(ptr) {
            return Some(CallTarget::ClassAdapter(adapter));
        }
        closure_cell_entry(ptr).map(|(env, entry)| CallTarget::Boxed(env, entry))
    }
}

/// Invoke the target with an unpacked argument window. The builtin
/// re-dispatch passes no name bytes and no receiver slot (a grow-
/// relocating method reached through `.call` cannot write the
/// caller's variable back — recorded boundary).
pub(crate) unsafe fn dispatch(
    target: &CallTarget,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        match target {
            CallTarget::Boxed(env, entry) => invoke_with_this(*env, *entry, this_arg, argv, argc),
            CallTarget::ClassAdapter(adapter) => {
                crate::method_value_class::__torajs_class_face_invoke(
                    *adapter, this_arg, argv, argc,
                )
            }
            CallTarget::Builtin(mid, fam) => {
                // A tag-15 cell (%Iterator.prototype%'s helpers) is
                // STRICT about its receiver — §27.1.4.x step 1 is a
                // TypeError on a non-Object `this`, never a ToObject
                // wrap (the redispatch below would wrapper-seed a
                // primitive and read its `next` — test262 plants a
                // poisoned `Number.prototype.next` getter to catch
                // exactly that). An Object rides the shared kernel,
                // mirror of the dynobj-chain leg.
                if *fam == 15 && crate::iter_helper::iter_proto_owns_mid(*mid) {
                    if !crate::iter_zip_shared::av_is_object(this_arg) {
                        __torajs_throw_type_error(
                            c"Iterator helper called on a non-object".as_ptr(),
                        );
                        return crate::nanbox::VALUE_UNDEFINED;
                    }
                    return crate::iter_helper::try_helper_chain(
                        crate::nanbox::as_void_ptr(this_arg) as *mut c_void,
                        *mid,
                        argv as *const AnyValue,
                        argc,
                    )
                    .unwrap_or_else(|| crate::method_call::method_no_such());
                }
                if let Some(out) = generic_builtin_this(*mid, this_arg, argv, argc, *fam) {
                    return out;
                }
                crate::method_call::any_method_redispatch(this_arg, *mid, argv, argc)
            }
        }
    }
}

// The borrowed-builtin generic lanes (family-generic gate + the
// §22.1.3 ToString(this) coerce) live in the sibling; re-exported so
// every station keeps its `method_call_closure::` path.
pub(crate) use crate::method_call_closure_generic::generic_builtin_this;
