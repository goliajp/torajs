//! §25.5.2.3 SerializeJSONProperty step 2 — the user `toJSON` hook
//! (rotation 205, split from `json_stringify.rs`): when the value is
//! a DynObj carrying a callable own `toJSON` entry, call it with the
//! object as the receiver and serialize the RESULT instead. Every
//! consumption site (top-level wrapper, object field walk, array
//! element walk) applies the hook once; the replacement then walks
//! ordinarily, so a hook answering a cyclic structure lands on the
//! recursion cap's TypeError like any other cycle.
//!
//! Recorded boundaries: the spec's `key` argument is not passed
//! (callers that read it are rare); struct-lane cells and BigInt
//! receivers don't consult the hook (the dynobj lane is where
//! test262's `obj.toJSON = fn` expando shape lands).

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};
use crate::prop_has::key_bytes;

/// `torajs_arr::layout::ARR_PROPS_OFF` mirror — the expando props
/// dynobj slot.
const ARR_PROPS_OFF: usize = 24;

unsafe extern "C" {
    /// torajs-date §21.4.4.37 — the ISO string, or NULL for an
    /// invalid date (whose `toJSON` answers JS `null`).
    fn __torajs_date_to_json(d: *const c_void) -> *mut u8;

    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;

    fn __torajs_str_alloc_ascii(p: *const u8, n: i64) -> *mut u8;
}

/// The interned `"toJSON"` key the struct arm reads with. Minted
/// once and never released — every consumer only READS it (same
/// posture as the iterator protocol's `next` / `return` keys).
static NAME_TO_JSON: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

/// The struct arm's own §25.5.2.3 step 2 probe: `Get(value,
/// "toJSON")` over a NOMINAL cell, which reaches a layout field and a
/// class method alike.
///
/// A struct used to answer "no hook" unconditionally — the module's
/// recorded boundary said the dynobj lane was where the expando shape
/// lands, which is true and was never the whole story:
/// `class C { toJSON() { return 9 } }` serialized as its FIELDS in
/// every lane, and so did an object literal whose `toJSON` types
/// `any`. Neither refuses anything; both answer a wrong document.
///
/// # Safety
/// `recv` is a live AnyValue holding the struct cell.
unsafe fn struct_tojson_entry(recv: AnyValue) -> Option<AnyValue> {
    unsafe {
        let mut key = NAME_TO_JSON.load(Ordering::Acquire);
        if key.is_null() {
            let fresh = __torajs_str_alloc_ascii(b"toJSON".as_ptr(), 6);
            key = match NAME_TO_JSON.compare_exchange(
                core::ptr::null_mut(),
                fresh,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => fresh,
                Err(winner) => {
                    crate::__torajs_str_drop(fresh as *mut c_void);
                    winner
                }
            };
        }
        let k = key as *const c_void;
        // Only a heap value can be callable, so a miss, a primitive
        // and an accessor sentinel all mean "walk the fields".
        if crate::member_get::__torajs_any_member_get_tag(recv, k) != 4 {
            return None;
        }
        let payload = crate::member_get_value::__torajs_any_member_get_value(recv, k);
        if payload == 0 {
            return None;
        }
        Some(crate::nanbox_encode::__torajs_anyv_box_from_pair(
            4,
            payload as i64,
        ))
    }
}

/// Apply the toJSON hook to `v` if it is a DynObj with a callable
/// own `toJSON`. `Some(result)` is OWNED (the invoke convention) —
/// the caller serializes it and releases; `None` means no hook, walk
/// `v` as-is. A pending throw from the hook body surfaces through
/// the caller's throw-check with `Some(VALUE_UNDEFINED)`-shaped
/// result — callers check the throw flag right after.
pub(crate) unsafe fn apply_tojson(v: AnyValue) -> Option<AnyValue> {
    unsafe {
        if !is_cell(v) {
            return None;
        }
        let ptr = as_void_ptr(v);
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        // A Date's `toJSON` is a builtin, and it has to run HERE
        // rather than only in the serializer's own Date arm: §25.5.2.2
        // hands the REPLACER the step-2 result, and bun agrees —
        // `JSON.stringify({d: new Date(0)}, (k, v) => v)` sees the ISO
        // string in `v`, not the Date. The serializer's arm stays as
        // the answer for a Date that never passes a property site.
        if tag == Tag::Date as u16 {
            let iso = __torajs_date_to_json(ptr);
            return Some(if iso.is_null() {
                crate::nanbox::VALUE_NULL
            } else {
                crate::nanbox::box_void_ptr(iso.cast())
            });
        }
        // A DynObj consults its own entries; an Arr consults its
        // expando props dynobj (`arr.toJSON = fn` lands there —
        // torajs_arr::layout::ARR_PROPS_OFF slot at +24).
        // A nominal struct keeps its properties in a layout, not in a
        // props bag, so the iteration below cannot see them. Ask the
        // ordinary member read instead — it reaches a layout field
        // and a class method alike.
        let entry = if tag == Tag::Obj as u16 {
            match struct_tojson_entry(v) {
                Some(e) => Some(e),
                None => return None,
            }
        } else {
            None
        };
        let holder = if entry.is_some() {
            core::ptr::null()
        } else if tag == Tag::DynObj as u16 {
            ptr as *const c_void
        } else if tag == Tag::Arr as u16 {
            let props = (ptr.cast::<u8>().add(ARR_PROPS_OFF) as *const *const c_void).read();
            if props.is_null() {
                return None;
            }
            props
        } else {
            return None;
        };
        let entry = match entry {
            Some(e) => e,
            None => find_tojson_entry(holder)?,
        };
        if !is_cell(entry) {
            return None;
        }
        let this_box = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, ptr as i64);
        if let Some(target) = crate::method_call_closure::call_target(as_void_ptr(entry)) {
            return Some(crate::method_call_closure::dispatch(
                &target,
                this_box,
                core::ptr::null(),
                0,
            ));
        }
        // §25.5.2.3 step 2 asks IsCallable, and `call_target`
        // answers a narrower question: which closure-shaped lane
        // drives this cell. The other two shapes §7.2.3 names have
        // no such lane, so they were read as "no hook" — and that
        // does not refuse the program, it serializes the hook
        // itself: `JSON.stringify({ toJSON: new Proxy(fn, {}) })`
        // answered `{"toJSON":{}}` where every engine answers what
        // the hook returns. A wrong document, from a program that
        // compiled and ran.
        //
        // The explicit-`this` kernel holds all three shapes (it
        // runs the `apply` trap for a proxy), so the fallback is
        // that one. The lane above is untouched, so no hook that
        // works today changes route.
        if !crate::construct::is_callable(entry) {
            return None;
        }
        Some(
            crate::method_call_closure_dispatch::__torajs_any_call_with_this(
                entry,
                this_box,
                core::ptr::null(),
                0,
            ),
        )
    }
}

/// The own `toJSON` entry's value, by key-byte comparison over the
/// live insertion-order walk (`iter_order` filters tombstones — a
/// raw index scan would resurrect a deleted `toJSON`). No Str cell
/// to mint, and serialization is already O(entries).
unsafe fn find_tojson_entry(obj: *const c_void) -> Option<AnyValue> {
    unsafe {
        let len = __torajs_dynobj_iter_len(obj);
        if len == 0 {
            return None;
        }
        let layout = core::alloc::Layout::from_size_align(len as usize * 8, 8).ok()?;
        let order = std::alloc::alloc_zeroed(layout) as *mut u64;
        let n = __torajs_dynobj_iter_order(obj, order, len);
        let mut found = None;
        for j in 0..n {
            let i = *order.add(j as usize);
            let key = __torajs_dynobj_iter_key(obj, i);
            if key.is_null() {
                continue;
            }
            if key_bytes(key).as_bytes() == b"toJSON" {
                found = Some(__torajs_dynobj_iter_value(obj, i));
                break;
            }
        }
        std::alloc::dealloc(order as *mut u8, layout);
        found
    }
}
