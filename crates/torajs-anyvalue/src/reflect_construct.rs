//! `__torajs_reflect_construct` — §28.1.2 Reflect.construct(target,
//! argumentsList[, newTarget]) (rotation 293).
//!
//! The runtime-construct kernel's reflective face: IsConstructor
//! gates on target AND newTarget (§28.1.2 steps 1/3 — the lowering
//! passes target again for the two-argument form), then an
//! unconditional CreateListFromArrayLike (§28.1.2 step 4 — nullish
//! is a TypeError here, same posture as `Reflect.apply`), then the
//! same factory-adapter construct `__torajs_anyv_construct` runs.
//!
//! When newTarget differs from target, the constructed object's
//! [[Prototype]] comes from `Get(newTarget, "prototype")` (§9.1.13
//! OrdinaryCreateFromConstructor reads newTarget, not target) — the
//! factory adapter wires target's prototype, so the kernel re-wires
//! afterward through the ordinary [[SetPrototypeOf]]. A non-object
//! `prototype` answer keeps the factory wiring (%Object.prototype%
//! fallback is what the factory already gave). Recorded boundary: a
//! fixed-layout (struct-repr) instance refuses [[SetPrototypeOf]]
//! loudly, so the differing-newTarget form raises that TypeError
//! today instead of answering a mis-wired object.
//!
//! The list walk is the cold-path copy of
//! [`crate::method_call_closure_apply_like::apply_list`]'s two lanes
//! (dense Arr / array-like [[Get]]) collecting into a Vec — the
//! apply kernel's stack-buffer hot shape stays untouched.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::construct::{ctor_entry, is_constructor};
use crate::iter_any_result::iter_result_get;
use crate::method_call_closure_dispatch::invoke_boxed;
use crate::method_value::ctor::ctor_tag_of_cell;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::to_primitive::is_object_value;

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-arr — kind-aware boxed element read (owned +1).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-meta — ordinary [[SetPrototypeOf]] over an any pair.
    fn __torajs_anyv_set_prototype_of(obj: u64, proto: u64);
    fn __torajs_anyv_get_proto_of_any(obj: u64) -> u64;
    /// torajs-meta — §10.1.13 step 4's intrinsic default for a class
    /// constructor (borrowed; 0 = keep what the target built).
    fn __torajs_class_default_proto(ctor: u64) -> u64;
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    fn __torajs_dynobj_alloc() -> *mut c_void;
}

/// Arr cell length slot — mirror of torajs-arr `layout::ARR_LEN_OFF`.
const ARR_LEN_OFF: usize = 8;

/// §7.3.18 CreateListFromArrayLike → owned boxed elements. `None`
/// means a TypeError / abrupt [[Get]] is already recorded pending
/// (collected elements released).
unsafe fn collect_list(list: AnyValue) -> Option<Vec<u64>> {
    unsafe {
        let ptr = if is_cell(list) {
            as_void_ptr(list)
        } else {
            core::ptr::null_mut()
        };
        let tag = if ptr.is_null() {
            0
        } else {
            (ptr.cast::<u8>().add(4) as *const u16).read()
        };
        if ptr.is_null()
            || tag == Tag::Str as u16
            || tag == Tag::Symbol as u16
            || tag == Tag::BigInt as u16
        {
            // §7.3.18 step 2 — a primitive argumentsList (heap-celled
            // primitives included) is a TypeError.
            __torajs_throw_type_error(
                c"Reflect.construct requires the second argument be an object".as_ptr(),
            );
            return None;
        }
        if tag == Tag::Arr as u16 {
            let n = *(ptr.cast::<u8>().add(ARR_LEN_OFF) as *const u64) as usize;
            return Some(
                (0..n)
                    .map(|i| __torajs_arr_index_get(ptr, i as i64))
                    .collect(),
            );
        }
        // Array-like: ToLength(Get(obj, "length")), then per-index
        // Get — accessor-aware, abrupt completions forward.
        let len_v = iter_result_get(list, ptr, b"length")?;
        let (t, p) = (
            crate::__torajs_anyv_unbox_tag(len_v),
            crate::__torajs_anyv_unbox_value(len_v),
        );
        let n_f = crate::coerce::any_to_number(t, p);
        // any_to_number only borrows and anyv_rc_dec no-ops on the
        // ShortStr immediate — release the materialized rc=1 Str
        // (546-02 M1 family).
        if crate::nanbox::is_short_str(len_v) && p != 0 {
            crate::__torajs_value_drop_heap(p as *mut c_void);
        }
        __torajs_anyv_rc_dec(len_v);
        if __torajs_throw_check() != 0 {
            return None;
        }
        let n = if n_f.is_nan() || n_f <= 0.0 {
            0
        } else {
            n_f.trunc().min(9007199254740991.0) as usize
        };
        let mut boxed: Vec<u64> = Vec::with_capacity(n);
        for i in 0..n {
            let key = i.to_string();
            let Some(v) = iter_result_get(list, ptr, key.as_bytes()) else {
                for b in &boxed {
                    __torajs_anyv_rc_dec(*b);
                }
                return None;
            };
            boxed.push(v);
        }
        Some(boxed)
    }
}

/// See module doc. Answers the constructed object, owned; on a gate
/// TypeError the pending throw records and `undefined` rides back.
///
/// # Safety
/// Cell operands are valid heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_reflect_construct(
    target: AnyValue,
    list: AnyValue,
    new_target: AnyValue,
) -> AnyValue {
    unsafe {
        // §28.1.2 step 1 — IsConstructor(target).
        if !is_constructor(target) {
            __torajs_throw_type_error(
                c"Reflect.construct requires the first argument be a constructor".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // §28.1.2 step 3 — IsConstructor(newTarget). The lowering
        // hands target back for the two-argument form.
        if !is_constructor(new_target) {
            __torajs_throw_type_error(
                c"Reflect.construct requires the third argument be a constructor".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // §28.1.2 step 4 — CreateListFromArrayLike, unconditional
        // (nullish is not an empty list here).
        if is_undefined(list) || is_null(list) {
            __torajs_throw_type_error(
                c"Reflect.construct requires the second argument be an object".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let Some(boxed) = collect_list(list) else {
            return VALUE_UNDEFINED;
        };
        let obj = construct_with_new_target(target, boxed.as_ptr(), boxed.len() as i64, new_target);
        for b in boxed {
            __torajs_anyv_rc_dec(b);
        }
        obj
    }
}

/// `target.[[Construct]](args, newTarget)` — an OWNED result, or
/// `undefined` with the pending throw recorded. A Proxy target takes
/// newTarget down its own trap; an ordinary constructor builds its
/// object and then, when newTarget differs, takes §9.1.13
/// GetPrototypeFromConstructor(newTarget) as the object's prototype.
///
/// # Safety
/// `target` and `new_target` are live constructors; `argv` holds
/// `argc` live AnyValues.
pub(crate) unsafe fn construct_with_new_target(
    target: AnyValue,
    argv: *const u64,
    argc: i64,
    new_target: AnyValue,
) -> AnyValue {
    unsafe {
        if crate::proxy::is_proxy(target) {
            return crate::proxy_callable::construct_proxy(target, argv, argc, new_target);
        }
        // A plain-fn closure constructs per §10.2.2 through the same
        // channel `new <value>` takes (B1 construct_plain_fn — fresh
        // `this` off the callee's `.prototype`); this path predates
        // B1 and only knew the class-factory registry.
        let target_tag = (as_void_ptr(target).cast::<u8>().add(4) as *const u16).read();
        let obj = if new_target != target
            && ctor_tag_of_cell(as_void_ptr(target).cast_const()) == Some(OBJECT_PROTO_TAG)
        {
            // §20.1.1.1 step 1 — Object under a foreign newTarget is
            // OrdinaryCreateFromConstructor: a fresh object, the value
            // argument plays no part (returning it would re-parent the
            // caller's own object below).
            crate::nanbox_encode::__torajs_anyv_box_from_pair(4, __torajs_dynobj_alloc() as i64)
        } else if target_tag == Tag::Closure as u16 {
            crate::construct::construct_with_nt(target, argv, argc, new_target)
        } else {
            let entry = ctor_entry(as_void_ptr(target) as u64);
            if entry == 0 {
                __torajs_throw_type_error(
                    c"this constructor cannot be reached through a runtime value yet".as_ptr(),
                );
                return VALUE_UNDEFINED;
            }
            invoke_boxed(core::ptr::null_mut(), entry, argv, argc)
        };
        if __torajs_throw_check() != 0 {
            return obj;
        }
        // §9.1.13 — [[Prototype]] from newTarget when it differs from
        // what the bottom of a bound chain was constructed with: the
        // layers swap a newTarget naming one of them for its target
        // (§10.4.1.2), and that construction already chose.
        let (bottom, resolved_nt) = if target_tag == Tag::Closure as u16 {
            let (b, nt, _) = crate::construct_bound::resolve_bound(target, new_target);
            (b, nt)
        } else {
            (target, new_target)
        };
        if resolved_nt != bottom && is_cell(obj) {
            if let Some(proto) =
                iter_result_get(resolved_nt, as_void_ptr(resolved_nt), b"prototype")
            {
                // Step 4 — a non-object `prototype` (a string and a symbol
                // are cells too) falls back to the realm intrinsic the
                // constructor names: its own prototype for an intrinsic,
                // %Object.prototype% under a chain of user classes.
                let want = if is_object_value(proto) {
                    proto
                } else {
                    default_proto(target)
                };
                // A proxy newTarget over the target usually answers the
                // prototype the object already has; re-parenting to the
                // same value is a no-op.
                if want != 0 {
                    let current = __torajs_anyv_get_proto_of_any(obj);
                    if current != want {
                        __torajs_anyv_set_prototype_of(obj, want);
                    }
                    __torajs_anyv_rc_dec(current);
                }
                __torajs_anyv_rc_dec(proto);
            } else {
                // Abrupt Get(newTarget, "prototype") — release the
                // constructed object and forward the throw.
                __torajs_anyv_rc_dec(obj);
                return VALUE_UNDEFINED;
            }
        }
        obj
    }
}

/// `%Object.prototype%`'s builtin-proto tag, which is also the
/// interned `Object` constructor cell's slot.
const OBJECT_PROTO_TAG: i64 = 1;

/// §10.1.13 step 4 — the intrinsic default prototype of `target`,
/// BORROWED, for a newTarget whose `prototype` is no object; 0 keeps
/// what the target built. An ordinary function's is
/// %Object.prototype% (§10.2.2 step 5.a); a builtin constructor built
/// its own intrinsic already; a class asks the class registry. A bound
/// function has no intrinsic of its own: §10.4.1.2 constructs its
/// target with the same newTarget, so the fallback is that of the
/// first target down the chain that is not bound, and a Proxy there
/// chose through its own [[Construct]].
///
/// # Safety
/// `target` is a live constructor.
unsafe fn default_proto(target: AnyValue) -> u64 {
    let mut cell = as_void_ptr(target);
    unsafe {
        while let Some((1, t, _)) = crate::construct_bound::bound_closure_meta(cell) {
            cell = t as *mut c_void;
        }
        if crate::proxy::is_proxy(crate::nanbox_encode::__torajs_anyv_box_pointer(cell)) {
            return 0;
        }
        if (cell.cast::<u8>().add(4) as *const u16).read() != Tag::Closure as u16 {
            return __torajs_class_default_proto(crate::nanbox_encode::__torajs_anyv_box_pointer(
                cell,
            ));
        }
        if ctor_tag_of_cell(cell.cast_const()).is_some()
            || crate::method_bind::bound_cell_meta(cell).is_some()
        {
            return 0;
        }
        __torajs_get_builtin_prototype(OBJECT_PROTO_TAG) as u64
    }
}
