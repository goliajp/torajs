//! `Iterator.concat(...items)` — RFC 20260730-iterator-global 刀 5a
//! (proposal-iterator-sequencing, stage 3).
//!
//! The static validates every item EAGERLY (non-object → TypeError;
//! an object with no `@@iterator` face → TypeError) and answers a
//! lazy kind-CONCAT IterHelper cell that opens each item's iterator
//! in sequence at step time: the underlying slot holds the items
//! `Array<Any>`, the counter is the next-item index, and the inner
//! slot drives exactly like flatMap's.
//!
//! Recorded boundary: a class instance (`Tag::Obj`) carries its
//! `@@iterator` on the vtable / genfn chain, which the symbol-key
//! walk cannot see statically — those items pass the eager check and
//! surface a missing-`next` TypeError at the first step instead of
//! at construction (generators, hand-written iterator classes are
//! the common case and step fine).

use core::ffi::c_void;

use crate::iter_from::derive_flattenable;
use crate::iter_helper::{
    ALIVE_OFF, COUNTER_OFF, INNER_OFF, ITER_HELPER_CONCAT, UNDERLYING_OFF, iter_helper_cell_alloc,
};
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
}

/// `WELL_KNOWN_DESCS` index of `Symbol.iterator`.
const WK_ITERATOR: i64 = 5;

/// `AnySlotTag::Undef` / `Null` in the member protocol.
const TAG_UNDEF: u64 = 5;
const TAG_NULL: u64 = 0;

/// Array length word (mirror of `torajs-arr::layout::ARR_LEN_OFF`).
const ARR_LEN_OFF: usize = 8;

/// One item's eager check — proposal §27.1.2.x steps 2.a-2.b: a
/// non-object refuses, and so does an object whose `@@iterator`
/// resolves nullish. `false` = pending throw.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern.
unsafe fn item_is_iterable(v: AnyValue) -> bool {
    unsafe {
        if is_short_str(v) || !is_cell(v) {
            __torajs_throw_type_error(c"Iterator.concat argument is not an object".as_ptr());
            return false;
        }
        let tag = (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Str as u16 {
            // A string primitive is not an Object (§27.1.2.x step
            // 2.a) — the wrapper object form passes below.
            __torajs_throw_type_error(c"Iterator.concat argument is not an object".as_ptr());
            return false;
        }
        // Vtable-carried @@iterator (class instances / generators)
        // sits outside the symbol-key walk — recorded boundary, the
        // open validates lazily.
        if tag == Tag::Obj as u16 {
            return true;
        }
        let sym = __torajs_symbol_well_known(WK_ITERATOR);
        if sym.is_null() {
            return true;
        }
        let (t, _) = crate::member_get_symbol::symbol_key_pair(v, sym);
        let _ = __torajs_rc_dec(sym);
        if t == TAG_UNDEF || t == TAG_NULL {
            __torajs_throw_type_error(c"Iterator.concat argument is not iterable".as_ptr());
            return false;
        }
        true
    }
}

/// `Iterator.concat(...items)` kernel — the statics wedge packs the
/// arguments into a fresh `Array<Any>` whose reference TRANSFERS
/// here: the minted cell's underlying slot takes it, and the refusal
/// path drops it. Undefined with a pending throw when an item fails
/// the eager iterability check.
///
/// # Safety
/// `items` is a live rc-1 `Array<Any>` heap pointer, ownership
/// transferred by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iterator_concat(items: *mut c_void) -> AnyValue {
    unsafe {
        let len = (items.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        for i in 0..len {
            if !item_is_iterable(__torajs_arr_get_any_boxed(items as *const c_void, i)) {
                __torajs_arr_drop_any(items);
                return VALUE_UNDEFINED;
            }
        }
        let cell = iter_helper_cell_alloc(ITER_HELPER_CONCAT);
        *(cell.add(UNDERLYING_OFF) as *mut u64) =
            crate::nanbox_encode::__torajs_anyv_box_pointer(items);
        crate::nanbox_encode::__torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// One protocol step of a kind-CONCAT cell: drain the current inner
/// iterator, else open the next item through GetIteratorFlattenable.
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
            // Borrowed slot read; the derive answers an owned
            // iterator (self-iterator items get their own inc).
            let item = __torajs_arr_get_any_boxed(items as *const c_void, idx);
            match derive_flattenable(item, false) {
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
