//! §27.1.6.2 `Iterator.from` + §27.1.4.5 flatMap inner drive — RFC
//! 20260730-iterator-global 刀 4.
//!
//! Both faces ride GetIteratorFlattenable: derive an iterator from a
//! value that may or may not carry `@@iterator`, treating the value
//! ITSELF as the iterator when the method is absent (unlike §7.4.2
//! GetIterator, which refuses). The two callers split on primitives:
//! `Iterator.from` iterates string primitives, flatMap rejects every
//! primitive (spec's ITERATE-STRING-PRIMITIVES / REJECT-PRIMITIVES).
//!
//! `Iterator.from` answers an already-Iterator input as itself
//! (§7.3.22 walk against %Iterator.prototype%, builtin-proto tag 15);
//! anything else minted here wears a kind-WRAP IterHelper cell whose
//! next/return forward to the underlying (§27.1.6.3
//! %WrapForValidIterator%).

use core::ffi::c_void;

use crate::iter_any::{MethodOutcome, call_obj_method_0};
use crate::iter_any_get_method::{GetIterator, get_iterator};
use crate::iter_helper::{
    ALIVE_OFF, COUNTER_OFF, FN_OFF, INNER_OFF, UNDERLYING_OFF, iter_helper_mint_wrap,
    iter_result_obj,
};
use crate::method_call_closure_dispatch::{closure_cell_entry, invoke_boxed, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_short_str};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use torajs_rc::Tag;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    /// torajs-meta — §7.3.22 OrdinaryHasInstance walk against a
    /// builtin prototype tag (15 = %Iterator.prototype%).
    fn __torajs_instanceof_builtin_proto(v: u64, proto_tag: i64) -> bool;
    /// torajs-arr / torajs-collections — the builtin `@@iterator`
    /// lane mints (each rc-incs its source; fresh rc-1 iter cell).
    fn __torajs_arr_iter_create_values(arr: *mut c_void) -> *mut c_void;
    fn __torajs_map_iter_create_keys(p: *mut c_void) -> *mut c_void;
    fn __torajs_map_iter_create_entries(p: *mut c_void) -> *mut c_void;
    /// torajs-str — key literal for the `return` forward probe.
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// `builtin_proto` table tag of %Iterator.prototype% (刀 1).
const ITERATOR_PROTO_TAG: i64 = 15;

/// `AnySlotTag::Undef` / `AnySlotTag::Null` in the member protocol.
const TAG_UNDEF: u64 = 5;
const TAG_NULL: u64 = 0;

/// §27.1.6.2 GetIteratorFlattenable — an OWNED iterator derived from
/// `v`. `allow_string` is the ITERATE-STRING-PRIMITIVES (from) /
/// REJECT-PRIMITIVES (flatMap) split. `None` = pending throw.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern.
pub(crate) unsafe fn derive_flattenable(v: AnyValue, allow_string: bool) -> Option<AnyValue> {
    unsafe {
        // Strings sit outside the @@iterator probe: tr's reified
        // String.prototype[@@iterator] IS str_iterator_mint.
        let cell_tag = if is_cell(v) {
            Some((as_void_ptr(v).cast::<u8>().add(4) as *const u16).read())
        } else {
            None
        };
        if is_short_str(v) || cell_tag == Some(Tag::Str as u16) {
            if !allow_string {
                __torajs_throw_type_error(c"flatMap mapped value is not an object".as_ptr());
                return None;
            }
            let it = crate::str_iterator::str_iterator_mint(v);
            if __torajs_throw_check() != 0 {
                return None;
            }
            return Some(it);
        }
        let Some(tag) = cell_tag else {
            __torajs_throw_type_error(c"value is not iterable".as_ptr());
            return None;
        };
        // GetMethod(v, @@iterator) then call — a user method wins;
        // native iterables answer NoUserMethod (their reified cell
        // aliases the builtin lane minted below).
        match get_iterator(v) {
            GetIterator::Iterator(it) => return Some(it),
            GetIterator::Threw => return None,
            GetIterator::NoUserMethod => {}
        }
        let minted = match tag {
            t if t == Tag::Arr as u16 => {
                __torajs_arr_iter_create_values(as_void_ptr(v) as *mut c_void)
            }
            // §22.1.5 — a String exotic object is an Object whose
            // @@iterator walks code units; ride the reified
            // String.prototype[@@iterator] body (刀 5b: zip's
            // string-object elements were the first consumer).
            t if t == Tag::StringWrapper as u16 => {
                let it = crate::str_iterator::str_iterator_mint(v);
                if __torajs_throw_check() != 0 {
                    return None;
                }
                return Some(it);
            }
            // Map's @@iterator is entries (§24.1.3.13); Set's is
            // values, which the collections kernel serves as keys
            // (§24.2.3.8 alias).
            t if t == Tag::Map as u16 => {
                __torajs_map_iter_create_entries(as_void_ptr(v) as *mut c_void)
            }
            t if t == Tag::Set as u16 => {
                __torajs_map_iter_create_keys(as_void_ptr(v) as *mut c_void)
            }
            _ => {
                // §27.1.6.2 step 2.b — no @@iterator: `v` itself is
                // the iterator (a missing next() surfaces at the
                // first step, not here).
                crate::payload_rc_inc(
                    crate::__torajs_anyv_unbox_tag(v),
                    crate::__torajs_anyv_unbox_value(v),
                );
                return Some(v);
            }
        };
        Some(crate::nanbox_encode::__torajs_anyv_box_pointer(minted))
    }
}

/// §27.1.6.2 `Iterator.from(O)` — the statics wedge's kernel.
/// Answers an owned iterator: the input itself when it already sits
/// on %Iterator.prototype%'s chain, a kind-WRAP helper cell
/// otherwise; undefined with a pending throw on a non-iterable.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iterator_from(v: AnyValue) -> AnyValue {
    unsafe {
        let Some(it) = derive_flattenable(v, true) else {
            return VALUE_UNDEFINED;
        };
        // Step 3 — an existing Iterator passes through as itself.
        if __torajs_instanceof_builtin_proto(it, ITERATOR_PROTO_TAG) {
            return it;
        }
        // Step 4 — WrapForValidIterator; the owned `it` transfers.
        iter_helper_mint_wrap(it)
    }
}

/// §27.1.6.3.2 %WrapForValidIterator%.return — forward to the
/// underlying's own `return` and pass its result through; an absent
/// method answers the spec's fresh `{ undefined, done: true }`.
/// (The caller already flipped the cell dead.)
///
/// # Safety
/// `underlying` is the cell's live underlying AnyValue.
pub(crate) unsafe fn wrap_return_forward(underlying: AnyValue) -> AnyValue {
    unsafe {
        // Vtable tier first — a class instance / generator keeps
        // `return` as a method (same cascade the close family runs).
        if is_cell(underlying)
            && (as_void_ptr(underlying).cast::<u8>().add(4) as *const u16).read() == Tag::Obj as u16
        {
            match call_obj_method_0(as_void_ptr(underlying) as *mut c_void, b"return") {
                MethodOutcome::Ok(r) => return r,
                MethodOutcome::Threw => return VALUE_UNDEFINED,
                MethodOutcome::Missing => {}
            }
        }
        // Member tier — a closure-valued `return` field, or none.
        let key = __torajs_str_alloc(c"return".as_ptr() as *const u8, 6);
        let tag = crate::member_get::__torajs_any_member_get_tag(underlying, key as *const c_void);
        let payload = if tag == TAG_UNDEF {
            0
        } else {
            crate::member_get_value::__torajs_any_member_get_value(underlying, key as *const c_void)
        };
        __torajs_str_drop(key as *mut c_void);
        if tag == TAG_UNDEF || tag == TAG_NULL {
            return iter_result_obj(VALUE_UNDEFINED, true);
        }
        let method = crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, payload as i64);
        let entry = if is_cell(method) {
            closure_cell_entry(as_void_ptr(method) as *mut c_void)
        } else {
            None
        };
        let Some((env, entry)) = entry else {
            __torajs_throw_type_error(c"return is not a function".as_ptr());
            return VALUE_UNDEFINED;
        };
        let argv: [u64; 0] = [];
        let r = invoke_with_this(env, entry, underlying, argv.as_ptr(), 0);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        r
    }
}

/// §27.1.4.5 flatMap — one protocol step of the double loop: drain
/// the current inner iterator; else step the outer, map, and open
/// the next inner through GetIteratorFlattenable (REJECT-PRIMITIVES).
/// Same contract as `iter_helper_step`: 1 with `*out` owned, 0 on
/// done / pending throw.
///
/// # Safety
/// `ptr` is a live IterHelper cell of kind FLAT_MAP (alive already
/// checked by the caller); `out` is writable.
pub(crate) unsafe fn iter_flat_map_step(ptr: *mut c_void, out: *mut AnyValue) -> i64 {
    unsafe {
        let p = ptr.cast::<u8>();
        let underlying = (p.add(UNDERLYING_OFF) as *const u64).read();
        loop {
            let inner = (p.add(INNER_OFF) as *const u64).read();
            if inner != VALUE_UNDEFINED {
                let mut item: AnyValue = VALUE_UNDEFINED;
                let hit = crate::iter_any_step::step_derived_iterator(inner, &mut item, false);
                if hit == 1 {
                    *out = item;
                    return 1;
                }
                if __torajs_throw_check() != 0 {
                    // Step 5.b.ix.4.d — an inner abrupt completion
                    // closes the outer.
                    (p.add(ALIVE_OFF)).write(0);
                    crate::iter_any_close::__torajs_iter_close_value(underlying);
                    return 0;
                }
                // Inner exhausted — release it and step the outer.
                __torajs_anyv_rc_dec(inner);
                (p.add(INNER_OFF) as *mut u64).write(VALUE_UNDEFINED);
            }
            let mut item: AnyValue = VALUE_UNDEFINED;
            let hit = crate::iter_helper_next::helper_underlying_step(ptr, &mut item);
            if hit == 0 {
                // Outer done or threw — either way this helper is
                // finished (the caller's throw check tells apart).
                (p.add(ALIVE_OFF)).write(0);
                return 0;
            }
            let counter = (p.add(COUNTER_OFF) as *const u64).read();
            (p.add(COUNTER_OFF) as *mut u64).write(counter + 1);
            let fn_av = (p.add(FN_OFF) as *const u64).read();
            let Some((env, entry)) = closure_cell_entry(as_void_ptr(fn_av) as *mut c_void) else {
                // Unreachable by construction (mint validated).
                __torajs_anyv_rc_dec(item);
                (p.add(ALIVE_OFF)).write(0);
                return 0;
            };
            let counter_av = crate::__torajs_anyv_box_from_pair(2, counter as i64) as u64;
            let argv = [item, counter_av];
            let mapped = invoke_boxed(env, entry, argv.as_ptr(), 2);
            __torajs_anyv_rc_dec(item);
            if __torajs_throw_check() != 0 {
                // Step 5.b.iv — mapper threw: close the outer,
                // forward the throw.
                __torajs_anyv_rc_dec(mapped);
                (p.add(ALIVE_OFF)).write(0);
                crate::iter_any_close::__torajs_iter_close_value(underlying);
                return 0;
            }
            match derive_flattenable(mapped, false) {
                Some(inner_it) => {
                    // The inner slot takes the derived reference;
                    // the mapped intermediate releases (when the
                    // value IS its own iterator the derive inc'd).
                    __torajs_anyv_rc_dec(mapped);
                    (p.add(INNER_OFF) as *mut u64).write(inner_it);
                }
                None => {
                    // Step 5.b.vi — not flattenable: close the
                    // outer, forward the TypeError.
                    __torajs_anyv_rc_dec(mapped);
                    (p.add(ALIVE_OFF)).write(0);
                    crate::iter_any_close::__torajs_iter_close_value(underlying);
                    return 0;
                }
            }
        }
    }
}
