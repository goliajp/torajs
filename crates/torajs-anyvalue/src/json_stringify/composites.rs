//! The two composite-object writers of [`super`] — the dynobj
//! entry walk and the struct-cell field walk. Split out of the
//! parent under the 500-line file discipline; a child module reaches
//! its parent's private items, so the extern block and the shared
//! helpers stay private with no visibility churn.

use core::ffi::c_void;

use super::struct_expando;
use super::*;
use struct_slot::{SlotEntry, SlotKey, struct_key_listed, struct_slot_key, struct_slot_value};

mod struct_slot;

unsafe extern "C" {
    // torajs-meta — the layout rows in §10.1.11.1 order (integer-index
    // names ascending, then the rest in declaration order), the order
    // `Object.keys` walks them; one table per serialized instance.
    fn __torajs_struct_field_order_fill(layout: *const c_void, n: u32, out: *mut u32) -> u32;
}

/// torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF` mirror (the u32 class tag
/// after the heap header of a `Tag::Obj` cell) — named so a class-tag
/// read-point sweep finds this walk.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// Marks a layout row [`write_struct`] left out of K (a row index never
/// reaches bit 31).
const NOT_IN_K: u32 = 1 << 31;

/// `{...}` — own enumerable entries in §10.1.11.1 order (the print
/// walker's `iter_order` contract); a key whose value serializes to
/// nothing is omitted entirely.
/// §25.5.2.5 — every object not handled by a dedicated arm serializes
/// its enumerable own string-keyed properties, and for a Map, a Set,
/// a RegExp, a Symbol wrapper, … those are the expando bag
/// (`Object.assign(new Map(), {a: 1})` is `{"a":1}`). A cell with no
/// bag is `{}`, as it always was.
///
/// # Safety
/// `ptr` is a live heap cell; `sb` a live string builder.
pub(super) unsafe fn write_bag_cell(sb: *mut c_void, ptr: *mut c_void, depth: u32, st: &St) {
    let bag = unsafe { crate::member_get_layout::__torajs_cell_expando_bag(ptr) };
    if bag.is_null() {
        unsafe { push_bytes(sb, b"{}") };
    } else {
        unsafe { write_object(sb, bag, depth, st) };
    }
}

pub(super) unsafe fn write_object(sb: *mut c_void, ptr: *mut c_void, depth: u32, st: &St) {
    unsafe {
        // §25.5.2.4 step 5 — an array replacer replaces the own-name
        // list wholesale, so that walk takes over entirely.
        if st.property_list.is_some() {
            return replacer::write_object_list(sb, box_void_ptr(ptr), depth, st);
        }
        __torajs_jsb_push_byte(sb, b'{');
        let len = __torajs_dynobj_iter_len(ptr);
        let order_layout = core::alloc::Layout::from_size_align(len as usize * 8, 8).unwrap();
        let order = if len > 0 {
            std::alloc::alloc_zeroed(order_layout) as *mut u64
        } else {
            core::ptr::null_mut()
        };
        let n = __torajs_dynobj_iter_order(ptr, order, len);
        let mut emitted = false;
        for j in 0..n {
            let i = *order.add(j as usize);
            if __torajs_dynobj_iter_flags(ptr, i) & BUCKET_FLAG_ENUMERABLE == 0 {
                continue;
            }
            let key = __torajs_dynobj_iter_key(ptr, i);
            if key.is_null() {
                continue;
            }
            // A replacer body runs mid-walk and may delete this very
            // entry, which frees the key cell the emit below still has
            // to quote — hold it across the call. The value is already
            // a snapshot either way, the same approximation
            // `json_reviver` documents for its own walk.
            let pinned = st.replacer.is_some();
            if pinned {
                crate::payload_rc_inc(4, key as i64);
            }
            emitted |= write_entry(sb, (ptr, i, key), ptr, depth, st, emitted);
            if pinned {
                __torajs_str_drop(key);
            }
            if __torajs_throw_check() != 0 {
                break;
            }
        }
        if !order.is_null() {
            std::alloc::dealloc(order as *mut u8, order_layout);
        }
        if emitted {
            push_indent(sb, depth, st);
        }
        __torajs_jsb_push_byte(sb, b'}');
    }
}

/// The entry `i` of the dict `ptr`, keyed `key` — §25.5.2.2 for the dynobj
/// and struct expando lanes; `holder` is the object the program sees. Answers
/// whether it wrote text: an omitted key leaves the separator state alone.
pub(super) unsafe fn write_entry(
    sb: *mut c_void,
    (ptr, i, key): (*mut c_void, u64, *mut c_void),
    holder: *mut c_void,
    depth: u32,
    st: &St,
    emitted: bool,
) -> bool {
    unsafe {
        let mut value = __torajs_dynobj_iter_value(ptr, i);
        // §25.5.2.2 step 1 — the serialized value is ? Get(holder,
        // key): an accessor entry stores its AccessorPair cell, so
        // run the getter (receiver = the holder, borrowed into the
        // invoke) instead of serializing the pair as an empty
        // object. The result is OWNED (len_get's box_probe_pair
        // convention); a pending throw aborts the walk and
        // propagates through the caller's throw-check.
        let holder = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, holder as i64);
        let mut owned = accessor_pair_of(value).is_some();
        if let Some(pair) = accessor_pair_of(value) {
            value = __torajs_accessor_invoke_getter(pair, holder);
            if __torajs_throw_check() != 0 {
                return false;
            }
        }
        // §25.5.2.3 step 2 — field-level toJSON hook.
        if let Some(r) = crate::json_stringify_tojson::apply_tojson(value) {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            value = r;
            owned = true;
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                return false;
            }
        }
        // §25.5.2.2 step 3 — the replacer answers the value actually
        // serialized, so it runs BEFORE the omit test below: an
        // `undefined` answer is exactly how a replacer drops a key.
        (value, owned) = replacer::apply(st, holder, key, value, owned);
        if __torajs_throw_check() != 0 {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            return false;
        }
        // Probe the value FIRST: an undefined / callable field
        // drops its key, so the separator and key bytes must not
        // be emitted speculatively. Serializing into a scratch
        // builder would cost an alloc per field — instead take
        // the cheap pre-check the split allows.
        if serializes_to_nothing(value) {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            return false;
        }
        if emitted {
            __torajs_jsb_push_byte(sb, b',');
        }
        push_indent(sb, depth + 1, st);
        __torajs_jsb_push_str_quoted(sb, key as *const u8);
        __torajs_jsb_push_byte(sb, b':');
        if st.has_gap() {
            __torajs_jsb_push_byte(sb, b' ');
        }
        write_value(sb, value, depth + 1, st);
        if owned {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
        }
        true
    }
}

/// `{...}` — the Tag::Obj struct-cell twin of [`write_object`]. Reads
/// the instance's `class_tag` (`u32` at `+8`), looks the layout up in
/// the toolchain-emitted `__torajs_class_layouts` table, and walks
/// the fields in declaration order. Undefined / callable field values
/// drop their key (§25.5.2), same three-way split as dynobj. An
/// anonymous struct interned too late to receive a `class_tag`
/// (`class_tag == 0` → NULL layout) serializes as `{}` — matches the
/// same coverage gap `Object.keys(anonAny)` documents.
pub(super) unsafe fn write_struct(sb: *mut c_void, ptr: *mut c_void, depth: u32, st: &St) {
    unsafe {
        // Same §25.5.2.4 step 5 hand-off as the dynobj lane — the
        // list's `Get` reaches declared fields and expandos alike.
        if st.property_list.is_some() {
            return replacer::write_object_list(sb, box_void_ptr(ptr), depth, st);
        }
        __torajs_jsb_push_byte(sb, b'{');
        let mut k_expandos = struct_expando::KeysInK::new();
        let class_tag = (ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read();
        let layout = __torajs_struct_layout_lookup(class_tag);
        let mut emitted = false;
        // §25.5.2 SerializeJSONObject walks EnumerableOwnProperties,
        // and an error instance's `message` / `stack` are `E:false`
        // (§20.5.6.1.1) — a layout field list cannot express that, so
        // the two names are filtered here. `struct_enum::error_prop_skip`
        // is the `Object.keys` twin of this test.
        let is_error = crate::member_get::header_flag(ptr, torajs_rc::FLAG_ERROR);
        if !layout.is_null() {
            let n = __torajs_struct_field_count(layout);
            let mut order = vec![0u32; n as usize];
            let rows = __torajs_struct_field_order_fill(layout, n, order.as_mut_ptr());
            order.truncate(rows as usize);
            // §25.5.2.4 step 6 fixes K before step 8 reads a value, so
            // a getter that deletes a later member leaves its key in
            // the walk. While the redefine and absent bits are clear
            // every declared member is an own enumerable property, so K
            // is the layout; an instance already moved takes a first
            // pass over the rows, which runs no user code, to mark the
            // ones K leaves out.
            let mut scan = crate::member_get::header_flag(
                ptr,
                torajs_rc::FLAG_OBJ_EXOTIC_FIELD | torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD,
            );
            'walk: loop {
                for row in order.iter_mut() {
                    let i = *row & !NOT_IN_K;
                    if !scan {
                        // a computed field declared before this row
                        struct_expando::write_struct_expandos(
                            sb,
                            ptr,
                            depth,
                            st,
                            &mut emitted,
                            (struct_expando::before_row(i), &mut k_expandos),
                        );
                        if __torajs_throw_check() != 0 {
                            break 'walk;
                        }
                        if *row & NOT_IN_K != 0 {
                            continue;
                        }
                    }
                    let Some(k) = struct_slot_key(ptr, layout, i, is_error) else {
                        continue;
                    };
                    if scan {
                        if !struct_key_listed(ptr, &k) {
                            *row |= NOT_IN_K;
                        }
                        continue;
                    }
                    if write_struct_row(sb, ptr, k, depth, st, &mut emitted) {
                        break 'walk;
                    }
                }
                if !scan {
                    break;
                }
                scan = false;
            }
        }
        // the expando entries: the ones declared after the last row,
        // then the rest in insertion order
        struct_expando::write_struct_expandos(
            sb,
            ptr,
            depth,
            st,
            &mut emitted,
            (struct_expando::TAIL, &mut k_expandos),
        );
        struct_expando::write_struct_expandos(
            sb,
            ptr,
            depth,
            st,
            &mut emitted,
            (struct_expando::REST, &mut k_expandos),
        );
        if emitted {
            push_indent(sb, depth, st);
        }
        __torajs_jsb_push_byte(sb, b'}');
    }
}

/// Serialize one row of K whose key `k` the walk has taken: its value
/// through toJSON and the replacer, then the `"name":value` entry
/// unless the value serializes to nothing. True when a step threw and
/// the walk has to stop.
#[inline(always)]
unsafe fn write_struct_row(
    sb: *mut c_void,
    ptr: *mut c_void,
    k: SlotKey,
    depth: u32,
    st: &St,
    emitted: &mut bool,
) -> bool {
    unsafe {
        let (name, mut value, is_accessor) = match struct_slot_value(ptr, k) {
            SlotEntry::Threw => return true,
            SlotEntry::Emit { key, value, owned } => (key, value, owned),
        };
        // §25.5.2.3 step 2 — field-level toJSON hook (a
        // struct's Any field can hold a dynobj carrying one).
        // An accessor's [[Get]] result arrives OWNED — the
        // same ledger the hook result rides.
        let mut hook_owned = is_accessor;
        if let Some(r) = crate::json_stringify_tojson::apply_tojson(value) {
            if hook_owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            value = r;
            hook_owned = true;
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                return true;
            }
        }
        // §25.5.2.2 step 3 — the field name lives in the
        // static layout table as raw bytes, so the call site
        // mints the Str the replacer's `key` argument needs.
        (value, hook_owned) = replacer::apply_named(
            st,
            crate::nanbox::box_void_ptr(ptr),
            name.ptr,
            name.len,
            value,
            hook_owned,
        );
        if __torajs_throw_check() != 0 {
            if hook_owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            return true;
        }
        if serializes_to_nothing(value) {
            if hook_owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            return false;
        }
        if *emitted {
            __torajs_jsb_push_byte(sb, b',');
        }
        *emitted = true;
        push_indent(sb, depth + 1, st);
        quote_bytes(sb, name.ptr, name.len);
        __torajs_jsb_push_byte(sb, b':');
        if st.has_gap() {
            __torajs_jsb_push_byte(sb, b' ');
        }
        write_value(sb, value, depth + 1, st);
        if hook_owned {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
        }
        if __torajs_throw_check() != 0 {
            return true;
        }
        false
    }
}
