//! Own-bag method dispatch for the cell shapes whose ordinary own
//! face is a lazy props bag — split from `method_call.rs` (file-size
//! cap) as the wrapper-only probe, widened to the whole bag table in
//! rotation 528. See the probe doc below; the dispatcher calls it
//! before the subclass / view-through / per-tag surfaces so the ES
//! own-property order holds.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::method_call::{closure_cell_entry, invoke_with_this, not_callable};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair: the slot's ANY_TAG
    /// (5 = ANY_UNDEF, which is both "absent" and "stored
    /// undefined") / per-tag payload.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — own-key membership probe (1 = present), the
    /// disambiguator for `get_tag`'s ANY_UNDEF answer.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — run an accessor entry's getter; the answer is
    /// an owned AnyValue per the boxed-value convention.
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    /// torajs-throw — pending-throw probe (non-zero = pending); the
    /// getter-as-callee arm aborts before the callee probe so
    /// `not_callable` cannot clobber the user throw.
    fn __torajs_throw_check() -> i64;
}

/// ANY_TAG of a heap-cell payload — mirror of torajs-core
/// `ssa_lower.rs`'s ANY_HEAP. Only this tag's payload bits are a
/// pointer; reading any other slot as one is a wild deref.
const ANY_HEAP_TAG: u64 = 4;

/// ANY_TAG of an accessor pair — mirror of torajs-core
/// `ssa_lower_accessor.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: u64 = 6;

/// Whether a call against this cell shape must consult the own bag
/// first — every shape [`expando_props_off`] knows a slot for except
/// `Tag::Closure`, whose arm carries its own expando probe (and
/// whose `call` / `apply` shadowing already resolves there).
///
/// §10.1.8.1 OrdinaryGet does not care what internal state a
/// receiver carries: an own entry shadows the inherited builtin
/// surface. Until rotation 528 only wrappers and `Tag::Arr` (its own
/// probe, inside its arm) honoured that on the CALL side, so a
/// function stored on a Map / Set / Date / RegExp / Promise / buffer
/// / iterator / struct receiver was "not a function", and an own
/// entry whose name collided with a builtin — `m.get`, `d.getTime` —
/// silently ran the builtin instead.
///
/// [`expando_props_off`]: crate::member_get_layout::expando_props_off
#[inline]
pub(crate) fn bag_probed_tag(tag: u16) -> bool {
    tag != Tag::Closure as u16 && crate::member_get_layout::expando_props_off(tag).is_some()
}

/// Own-bag method probe — `Some(result)` when the receiver's lazy
/// props bag owns `name` as a live entry and the stored value
/// dispatches: a reified builtin cell re-dispatches its ORIGINAL mid
/// against this receiver (the generic `.call` semantics — a
/// String.prototype method ToStrings the receiver, a
/// Boolean.prototype one brand-checks it), any other closure invokes
/// with the receiver as `this` through the receiver channel, and a
/// stored non-callable is the §13.3.6 TypeError. `None` = no entry;
/// the caller falls through to the inherited surfaces.
///
/// # Safety
/// `ptr` is a live heap cell whose header carries `tag`; `name_str`
/// is NULL or a live Str cell; `argv`/`argc` follow the boxed-adapter
/// convention.
pub(crate) unsafe fn bag_expando_method(
    recv: AnyValue,
    ptr: *mut c_void,
    tag: u16,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let props = crate::member_get_layout::expando_props(ptr, tag);
        if props.is_null() {
            return None;
        }
        // A known-mid call site carries no name bytes — mint the
        // canonical name from the meta row for the expando probe
        // (cold path; the expando table is a monkey-patch surface).
        let mut minted: *mut u8 = core::ptr::null_mut();
        let key: *const u8 = if !name_str.is_null() {
            name_str
        } else if let Some((nm, _)) = torajs_rc::any_method_meta(mid) {
            let s = crate::__torajs_str_alloc_pooled(nm.len() as u64);
            core::ptr::copy_nonoverlapping(nm.as_ptr(), s.add(16), nm.len());
            minted = s;
            s
        } else {
            return None;
        };
        let release = |m: *mut u8| {
            if !m.is_null() {
                crate::__torajs_str_drop(m as *mut c_void);
            }
        };
        let dtag = __torajs_dynobj_get_tag(props, key as *const c_void);
        let kv = key as *const c_void;
        if dtag == 5 {
            // ANY_UNDEF conflates "absent" with "own entry storing
            // undefined", and the two answer differently: a stored
            // undefined SHADOWS the builtin, so `m.has = undefined;
            // m.has(1)` is the resolved-not-callable TypeError, not
            // the builtin `has` (mirror of the arr / dynobj arms).
            let present = __torajs_dynobj_has(props, kv) != 0;
            release(minted);
            return if present { Some(not_callable()) } else { None };
        }
        if dtag == ANY_ACCESSOR_TAG {
            let out = accessor_callee(props, kv, recv, argv, argc);
            release(minted);
            return Some(out);
        }
        if dtag != ANY_HEAP_TAG {
            // A stored primitive shadows the builtin and is not
            // callable. Its payload bits are a number / bool / short
            // string, NOT a pointer — the pre-528 wrapper probe read
            // them as one and segfaulted (`w.zz = 5; w.zz()`).
            release(minted);
            return Some(not_callable());
        }
        let cell = __torajs_dynobj_get_value(props, kv) as *mut c_void;
        release(minted);
        // Reified builtin method cell — re-dispatch the original id
        // with this receiver (the closure_method .call path's
        // CallTarget::Builtin arm, generic coerce included).
        if let Some(orig_mid) = crate::method_value::builtin_method_mid(cell) {
            let fam = crate::method_value::builtin_method_family(cell);
            if let Some(out) =
                crate::method_call_closure::generic_builtin_this(orig_mid, recv, argv, argc, fam)
            {
                return Some(out);
            }
            return Some(crate::method_call::any_method_redispatch(
                recv, orig_mid, argv, argc,
            ));
        }
        if let Some((env, entry)) = closure_cell_entry(cell) {
            return Some(invoke_with_this(env, entry, recv, argv, argc));
        }
        Some(not_callable())
    }
}

/// Getter-as-callee over a bag entry — the accessor's getter runs
/// with the OUTER cell as its receiver (the bag is the holder's
/// storage, never the `this`), and its answer dispatches as the
/// callee. A throwing getter aborts before the callee probe, so
/// `not_callable` cannot clobber the user's pending throw (§13.3.6.1
/// Get ReturnIfAbrupt). Mirror of `method_call_dynobj`'s arm, which
/// can pass the holder as both because there the two are the same
/// object.
///
/// # Safety
/// `props` is a live dynobj whose entry under `key` is an accessor
/// pair; `argv` holds `argc` live slots.
unsafe fn accessor_callee(
    props: *const c_void,
    key: *const c_void,
    recv: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let pair = __torajs_dynobj_get_value(props, key) as *const c_void;
        let got = __torajs_accessor_invoke_getter(pair, recv as u64);
        if __torajs_throw_check() != 0 {
            return got;
        }
        if crate::nanbox::is_cell(got)
            && let Some((env, entry)) = closure_cell_entry(crate::nanbox::as_void_ptr(got))
        {
            let r = invoke_with_this(env, entry, recv, argv, argc);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
            return r;
        }
        crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
        not_callable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate is the bag table itself, minus the one shape that
    /// probes its own — so a shape that grows a bag is picked up
    /// here for free rather than needing a new arm.
    #[test]
    fn every_bag_shape_but_closure_is_probed() {
        for t in [
            Tag::Map,
            Tag::Set,
            Tag::Date,
            Tag::RegExp,
            Tag::ArrIter,
            Tag::MapIter,
            Tag::IterHelper,
            Tag::Promise,
            Tag::TypedArray,
            Tag::ArrayBuffer,
            Tag::DataView,
            Tag::Obj,
            Tag::NumberWrapper,
            Tag::StringWrapper,
            Tag::BooleanWrapper,
            Tag::SymbolWrapper,
        ] {
            assert!(bag_probed_tag(t as u16), "{t:?} carries a bag");
        }
        assert!(!bag_probed_tag(Tag::Closure as u16));
        // `Tag::Arr`'s expando lives behind the arrprops kernels, so
        // the table knows no slot for it and its arm keeps its own
        // probe.
        assert!(!bag_probed_tag(Tag::Arr as u16));
        assert!(!bag_probed_tag(Tag::DynObj as u16));
    }
}
