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
    ANY_METHOD_APPLY, ANY_METHOD_AT, ANY_METHOD_BIND, ANY_METHOD_CALL, ANY_METHOD_CONCAT,
    ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF, ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_SLICE,
    ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF, Tag,
};

use crate::method_call::{closure_cell_entry, invoke_with_this, method_no_such, not_callable};
// The apply tail (CreateListFromArrayLike) lives beside this arm;
// re-exported so reflect_apply keeps its import face.
pub(crate) use crate::method_call_closure_apply_like::apply_list;
use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_null,
    is_short_str, is_undefined,
};

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
                if let Some(out) = generic_builtin_this(*mid, this_arg, argv, argc, *fam) {
                    return out;
                }
                crate::method_call::any_method_redispatch(this_arg, *mid, argv, argc)
            }
        }
    }
}

/// §22.1.3 "the String.prototype methods are generic" — a reified
/// String.prototype method reached through `.call` / `.apply` with a
/// non-string thisArg runs ToString(this) (full OrdinaryToPrimitive,
/// observable toString→valueOf order; a double-object receiver
/// leaves a pending TypeError for the caller's throw check) and
/// dispatches the Str arm on the coerced temp. Excluded from the
/// coerce, staying on the ordinary lane:
/// - `toString` / `valueOf` (thisStringValue §22.1.3.28/.35) and
///   `toLocaleString` — a non-String receiver is a TypeError there;
/// - unless `str_family` (the cell was minted for the String
///   prototype — RFC 20260721 G4 per-family cells), mids SHARED
///   with the Array surface (at / concat / includes / indexOf /
///   lastIndexOf / slice): a family-less cell's
///   `Array.prototype.indexOf.call(arrayLike)` re-dispatch must
///   reach the array-like generic arm;
/// - string-shaped and nullish receivers (identity fast paths /
///   RequireObjectCoercible throw).
/// The family-generic re-dispatch gate every borrowed-builtin
/// station runs before the ordinary receiver-arm redispatch:
///
/// - An Array-prototype-minted `concat` on a primitive receiver
///   seeds `ToObject(this)` per §23.1.3.1 (the receiver arms only
///   know their own-family concat — string concat / no-such);
/// - a String-prototype-minted cell runs the §22.1.3 generic
///   ToString(this) lane ([`generic_str_this`]).
pub(crate) unsafe fn generic_builtin_this(
    mid: i64,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
    fam: i64,
) -> Option<AnyValue> {
    if fam == crate::method_value::family::ARR_PROTO_FAMILY
        && mid == ANY_METHOD_CONCAT
        && is_prim_shaped(this_arg)
    {
        return Some(unsafe {
            crate::method_call_arraylike_concat::prim_method(this_arg, argv, argc)
        });
    }
    // §21.1.3 thisNumberValue / §20.3.3 thisBooleanValue — a Number-
    // or Boolean-prototype-minted toString / valueOf borrowed onto a
    // receiver of the wrong brand is a TypeError (rotation 204,
    // mirror of the String family's thisStringValue gate below;
    // toString is NOT generic for these prototypes).
    if matches!(mid, ANY_METHOD_TO_STRING | ANY_METHOD_VALUE_OF) {
        let wrong_brand = (fam == crate::method_value::NUM_PROTO_FAMILY
            && !is_number_shaped(this_arg))
            || (fam == crate::method_value::BOOL_PROTO_FAMILY && !is_boolean_shaped(this_arg));
        if wrong_brand {
            unsafe {
                __torajs_throw_type_error(
                    c"builtin prototype method requires |this| to match its brand".as_ptr(),
                );
            }
            return Some(VALUE_UNDEFINED);
        }
    }
    unsafe {
        generic_str_this(
            mid,
            this_arg,
            argv,
            argc,
            fam == crate::method_value::STR_PROTO_FAMILY,
        )
    }
}

/// thisNumberValue shape — a number immediate or a Number wrapper
/// (whose thisNumberValue is its [[NumberData]]).
fn is_number_shaped(v: AnyValue) -> bool {
    if is_int32(v) || is_double(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::NumberWrapper as u16
}

/// thisBooleanValue shape — a bool immediate or a Boolean wrapper.
fn is_boolean_shaped(v: AnyValue) -> bool {
    if is_bool(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::BooleanWrapper as u16
}

/// A primitive shape whose ToObject mints a fresh wrapper — the
/// receivers whose own dispatch arm would answer the WRONG concat
/// (string concat on Str shapes, no-such on bool/number). Heap
/// receivers (wrapper objects included) already ride the cell arm's
/// seeded concat gate.
fn is_prim_shaped(v: AnyValue) -> bool {
    if is_bool(v) || is_int32(v) || is_double(v) || is_short_str(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::Str as u16
}

/// §6.1.4-shape probe for the thisStringValue gate — a ShortStr
/// immediate, a Str cell, or a String wrapper object (whose
/// thisStringValue is its [[StringData]]).
fn is_string_shaped(v: AnyValue) -> bool {
    if is_short_str(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::Str as u16 || tag == Tag::StringWrapper as u16
}

pub(crate) unsafe fn generic_str_this(
    mid: i64,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
    str_family: bool,
) -> Option<AnyValue> {
    if !crate::method_support::str_supports(mid) {
        return None;
    }
    if matches!(
        mid,
        ANY_METHOD_TO_STRING | ANY_METHOD_VALUE_OF | ANY_METHOD_TO_LOCALE_STRING
    ) {
        // §22.1.3.28/.35 thisStringValue — a String-prototype-minted
        // toString / valueOf borrowed onto a non-string receiver is
        // a TypeError (RFC 20260721 G5); string shapes ride the
        // ordinary re-dispatch identity lane. toLocaleString is the
        // inherited generic (§20.1.4.6), never brand-checked here.
        if str_family && mid != ANY_METHOD_TO_LOCALE_STRING && !is_string_shaped(this_arg) {
            unsafe {
                __torajs_throw_type_error(
                    c"String.prototype method requires that |this| be a String".as_ptr(),
                );
            }
            return Some(VALUE_UNDEFINED);
        }
        return None;
    }
    if !str_family
        && matches!(
            mid,
            ANY_METHOD_AT
                | ANY_METHOD_CONCAT
                | ANY_METHOD_INCLUDES
                | ANY_METHOD_INDEX_OF
                | ANY_METHOD_LAST_INDEX_OF
                | ANY_METHOD_SLICE
        )
    {
        return None;
    }
    if is_undefined(this_arg) || is_null(this_arg) || is_short_str(this_arg) {
        return None;
    }
    if is_cell(this_arg) {
        let ptr = as_void_ptr(this_arg);
        if unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() } == Tag::Str as u16 {
            return None;
        }
    }
    // §22.1.3.14 / §22.1.3.20 step 2.b precedes step 3's ToString(O)
    // — a non-global RegExp search argument disqualifies the call
    // before the receiver's user `toString` may run.
    if unsafe { crate::method_call_str::reject_non_global_regex_search(mid, argv, argc) } {
        return Some(VALUE_UNDEFINED);
    }
    unsafe {
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(this_arg);
        let out = crate::method_call_str::str_method(s as *mut u8, mid, argv, argc);
        __torajs_str_drop(s);
        Some(out)
    }
}
