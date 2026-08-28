//! `Iterator.concat(...items)` — RFC 20260730-iterator-global 刀 5a
//! (proposal-iterator-sequencing, stage 3).
//!
//! The static runs step 2's `GetMethod(item, @@iterator)` EAGERLY for
//! every item and PARKS the answer — that is what the spec means by
//! `[[OpenMethod]]` — then answers a lazy kind-CONCAT IterHelper cell
//! that opens each item in sequence at step time: the underlying slot
//! holds the items `Array<Any>`, the fn slot the parallel open-method
//! list, the counter is the next-item index, and the inner slot
//! drives exactly like flatMap's.
//!
//! The eager half used to be a PRESENCE check that re-walked the
//! symbol at open time. A data property cannot tell the two apart; an
//! accessor can, three ways: its getter ran at the first `next()`
//! instead of at `concat()`, a second walk would have run it twice,
//! and a getter answering a non-callable refused at the first step
//! instead of at construction. Parking the method fixes all three,
//! because the spec's one GetMethod is then the only one there is.
//!
//! Recorded boundary, unchanged: a class instance (`Tag::Obj`) carries
//! its `@@iterator` on the vtable / genfn chain, which the symbol-key
//! walk cannot see, and a native tag's reify IS the lazy lane's own
//! behavior. Both park `undefined` as their open method, meaning
//! "open this one the lazy way", and surface a missing-`next`
//! TypeError at the first step (generators and hand-written iterator
//! classes are the common case and step fine).

use core::ffi::c_void;

use crate::iter_from::derive_flattenable;
use crate::iter_helper::{
    ALIVE_OFF, COUNTER_OFF, FN_OFF, INNER_OFF, ITER_HELPER_CONCAT, UNDERLYING_OFF,
    iter_helper_cell_alloc,
};
use crate::method_call_closure_dispatch::{closure_cell_entry, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_short_str};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use torajs_rc::Tag;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    /// torajs-arr — kind-aware borrowed whole-box slot read (the same
    /// lane ArrIter's step drives).
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    /// torajs-str — well-known singleton table; idx 5 = `@@iterator`.
    /// Answers an owned +1.
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    /// torajs-arr — rc-aware Array<Any> drop (releases slots); the
    /// refusal path owns the transferred items list and must let it
    /// go.
    fn __torajs_arr_drop_any(arr: *mut c_void);
    /// torajs-arr — fresh rc-1 `Array<Any>` with room for `cap`.
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-arr — append one pair; the value's stake TRANSFERS to
    /// the array. Answers the (possibly reallocated) array.
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
}

/// `WELL_KNOWN_DESCS` index of `Symbol.iterator`.
const WK_ITERATOR: i64 = 5;

/// `AnySlotTag::Undef` / `Null` in the member protocol.
const TAG_UNDEF: u64 = 5;
const TAG_NULL: u64 = 0;

/// Array length word (mirror of `torajs-arr::layout::ARR_LEN_OFF`).
const ARR_LEN_OFF: usize = 8;

/// One item's eager step 2 — a non-object refuses (2.a), then
/// `GetMethod(item, @@iterator)` (2.b) whose answer is what gets
/// parked; a nullish (2.c) or present-but-not-callable one refuses
/// HERE, at `concat()`, not at the first `next()`.
///
/// `Some(m)` is the open method to park: an OWNED method cell, or
/// `undefined` meaning "open this item the lazy way" — the vtable /
/// genfn boundary and the native-tag reify, whose behavior the lazy
/// lane already is. `None` = pending throw.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern.
unsafe fn item_open_method(v: AnyValue) -> Option<AnyValue> {
    unsafe {
        if is_short_str(v) || !is_cell(v) {
            __torajs_throw_type_error(c"Iterator.concat argument is not an object".as_ptr());
            return None;
        }
        let tag = (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Str as u16 {
            // A string primitive is not an Object (§27.1.2.x step
            // 2.a) — the wrapper object form passes below.
            __torajs_throw_type_error(c"Iterator.concat argument is not an object".as_ptr());
            return None;
        }
        // Vtable-carried @@iterator (class instances / generators)
        // sits outside the symbol-key walk — recorded boundary, the
        // open validates lazily.
        if tag == Tag::Obj as u16 {
            return Some(VALUE_UNDEFINED);
        }
        let sym = __torajs_symbol_well_known(WK_ITERATOR);
        if sym.is_null() {
            return Some(VALUE_UNDEFINED);
        }
        // A real Get: an accessor-shaped @@iterator runs its getter
        // here, ONCE — the answer is parked, so the open below never
        // asks again.
        let open_method = crate::member_get_symbol::symbol_key_get(v, sym);
        let (t, payload) = open_method.pair();
        let _ = __torajs_rc_dec(sym);
        if __torajs_throw_check() != 0 {
            return None;
        }
        if t == TAG_UNDEF || t == TAG_NULL {
            __torajs_throw_type_error(c"Iterator.concat argument is not iterable".as_ptr());
            return None;
        }
        let closure = if t == 4 && payload != 0 {
            let cell = payload as *mut c_void;
            // SAFETY: tag 4 payloads are live heap cells; only a
            // Closure-tagged one carries the boxed-entry
            // discriminator the two reads below want.
            let ct = (cell.cast::<u8>().add(4) as *const u16).read();
            if ct == torajs_rc::Tag::Closure as u16 {
                Some(cell)
            } else {
                None
            }
        } else {
            None
        };
        // A native tag's reified @@iterator aliases the builtin open
        // the lazy lane mints; parking it would route the Arr / Map /
        // Set fast opens through a user-method invoke instead.
        if let Some(cell) = closure
            && crate::method_value::builtin_method_mid(cell).is_some()
        {
            return Some(VALUE_UNDEFINED);
        }
        // Step 2.c's other half — present but not callable refuses at
        // construction. It used to ride to the first step as a
        // missing-`next` TypeError.
        if closure.is_none_or(|cell| closure_cell_entry(cell).is_none()) {
            __torajs_throw_type_error(c"Iterator.concat argument is not iterable".as_ptr());
            return None;
        }
        // The parked list outlives this frame, so the +1 leaves the
        // guard with it — a borrowed dict entry takes one of its own.
        Some(open_method.into_owned_value())
    }
}

/// Step 3's `Call(iterable.[[OpenMethod]], iterable.[[Iterable]])`
/// over the method resolved back at `concat()` — an accessor's getter
/// already ran then, and is not asked again here.
///
/// # Safety
/// `item` is a live AnyValue; `method` is a live callable cell.
unsafe fn open_with_method(item: AnyValue, method: AnyValue) -> Option<AnyValue> {
    unsafe {
        // Parked methods are Closure cells by construction (the eager
        // step refused everything else), so this is a live cell.
        let cell = as_void_ptr(method) as *mut c_void;
        let Some((env, entry)) = closure_cell_entry(cell) else {
            __torajs_throw_type_error(c"Iterator.concat argument is not iterable".as_ptr());
            return None;
        };
        let argv: [u64; 0] = [];
        let it = invoke_with_this(env, entry, item, argv.as_ptr(), 0);
        if __torajs_throw_check() != 0 {
            return None;
        }
        if !is_cell(it) {
            __torajs_anyv_rc_dec(it);
            __torajs_throw_type_error(c"iterator is not an object".as_ptr());
            return None;
        }
        Some(it)
    }
}

/// `Iterator.concat(...items)` kernel — the statics wedge packs the
/// arguments into a fresh `Array<Any>` whose reference TRANSFERS
/// here: the minted cell's underlying slot takes it, and the refusal
/// path drops it along with whatever open methods were parked before
/// the refusal. Undefined with a pending throw when an item fails
/// step 2.
///
/// # Safety
/// `items` is a live rc-1 `Array<Any>` heap pointer, ownership
/// transferred by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iterator_concat(items: *mut c_void) -> AnyValue {
    unsafe {
        let len = (items.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        // Step 2 runs over every item BEFORE any of them is opened,
        // and each answer is parked in a list parallel to `items`.
        let mut methods = __torajs_arr_alloc_any(len) as *mut c_void;
        for i in 0..len {
            let Some(m) = item_open_method(__torajs_arr_get_any_boxed(items as *const c_void, i))
            else {
                __torajs_arr_drop_any(methods);
                __torajs_arr_drop_any(items);
                return VALUE_UNDEFINED;
            };
            // The push TRANSFERS the method's stake to the list.
            methods = __torajs_arr_push_any(
                methods,
                crate::__torajs_anyv_unbox_tag(m) as u64,
                crate::__torajs_anyv_unbox_value(m) as u64,
            ) as *mut c_void;
        }
        let cell = iter_helper_cell_alloc(ITER_HELPER_CONCAT);
        *(cell.add(UNDERLYING_OFF) as *mut u64) =
            crate::nanbox_encode::__torajs_anyv_box_pointer(items);
        *(cell.add(FN_OFF) as *mut u64) = crate::nanbox_encode::__torajs_anyv_box_pointer(methods);
        crate::nanbox_encode::__torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// One protocol step of a kind-CONCAT cell: drain the current inner
/// iterator, else open the next item — through its parked
/// [[OpenMethod]], or GetIteratorFlattenable for the boundary items
/// that parked `undefined`.
/// Same contract as `iter_helper_step`: 1 with `*out` owned, 0 on
/// done / pending throw.
///
/// # Safety
/// `ptr` is a live IterHelper cell of kind CONCAT (alive already
/// checked by the caller); `out` is writable.
pub(crate) unsafe fn iter_concat_step(ptr: *mut c_void, out: *mut AnyValue) -> i64 {
    unsafe {
        let p = ptr.cast::<u8>();
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
                    // An inner abrupt completion finishes the whole
                    // concat (there is no outer iterator to close —
                    // the remaining items were never opened).
                    (p.add(ALIVE_OFF)).write(0);
                    return 0;
                }
                // Inner exhausted — release it, open the next item.
                __torajs_anyv_rc_dec(inner);
                (p.add(INNER_OFF) as *mut u64).write(VALUE_UNDEFINED);
            }
            let idx = (p.add(COUNTER_OFF) as *const u64).read();
            let items_av = (p.add(UNDERLYING_OFF) as *const u64).read();
            let items = as_void_ptr(items_av) as *mut c_void;
            let len = (items.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
            if idx >= len {
                (p.add(ALIVE_OFF)).write(0);
                return 0;
            }
            (p.add(COUNTER_OFF) as *mut u64).write(idx + 1);
            // Borrowed slot reads; both opens answer an OWNED
            // iterator (self-iterator items get their own inc).
            let item = __torajs_arr_get_any_boxed(items as *const c_void, idx);
            let methods_av = (p.add(FN_OFF) as *const u64).read();
            let method = __torajs_arr_get_any_boxed(as_void_ptr(methods_av) as *const c_void, idx);
            // `undefined` = the boundary items, which open lazily; a
            // parked method opens through the spec's stored
            // [[OpenMethod]] and never re-reads the symbol.
            let opened = if method == VALUE_UNDEFINED {
                derive_flattenable(item, false)
            } else {
                open_with_method(item, method)
            };
            match opened {
                Some(it) => (p.add(INNER_OFF) as *mut u64).write(it),
                None => {
                    (p.add(ALIVE_OFF)).write(0);
                    return 0;
                }
            }
        }
    }
}

/// §7.4.9-shape close for the CONCAT return() face — only the
/// currently-open inner needs closing (the items list is data, not
/// an iterator; unopened items never observe the close).
///
/// # Safety
/// `ptr` is a live IterHelper cell of kind CONCAT.
pub(crate) unsafe fn iter_concat_close_inner(ptr: *mut c_void) {
    unsafe {
        let p = ptr.cast::<u8>();
        let inner = (p.add(INNER_OFF) as *const u64).read();
        if inner != VALUE_UNDEFINED {
            crate::iter_any_close::__torajs_iter_close_value(inner);
            __torajs_anyv_rc_dec(inner);
            (p.add(INNER_OFF) as *mut u64).write(VALUE_UNDEFINED);
        }
    }
}
