//! The collection constructors' iterable argument — §24.1.1.1 step 7
//! (Map), §24.2.2.1 step 7 (Set), §24.3.1.1 (WeakMap) and §24.4.1.1
//! (WeakSet) are one algorithm with two shapes, so this is one
//! kernel with a `kind` selector rather than four walks.
//!
//! The lowering keeps its static fast lanes (a literal `[[k, v], …]`
//! or a typed source it can see the shape of); everything it cannot
//! see statically — an `any` source, a generator, another collection,
//! a nullish argument — arrives here and is walked with the same
//! `__torajs_any_iter_next` protocol `for (… of …)` uses.
//!
//! Two spec details the shape depends on:
//!
//! - **Nullish adds nothing** (§24.1.1.1 step 6). `new Map(null)` is
//!   not an error and not an empty-iterable walk; it returns before
//!   the iterator is ever requested.
//! - **A Map/WeakMap entry must be an Object** (§24.1.1.2 step 4.d).
//!   `new Map([1, 2])` throws a TypeError, and the iterator gets its
//!   §7.4.11 close before the throw propagates.
//!
//! Ledger: the walk's `out` is OWNED (each step hands over a stake).
//! Entry keys / values come out of `__torajs_any_index_get` owned
//! too. The adder takes its arguments BORROWED, so every stake this
//! kernel takes it also releases.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_null, is_undefined};
use crate::to_primitive::is_object_value;

unsafe extern "C" {
    /// torajs-throw — pending-throw flag (1 = a throw is recorded).
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// Cross-tier — universal NaN-box-safe heap-value release.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-str — flatten a Substr view into a fresh owned Str.
    fn __torajs_substr_to_owned(s: *const u8) -> *mut c_void;
}

/// `FLAG_SUBSTR_INLINE | FLAG_SUBSTR_VIEW` mirror (torajs-str
/// `substr.rs` header bits 0 and 10) — same mirror `coerce.rs`
/// keeps.
const FLAG_SUBSTR_ANY: u16 = (1 << 0) | (1 << 10);

/// Flatten a walked value that turned out to be a Substr view into
/// an owned plain Str, answering every other shape unchanged (with
/// its stake transferred either way).
///
/// A view shares `Tag::Str` but keeps its bytes behind a
/// parent+offset indirection, and the consumers past this point
/// read the plain Str layout: the collections hash and compare a
/// string key by reading `len@8` / `data@16` off the block, so a
/// view stored as a key hashes and compares against its parent
/// POINTER rather than its text and no equal string would ever find
/// it (`new Set("abc").has("b")` answering false). Materializing
/// where a view crosses into a general consumer is the same
/// convention `coerce.rs` follows at the ToString boundary.
unsafe fn flatten_view(v: AnyValue) -> AnyValue {
    if !crate::nanbox::is_cell(v) {
        return v;
    }
    let p = crate::nanbox::as_void_ptr(v);
    // SAFETY: is_cell guarantees a live header.
    let (tag, flags) = unsafe {
        (
            (p.cast::<u8>().add(4) as *const u16).read(),
            (p.cast::<u8>().add(6) as *const u16).read(),
        )
    };
    if tag != torajs_rc::Tag::Str as u16 || flags & FLAG_SUBSTR_ANY == 0 {
        return v;
    }
    let owned = unsafe { __torajs_substr_to_owned(p as *const u8) };
    unsafe { release(v) };
    unsafe { crate::nanbox_encode::__torajs_anyv_box_pointer(owned) }
}

/// Map and WeakMap take entry pairs; Set and WeakSet take values.
fn takes_entries(kind: i64) -> bool {
    use torajs_rc::collection_kind::{COLLECTION_MAP, COLLECTION_WEAKMAP};
    kind == COLLECTION_MAP || kind == COLLECTION_WEAKMAP
}

/// Release an owned AnyValue's stake (immediates are a no-op inside
/// the shared release kernel).
unsafe fn release(v: AnyValue) {
    unsafe { __torajs_value_drop_heap(crate::nanbox::as_void_ptr(v)) };
}

/// Add one walked item to `target` through the receiver's own
/// `set` / `add` — the same dispatch a hand-written `m.set(k, v)`
/// takes, so the weak families' CanBeHeldWeakly key rejection and
/// the collections' value ledger are answered in exactly one place.
///
/// # Safety
/// `target` is a live collection cell; `item` is an owned walked
/// value the caller still owns.
unsafe fn add_one(target: AnyValue, item: AnyValue, kind: i64) {
    unsafe {
        if !takes_entries(kind) {
            let argv = [item];
            // `add` answers the receiver (+1 by the boxed-value
            // convention) — this kernel discards it.
            let out = crate::method_call::__torajs_any_method_call(
                target,
                torajs_rc::ANY_METHOD_ADD,
                core::ptr::null(),
                0,
                core::ptr::null_mut(),
                argv.as_ptr(),
                1,
            );
            release(out);
            return;
        }
        // §24.1.1.2 step 4.d — a non-Object entry is a TypeError, and
        // the caller closes the iterator before it propagates.
        if !is_object_value(item) {
            __torajs_throw_type_error(c"Iterator value is not an entry object".as_ptr());
            return;
        }
        let k = flatten_view(crate::index_any::__torajs_any_index_get(item, 0));
        if __torajs_throw_check() != 0 {
            release(k);
            return;
        }
        let v = crate::index_any::__torajs_any_index_get(item, 1);
        if __torajs_throw_check() != 0 {
            release(k);
            release(v);
            return;
        }
        let argv = [k, v];
        let out = crate::method_call::__torajs_any_method_call(
            target,
            torajs_rc::ANY_METHOD_SET,
            core::ptr::null(),
            0,
            core::ptr::null_mut(),
            argv.as_ptr(),
            2,
        );
        // `set` answers the receiver (+1 by the boxed-value
        // convention) — this kernel discards it.
        release(out);
        release(k);
        release(v);
    }
}

/// See module doc. Answers nothing; an abrupt completion is left as
/// the pending throw the caller's throw-check forwards.
///
/// # Safety
/// `target` is a freshly constructed collection cell of the shape
/// `kind` names; `iterable` is any AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_collection_init_from_iterable(
    target: AnyValue,
    iterable: AnyValue,
    kind: i64,
) {
    // §24.1.1.1 step 6 — a nullish iterable adds nothing, and is not
    // asked for its @@iterator at all.
    if is_null(iterable) || is_undefined(iterable) {
        return;
    }
    let mut idx: i64 = 0;
    let mut iter_slot: AnyValue = VALUE_UNDEFINED;
    let mut item: AnyValue = VALUE_UNDEFINED;
    unsafe {
        loop {
            let has = crate::iter_any::__torajs_any_iter_next(
                iterable,
                &mut idx,
                &mut iter_slot,
                &mut item,
            );
            if __torajs_throw_check() != 0 {
                break;
            }
            if has == 0 {
                break;
            }
            // Flatten in place: `flatten_view` consumes the stake it
            // was handed, so the loop's release must see the value it
            // answered, not the one it retired.
            item = flatten_view(item);
            add_one(target, item, kind);
            release(item);
            item = VALUE_UNDEFINED;
            if __torajs_throw_check() != 0 {
                // §7.4.11 — an abrupt add (or a malformed entry)
                // closes the iterator before propagating.
                crate::iter_any_close::__torajs_any_iter_close(iterable, &mut iter_slot);
                break;
            }
        }
        release(iter_slot);
    }
}
