//! §7.3.22 OrdinaryHasInstance when the target arrived as a VALUE —
//! `o instanceof C` where `C` is a binding the compile-time class
//! lane could not fold, so only the running program knows what it is.
//!
//! Split out of [`crate::construct`] under the 500-line file cap.
//! Verbatim move. The seam is the question each half answers:
//! `construct` is about `new C(...)` — making an object — while these
//! four are about whether an object is already on C's prototype
//! chain. `instanceof_dynamic` routes to both and is unchanged.

use core::ffi::c_void;

use torajs_rc::{HeapHeader, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
    fn __torajs_throw_check() -> i64;
}

/// §7.3.22 OrdinaryHasInstance(C, O) where C is a plain-fn VALUE —
/// `o instanceof C` with a fn-expr binding behind the name (no class
/// resolves, so the compile-time tag dispatch has nothing to fold).
/// O must be an Object (step 3, non-objects answer false); P is
/// C's canonical `.prototype` (the same fnprops cell the construct
/// kernel links, so `Array.from.call(C, …)` products match); the
/// walk follows [[Prototype]] links (the proto getter answers
/// borrows / immortal singletons — nothing to release) comparing
/// cell identity per step 7. A callable with no object-shaped
/// `.prototype` throws (step 5's Get + step 6's Type check).
///
/// # Safety
/// `v` is a valid AnyValue; `cell` is a live `Tag::Closure` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_instanceof_fn_value(v: u64, cell: *mut c_void) -> bool {
    unsafe {
        // §7.3.22 step 2 — a bound function has no `.prototype` of
        // its own: the answer is InstanceofOperator(O, BC) on the
        // target it was bound from.
        if let Some((1, target, _)) = crate::method_bind::bound_cell_meta(cell) {
            return bound_has_instance(v, target);
        }
        // §7.3.22 step 3 — "If O is not an Object, return false".
        // This precedes the Get, so a callable with a broken
        // `.prototype` is not even consulted when the left side is a
        // primitive: `1 instanceof f` is false, never a TypeError.
        // `proto_chain_reaches` asks the same question at its own
        // entry; hoisting it is what keeps step 3 ahead of step 5
        // now that step 5 can raise.
        if !is_heap_object(v) {
            return false;
        }
        // §7.3.22 steps 4-5 — `Get(C, "prototype")` and then "If P is
        // not an Object, throw a TypeError exception". The Get half
        // was here; the type check was not, because the pair's TAG
        // was dropped on the floor. `function f(){}; f.prototype =
        // true; ({}) instanceof f` therefore walked the chain looking
        // for the cell at address 1, never found it, and answered
        // false where the spec raises.
        //
        // The question is the same one `Reflect.setPrototypeOf`'s
        // proto gate asks, and `av_is_object` is where it is named: a
        // heap cell is not enough, because a Str, a Symbol or a
        // BigInt is a cell here and a primitive to the spec.
        let Some((ptag, pval)) = crate::closure_proto::fn_prototype_pair(cell) else {
            __torajs_throw_type_error(
                c"Function has non-object prototype in instanceof check".as_ptr(),
            );
            return false;
        };
        let p = crate::nanbox_encode::__torajs_anyv_box_from_pair(ptag as i64, pval as i64);
        if !crate::iter_zip_shared::av_is_object(p) {
            __torajs_throw_type_error(
                c"Function has non-object prototype in instanceof check".as_ptr(),
            );
            return false;
        }
        proto_chain_reaches(v, pval)
    }
}

/// §7.3.22 step 2 — InstanceofOperator(O, BC) over the
/// [[BoundTargetFunction]] `target`, for every bound layer below in one
/// loop rather than one native frame per layer. Each layer is asked
/// for @@hasInstance once, as §13.10.2 step 2 does. The default
/// handler (§20.2.3.6) and an absent one both run OrdinaryHasInstance
/// on that layer — for a bound layer that is this step again, one
/// layer down; for any other function it is the ordinary walk. Any
/// other handler is called with the layer as `this` (step 3).
///
/// # Safety
/// `v` is a valid AnyValue; `target` is a live kind-1 bound target.
// out of line and for size: the ordinary walk above stays as it was
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn bound_has_instance(v: AnyValue, target: u64) -> bool {
    unsafe {
        let mut c = crate::nanbox_encode::__torajs_anyv_box_pointer(target as *mut c_void);
        loop {
            if let Some(b) = layer_has_instance(v, c) {
                return b;
            }
            match crate::construct_bound::bound_closure_meta(as_void_ptr(c)) {
                Some((1, t, _)) => {
                    c = crate::nanbox_encode::__torajs_anyv_box_pointer(t as *mut c_void)
                }
                _ => return crate::instanceof_dynamic::ordinary_has_instance(v, c, false),
            }
        }
    }
}

/// One layer of [`bound_has_instance`]: §13.10.2 steps 2-3 over `c`.
/// None for the default handler or none, which leave
/// OrdinaryHasInstance to run on `c`.
///
/// # Safety
/// As [`bound_has_instance`]; `c` is a live function.
#[inline(always)]
unsafe fn layer_has_instance(v: AnyValue, c: AnyValue) -> Option<bool> {
    use crate::instanceof_dynamic::{TAG_HEAP, TAG_NULL, TAG_UNDEF, WK_HAS_INSTANCE};
    unsafe {
        let handler = crate::member_get_symbol::symbol_key_get(
            c,
            crate::method_value::symbol_static::well_known_singleton(WK_HAS_INSTANCE),
        );
        let (tag, value) = handler.pair();
        // one exit, so the handler is released in one place
        if __torajs_throw_check() != 0 {
            Some(false)
        } else if tag == TAG_HEAP && value != 0 {
            if crate::method_value::builtin_method_mid(value as *mut c_void)
                == Some(torajs_rc::ANY_METHOD_FN_HAS_INSTANCE)
            {
                None
            } else {
                let h = crate::nanbox_encode::__torajs_anyv_box_pointer(value as *mut c_void);
                let r =
                    crate::method_call_closure_dispatch::__torajs_any_call_with_this(h, c, &v, 1);
                let b = crate::nanbox_ffi::__torajs_anyv_to_bool(r);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(r);
                Some(b)
            }
        } else if !matches!(tag, TAG_NULL | TAG_UNDEF) {
            __torajs_throw_type_error(c"Symbol.hasInstance handler is not callable".as_ptr());
            Some(false)
        } else {
            None
        }
    }
}

/// §7.3.22 steps 3 and 6-7 — is `pval` on O's [[Prototype]] chain?
///
/// O must be an Object (step 3, non-objects answer false); the walk
/// compares cell identity. The proto getter answers an OWNED
/// reference (`share_proto`, `dynobj_entry`), so each hop is released
/// once the next one is read: keeping them leaked one reference per
/// mortal prototype per `instanceof` that walked it, which pinned the
/// constructor behind it for good.
///
/// # Safety
/// `v` is a valid AnyValue; `pval` is a heap cell address.
// one shared copy for its four callers
#[inline(never)]
pub(crate) unsafe fn proto_chain_reaches(v: AnyValue, pval: u64) -> bool {
    unsafe {
        if !is_heap_object(v) {
            return false;
        }
        let mut cur = __torajs_anyv_get_proto_of_any(v);
        while cur != crate::nanbox::VALUE_NULL && cur != 0 {
            let reached = is_cell(cur) && as_void_ptr(cur) as u64 == pval;
            let next = if reached {
                0
            } else {
                __torajs_anyv_get_proto_of_any(cur)
            };
            crate::nanbox_ffi::__torajs_anyv_rc_dec(cur);
            if reached {
                return true;
            }
            cur = next;
        }
        false
    }
}

/// §7.3.22 for a target that is a PROXY. Steps 4-7 are ordinary:
/// `Get(C, "prototype")` runs the proxy's `get` trap, and the answer
/// is walked against V's chain exactly as for any other callable.
/// There is no proxy-specific rule here — step 1's IsCallable is the
/// only place a proxy differs, and it reads through to the target.
///
/// `None` when `target` is not a proxy. A proxy that is not callable
/// is the caller's step-1 decision, not this function's.
///
/// # Safety
/// `target` is a live AnyValue over a Proxy cell.
pub(crate) unsafe fn proxy_has_instance(v: AnyValue, target: AnyValue) -> Option<bool> {
    if !crate::proxy::is_proxy(target) {
        return None;
    }
    unsafe {
        let p = crate::proxy::get_named(target, b"prototype");
        if __torajs_throw_check() != 0 {
            return Some(false);
        }
        // §7.3.22 step 6 — "If P is not an Object, throw a TypeError".
        if !crate::iter_zip_shared::av_is_object(p) {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(p);
            __torajs_throw_type_error(
                c"Function has non-object prototype in instanceof check".as_ptr(),
            );
            return Some(false);
        }
        let out = proto_chain_reaches(v, as_void_ptr(p) as u64);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(p);
        Some(out)
    }
}

/// §7.3.22 for a target that is one of tr's CLASS objects.
///
/// A class constructor is a function object in the spec, but tr models
/// it as a dynobj carrying `FLAG_DYNOBJ_CLASS_CTOR` (the bit that
/// makes `typeof C` answer `"function"`) — so the closure-shaped walk
/// above cannot read its `.prototype`. That property is an ordinary
/// own entry on the class dynobj itself, pointing at the very object
/// every instance's [[Prototype]] links to.
///
/// `None` when the cell is not a class object, or carries no
/// object-shaped `prototype` — the caller decides which spec step
/// that is.
///
/// # Safety
/// `cell` is a live heap cell.
pub(crate) unsafe fn class_ctor_has_instance(v: AnyValue, cell: *mut c_void) -> Option<bool> {
    unsafe {
        let hdr = &*(cell as *const HeapHeader);
        if hdr.type_tag != Tag::DynObj as u16 || hdr.flags & torajs_rc::FLAG_DYNOBJ_CLASS_CTOR == 0
        {
            return None;
        }
        let (ptag, pval) = crate::closure_proto::class_prototype_pair(cell)?;
        // §7.3.22 step 6 — "If P is not an Object, throw a TypeError".
        // This used to answer `None`, which means "not a class object"
        // to the caller, and the caller's fallthrough raises "Right-hand
        // side of 'instanceof' is not callable" — and only in strict
        // mode. Two things wrong at once: the wrong error, and no error
        // at all in sloppy code. It is the same step the closure lane
        // above enforces, so it asks the same predicate.
        let p = crate::nanbox_encode::__torajs_anyv_box_from_pair(ptag as i64, pval as i64);
        if !crate::iter_zip_shared::av_is_object(p) {
            __torajs_throw_type_error(
                c"Function has non-object prototype in instanceof check".as_ptr(),
            );
            return Some(false);
        }
        Some(proto_chain_reaches(v, pval))
    }
}

/// Is this completion value an Object in the spec sense — a heap cell
/// that is not one of the heap-shaped primitives (Str / Symbol /
/// BigInt)?
pub(crate) unsafe fn is_heap_object(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    let ptr = as_void_ptr(av);
    if ptr.is_null() {
        return false;
    }
    // SAFETY: a cell-encoded AnyValue points at a live heap header.
    let hdr = unsafe { &*(ptr as *const HeapHeader) };
    hdr.type_tag != Tag::Str as u16
        && hdr.type_tag != Tag::Symbol as u16
        && hdr.type_tag != Tag::BigInt as u16
}
