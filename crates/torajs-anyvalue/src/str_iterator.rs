//! §22.1.3.36 `String.prototype[Symbol.iterator]` — the string leg
//! of the F0 `@@iterator` builtin reify (RFC
//! 20260728-gen-forof-yieldstar).
//!
//! Unlike Array/Map/Set there is no named prototype alias to ride,
//! so the reified cell carries the dedicated
//! [`torajs_rc::ANY_METHOD_STR_ITERATOR`] id and this module is its
//! [[Call]] body. The spec body is receiver-generic —
//! `ToString(RequireObjectCoercible(this))` — so a number receiver
//! iterates its decimal image; the dispatcher's nullish guard is the
//! RequireObjectCoercible step.
//!
//! tr has no StringIterator substrate; the mint materializes the
//! character array (one fresh Str per UTF-16 code unit — the
//! any-lane for-of tier's documented deviation from §22.1.5.1's
//! per-code-point walk) and answers a VALUES-kind ArrIter over it
//! carrying the string family word, so it still names
//! %StringIteratorPrototype% when asked what it inherits from,
//! whose `next()` / for-of step faces the any lane already serves.
//! The iterator holds the array's only reference; exhaustion latches
//! per §23.1.5.2.1 and frees it.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_pointer;
use crate::nanbox_ffi::__torajs_anyv_to_str;

/// `AnySlotTag::Heap` in the Array<Any> slot protocol.
const TAG_HEAP: u64 = 4;

/// torajs-str Str layout — u32 length at +8.
const STR_LEN_OFF: usize = 8;

unsafe extern "C" {
    /// torajs-str — fresh rc-1 single-code-unit Str at index `i`.
    fn __torajs_str_at(s: *const u8, i: i64) -> *mut u8;
    /// torajs-arr — fresh rc-1 `Array<Any>` block.
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-arr — append one NaN-box slot pair (transfer: no inc).
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    /// torajs-arr — §22.1.5.1 VALUES-kind ArrIter mint (rc-incs the
    /// source). The `_string` face is the same cell with its family
    /// word set, which is what makes it answer
    /// %StringIteratorPrototype% rather than %ArrayIteratorPrototype%.
    fn __torajs_arr_iter_create_values_string(arr: *mut c_void) -> *mut c_void;
    /// torajs-rc — universal heap-header decrement.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_str_drop(s: *mut c_void);
}

/// The reified cell's [[Call]] body — see module doc. Answers an
/// owned ArrIter AnyValue; `undefined` when ToString recorded a
/// pending throw (a Symbol receiver).
///
/// # Safety
/// `recv` carries a valid AnyValue bit pattern.
pub(crate) unsafe fn str_iterator_mint(recv: AnyValue) -> AnyValue {
    // §22.1.3.36 step 2 — ToString(this). Owned fresh/inc'd Str;
    // NULL = the kernel recorded a pending throw.
    let s = unsafe { __torajs_anyv_to_str(recv) };
    if s.is_null() {
        return VALUE_UNDEFINED;
    }
    let len = unsafe { (s.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() } as i64;
    let mut arr = unsafe { __torajs_arr_alloc_any(len.max(0) as u64) };
    for i in 0..len {
        // Fresh rc-1 Str hands its reference to the slot (push_any
        // is transfer-shaped).
        let ch = unsafe { __torajs_str_at(s.cast::<u8>(), i) };
        arr = unsafe { __torajs_arr_push_any(arr.cast::<c_void>(), TAG_HEAP, ch as u64) };
    }
    unsafe { __torajs_str_drop(s) };
    let it = unsafe { __torajs_arr_iter_create_values_string(arr.cast::<c_void>()) };
    // create rc-inc'd the source (1 → 2); hand our mint reference
    // over so the iterator is the array's only owner. Cannot reach
    // zero — no drop walk needed.
    unsafe { __torajs_rc_dec(arr.cast::<c_void>()) };
    // Transfer the iterator's rc into the box — owned result.
    unsafe { __torajs_anyv_box_pointer(it) }
}
