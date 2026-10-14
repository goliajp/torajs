//! DynObj implicit-set — `obj.x = v` + object-literal init.
//!
//! Implements spec §10.1.5.1 OrdinarySet → §10.1.6.2 CreateDataProperty
//! for the "writable=true" path; throws TypeError when overwriting a
//! non-writable entry (`__torajs_throw_type_error` records pending +
//! returns; caller's ssa-lower-side `emit_throw_check` propagates).
//!
//! Fresh inserts append to the dense entry array (insertion order) and
//! point the probed index slot (first tombstone, else first empty) at
//! the new entry: rc-bump the key (entry owns its share) + write
//! default flags (writable / enumerable / configurable all true) —
//! except on a builtin `<Ctor>.prototype` singleton, where a key the
//! prototype already owns synthetically is an OVERWRITE per §10.1.9.2
//! step 2 and keeps the method attributes {W:1, E:0, C:1}.
//! Existing entry overwrite: drop the old heap value if ANY_HEAP,
//! preserve the existing flag bits, swap only the NaN-box value.

use core::ffi::c_void;

use crate::nanbox;

use crate::accessor::{__torajs_accessor_invoke_setter, TAG_ACCESSOR_PAIR};
use crate::get::type_tag;
use crate::global_cell::{TAG_GLOBAL_CELL, global_cell_write};
use crate::layout::{
    ANY_HEAP, BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_WRITABLE, BUCKET_FLAGS_DEFAULT,
    BUCKET_TAG_MASK, DYNOBJ_HDR_FLAG_NON_EXTENSIBLE,
};
use crate::probe::{
    Entry, Probe, bucket_flags, bucket_make_key_tagged, count, entries, entries_cap, entries_len,
    index_ptr, key_str_bytes, probe, set_count, set_entries_len,
};
use crate::resize::resize;

unsafe extern "C" {
    /// Cross-tier — torajs-rc's refcount inc. Entry takes ownership
    /// of the key string on fresh insert.
    fn __torajs_rc_inc(p: *mut c_void);

    /// Cross-tier — torajs-throw's TypeError thrower. Records pending
    /// throw via TLS + returns normally; caller MUST explicitly
    /// `return;` after invoking (per `feedback_throw_extern_returns_void`).
    fn __torajs_throw_type_error(msg: *const u8);

    /// Cross-tier — heap-value drop dispatch (NaN-box-safe; immediate
    /// AnyValues are filtered by the 7d-A cell gate). Drops the old
    /// entry value when overwriting.
    fn __torajs_value_drop_heap(child: *mut c_void);

    /// torajs-rc — builtin-prototype-singleton own-write note (RFC
    /// 20260721 刀 11 G13): marks the (proto tag, mid) patch bit the
    /// any-method dispatcher's primitive fast arms pre-gate on.
    fn __torajs_builtin_proto_note_own_write(obj: *const c_void, name: *const u8, len: i64);

    /// torajs-anyvalue — non-zero when `obj` is a builtin
    /// `<Ctor>.prototype` singleton and `key` names a method it owns
    /// (the same probe gOPD synthesizes its descriptor from). Used
    /// only to pick the attributes of a fresh insert; the cell it
    /// answers is ignored here.
    fn __torajs_builtin_proto_own_method_cell(obj: *const c_void, key: *const c_void) -> u64;
}

/// `__torajs_dynobj_set(obj_slot, key, tag, value)` — implicit-set entry.
///
/// # Safety
/// `obj_slot` is non-NULL and points at a live `*mut c_void` holding
/// a dynobj or NULL. `key` is a live Str heap pointer. Caller must
/// check for pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) {
    unsafe { dynobj_set_impl(obj_slot, key, tag, value, true, false) };
}

/// §28.1.13 Reflect.set flavor — a [[Set]] refusal (readonly entry /
/// setter-less accessor / non-extensible fresh key) answers `0`
/// instead of recording the strict-assignment TypeError; a handled
/// write answers `1`. Same R3-style parameterization as
/// `__torajs_any_prop_delete_soft`.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_set`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_soft(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) -> i64 {
    unsafe { dynobj_set_impl(obj_slot, key, tag, value, false, false) }
}

/// Flavor-carrying core of the two shells above — `throw_on_refusal`
/// picks §13.15.2 strict-assignment (record the TypeError) or
/// §28.1.13 Reflect.set (answer false). Both flavors return the
/// success i64 (1 = written / setter ran, 0 = refused).
///
/// `own_only` is [`__torajs_dynobj_set_own`]'s flavor: an absent key
/// writes nothing and answers [`DYNOBJ_SET_OWN_MISS`]. One body serves
/// all three entries.
#[inline(never)]
unsafe fn dynobj_set_impl(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
    own_only: bool,
) -> i64 {
    let obj = unsafe { *obj_slot };
    if obj.is_null() {
        return 1;
    }
    let mut pr = unsafe { probe(obj, key as *const c_void) };
    if !pr.found && own_only {
        return DYNOBJ_SET_OWN_MISS;
    }
    unsafe { note_own_write(obj, key) };
    if pr.found {
        unsafe {
            write_own_entry(
                obj,
                entries(obj).add(pr.entry as usize),
                tag,
                value,
                throw_on_refusal,
            )
        }
    } else {
        // §10.1.5.1 [[Set]] on a non-extensible dict must reject new
        // keys in strict mode (tr modules are all strict). Mirrors
        // `__torajs_dynobj_define`'s fresh-insert gate so
        // `Object.preventExtensions(o); o.newKey = 5;` throws with
        // bun-parity wording — implicit-set was previously silently
        // creating the entry (7 test262 Object/preventExtensions/
        // 15.2.3.10-3-{2,5-1,6,7,12,15,16,17}.js).
        let header_flags = unsafe { *(obj.cast::<u8>().add(6) as *const u16) };
        if header_flags & DYNOBJ_HDR_FLAG_NON_EXTENSIBLE != 0 {
            if throw_on_refusal {
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to define property on object that is not extensible.".as_ptr()
                            as *const u8,
                    );
                }
            }
            return 0;
        }
        // §10.1.9.2 step 2 — assigning over an EXISTING own data
        // property keeps its attributes. A builtin prototype's methods
        // are own properties that live in no dict (the synthesized
        // surface `proto_tag_owns` answers reads and gOPDs from), so
        // this insert IS that overwrite and must land with the method
        // attributes {W:1, E:0, C:1} rather than a plain assignment's
        // enumerable default. Without this `Map.prototype.get = f`
        // made `get` show up in `Object.keys(Map.prototype)` and
        // for-in — a monkey-patch is not supposed to be visible there.
        let attrs =
            if unsafe { __torajs_builtin_proto_own_method_cell(obj, key as *const c_void) } != 0 {
                BUCKET_FLAG_WRITABLE | BUCKET_FLAG_CONFIGURABLE
            } else {
                BUCKET_FLAGS_DEFAULT
            };
        // Dense-array-full guard: compact (and grow if genuinely full)
        // so the insert has an append slot. Resize swaps the store
        // inside the header cell — `obj` itself stays valid (RFC
        // 20260809-dynobj-store-split) — and rehashes, so the slot is
        // probed again.
        if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
            unsafe {
                resize(obj);
                pr = probe(obj, key as *const c_void);
            }
        }
        // Fresh insert: append to the dense array (insertion order),
        // point the probed slot (tombstone reuse or empty) at it.
        let e_idx = unsafe { entries_len(obj) };
        unsafe {
            __torajs_rc_inc(key);
            *entries(obj).add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, attrs),
                value_anyv: nanbox::box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64),
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
        1
    }
}

/// `__torajs_dynobj_set_own`'s answer when `key` has no own entry:
/// nothing was written and the caller still holds its `+1` on a heap
/// payload.
pub const DYNOBJ_SET_OWN_MISS: i64 = -1;

/// §10.1.9.2 OrdinarySet when the receiver owns `key`: the own entry
/// decides alone (step 2 never consults the prototype chain), so one
/// probe both finds the entry and writes it. An accessor entry runs
/// its setter and a non-writable one refuses, exactly as
/// [`__torajs_dynobj_set`] does. A miss writes nothing and answers
/// [`DYNOBJ_SET_OWN_MISS`]: the caller then takes the paths an
/// absent key needs (`__proto__`, the inherited setter walk, the
/// extensibility gate, the insert).
///
/// # Safety
/// `obj` is a live dynobj; `key` is a live Str or Symbol cell; on a
/// hit, a heap payload's `+1` moves into the entry (or is dropped on
/// a refusal / setter call), and the caller checks for a pending
/// throw.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_own(
    obj: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    let mut slot = obj;
    unsafe { dynobj_set_impl(&mut slot, key, tag, value, throw_on_refusal, true) }
}

/// A write landing on a builtin `<Ctor>.prototype` singleton is a
/// monkey-patch — note it for the fast-arm pre-gate (RFC 20260721
/// 刀 11 G13; one relaxed load when no singleton exists). The gate
/// keys off the method NAME, so a symbol key has nothing to report
/// and must not have the Str payload offsets read off its 16-byte
/// cell.
///
/// # Safety
/// `obj` is a live dynobj; `key` is a live Str or Symbol cell.
pub(crate) unsafe fn note_own_write(obj: *mut c_void, key: *mut c_void) {
    if let Some((data, len, true)) = unsafe { key_str_bytes(key) } {
        unsafe { __torajs_builtin_proto_note_own_write(obj, data, len as i64) };
    }
}

/// The write into an entry the probe found: an accessor runs its
/// setter, a non-writable entry refuses, a global-variable cell
/// writes through to its binding, and a data entry swaps its value
/// keeping its flag bits. Answers 1 = written / setter ran, 0 =
/// refused.
///
/// # Safety
/// `e` is a live entry of `obj`; `(tag, value)` carries the caller's
/// `+1` on a heap payload.
#[inline]
unsafe fn write_own_entry(
    obj: *mut c_void,
    e: *mut Entry,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    let cur_value_anyv = unsafe { (*e).value_anyv };
    // The current value's heap type tag, read once for the accessor
    // and global-cell checks and the drop below; an immediate has none.
    let cur_kind = if nanbox::is_cell(cur_value_anyv) {
        Some(unsafe { type_tag(cur_value_anyv as *const c_void) })
    } else {
        None
    };
    // RFC C3 — accessor entry: dispatch the setter, never a data
    // write (checked before the writable gate — accessors carry no
    // writable bit, so the data path would wrongly throw "read
    // only"). `cur_value_anyv` is the AccessorPair cell verbatim.
    if cur_kind == Some(TAG_ACCESSOR_PAIR) {
        let value_anyv = nanbox::box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64);
        // Receiver = the dynobj itself, boxed — an ACC_KIND_RECV
        // fn-expr face reads it as `this`; other faces ignore it.
        let recv_anyv = nanbox::box_from_pair(4, obj as i64);
        let ran = unsafe {
            __torajs_accessor_invoke_setter(cur_value_anyv as *const c_void, recv_anyv, value_anyv)
        };
        // the setter borrows the value; the caller's +1 ends here
        unsafe { drop_heap_payload(tag, value) };
        if ran == 0 {
            if throw_on_refusal {
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempted to assign to readonly property.".as_ptr() as *const u8,
                    );
                }
            }
            return 0;
        }
        return 1;
    }
    let cur_flags = bucket_flags(unsafe { (*e).key_ptr_tagged });
    if cur_flags & BUCKET_FLAG_WRITABLE == 0 {
        unsafe { drop_heap_payload(tag, value) };
        if throw_on_refusal {
            unsafe {
                __torajs_throw_type_error(
                    c"Attempted to assign to readonly property.".as_ptr() as *const u8
                );
            }
        }
        return 0;
    }
    // RFC 20260912-global-var-cell — the entry is a top-level
    // binding's slot: the write lands IN the binding. A typed
    // slot that cannot spell the value refuses loudly rather
    // than storing bits every bare read after it would misread.
    match cur_kind {
        Some(TAG_GLOBAL_CELL) => {
            let cell = cur_value_anyv as *const c_void;
            if unsafe { global_cell_write(cell, tag & BUCKET_TAG_MASK, value) } {
                return 1;
            }
            if throw_on_refusal {
                unsafe {
                    __torajs_throw_type_error(
                        c"Cannot store a value of this type into a typed global variable binding through the global object (not yet supported)."
                            .as_ptr() as *const u8,
                    );
                }
            }
            return 0;
        }
        // Drop the old value if it is a heap cell. A ShortStr also
        // decodes to ANY_HEAP, but it is an immediate the drop's own
        // cell gate turns away, so the cell test alone decides.
        Some(_) => unsafe { __torajs_value_drop_heap(cur_value_anyv as *mut c_void) },
        None => {}
    }
    // Preserve existing flag bits in key_ptr_tagged; rebox the
    // (tag, value) pair into a fresh NaN-box AnyValue.
    unsafe {
        (*e).value_anyv = nanbox::box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64);
    }
    1
}

/// Object-literal-init set — the narrow kernel the lowering emits
/// for a literal with no accessor members (RFC
/// 20260825-inject-narrow-define 刀 3). The receiver is the fresh
/// dynobj the literal just allocated, so every slow arm of
/// [`dynobj_set_impl`] is compile-time unreachable: it cannot be a
/// builtin `<Ctor>.prototype` singleton (no note, no synthetic-own
/// attrs), cannot be non-extensible or frozen, and — because the
/// lowering only selects this kernel when the literal has no
/// getter/setter members — no probed entry can hold an
/// AccessorPair. A duplicate data key (`{a: 1, a: 2}`) is the one
/// found-path this keeps: drop the old value, keep the flags.
///
/// Those arms are exactly what link-rooted the accessor-invoke and
/// dispatch worlds from every program with an object literal.
///
/// r503 — and the duplicate-key found path is the one edge from
/// every literal into the generic heap drop (`{a: 1, a: 2}` drops
/// the first value), which roots the cycle / weak worlds. The
/// lowering knows a literal's static keys, so it proves a key
/// fresh — first of its name, no computed key or spread before it —
/// and stores it here, insert-only; every other key stores through
/// [`__torajs_dynobj_set_fresh_dup`], which keeps the found path.
///
/// # Safety
/// `obj_slot` points at the literal's live init slot (fresh
/// `TAG_DYNOBJ`, never NULL); `key` is a live Str cell the table
/// does not yet hold; on `tag == ANY_HEAP` the entry takes the
/// caller's transferred reference to `value`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_fresh(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) {
    unsafe {
        let obj = fresh_receiver(obj_slot);
        let pr = probe(obj, key as *const c_void);
        debug_assert!(
            !pr.found,
            "set_fresh contract: a key the lowering could not prove fresh stores through set_fresh_dup"
        );
        fresh_insert(obj, key, tag, value, &pr);
    }
}

/// [`__torajs_dynobj_set_fresh`] for a key the lowering cannot prove
/// fresh: a repeated literal key, or any key after a computed key or
/// a spread. Last write wins (§13.2.5.5 evaluation order); the flags
/// stay the data defaults the first write stamped.
///
/// # Safety
/// As [`__torajs_dynobj_set_fresh`], minus the fresh-key clause.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set_fresh_dup(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) {
    unsafe {
        let obj = fresh_receiver(obj_slot);
        let pr = probe(obj, key as *const c_void);
        if pr.found {
            let e = entries(obj).add(pr.entry as usize);
            let cur = (*e).value_anyv;
            if nanbox::unbox_tag(cur) as u64 == ANY_HEAP {
                __torajs_value_drop_heap(cur as *mut c_void);
            }
            (*e).value_anyv = nanbox::box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64);
            return;
        }
        fresh_insert(obj, key, tag, value, &pr);
    }
}

/// The literal's fresh dynobj, grown ahead of the insert the caller
/// is about to make (a grow must precede the probe: the slot index
/// is capacity-relative).
///
/// # Safety
/// `obj_slot` per the set_fresh contract.
unsafe fn fresh_receiver(obj_slot: *mut *mut c_void) -> *mut c_void {
    unsafe {
        let obj = *obj_slot;
        debug_assert_eq!(
            (obj.cast::<u8>().add(4) as *const u16).read(),
            crate::layout::TAG_DYNOBJ,
            "set_fresh contract: receiver is the literal's fresh dynobj"
        );
        if entries_len(obj) == entries_cap(obj) {
            resize(obj);
        }
        obj
    }
}

/// Append a new data entry at the probe's insertion slot.
///
/// # Safety
/// `pr` is a not-found probe of `key` against `obj` with room for
/// one more entry.
unsafe fn fresh_insert(obj: *mut c_void, key: *mut c_void, tag: u64, value: u64, pr: &Probe) {
    unsafe {
        let e_idx = entries_len(obj);
        __torajs_rc_inc(key);
        *entries(obj).add(e_idx as usize) = Entry {
            key_ptr_tagged: bucket_make_key_tagged(key, BUCKET_FLAGS_DEFAULT),
            value_anyv: nanbox::box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64),
        };
        *index_ptr(obj).add(pr.slot as usize) = e_idx;
        set_entries_len(obj, e_idx + 1);
        set_count(obj, count(obj) + 1);
    }
}

/// Release the caller's +1 on a heap payload the set did not store.
unsafe fn drop_heap_payload(tag: u64, value: u64) {
    if tag & BUCKET_TAG_MASK == ANY_HEAP && value != 0 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::{__torajs_dynobj_alloc, free_dynobj_blocks};
    use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};
    use crate::probe::KEY_STR_FLAG_IS_LATIN1;

    /// A Latin-1 Str cell (header flags `IS_LATIN1`), as the probe reads it.
    fn make_str(s: &str) -> Vec<u64> {
        let mut v = vec![0u64; (STR_DATA_OFF + s.len()).div_ceil(8)];
        unsafe {
            let p = v.as_mut_ptr() as *mut u8;
            *(p.add(6) as *mut u16) = KEY_STR_FLAG_IS_LATIN1;
            *(p.add(STR_LEN_OFF) as *mut u64) = s.len() as u64;
            core::ptr::copy_nonoverlapping(s.as_ptr(), p.add(STR_DATA_OFF), s.len());
        }
        v
    }

    /// The fresh insert minus the key retain (the tests own their key
    /// buffers).
    unsafe fn insert(obj: *mut c_void, key: *mut c_void, flags: u64, value_anyv: u64) {
        unsafe {
            let pr = probe(obj, key);
            assert!(!pr.found);
            let e_idx = entries_len(obj);
            *entries(obj).add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, flags),
                value_anyv,
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }

    unsafe fn value_of(obj: *mut c_void, key: *mut c_void) -> u64 {
        unsafe {
            let pr = probe(obj, key);
            assert!(pr.found);
            (*entries(obj).add(pr.entry as usize)).value_anyv
        }
    }

    /// An absent key writes nothing and leaves the payload with the caller.
    #[test]
    fn set_own_miss_writes_nothing() {
        let k = make_str("k");
        unsafe {
            let obj = __torajs_dynobj_alloc();
            let r = __torajs_dynobj_set_own(obj, k.as_ptr() as *mut c_void, 2, 7, true);
            assert_eq!(r, DYNOBJ_SET_OWN_MISS);
            assert_eq!(entries_len(obj), 0);
            assert_eq!(count(obj), 0);
            free_dynobj_blocks(obj);
        }
    }

    /// A writable own entry takes the new value in place, flags kept.
    #[test]
    fn set_own_hit_overwrites_in_place() {
        let ka = make_str("a");
        let kb = make_str("b");
        unsafe {
            let obj = __torajs_dynobj_alloc();
            let a = ka.as_ptr() as *mut c_void;
            let b = kb.as_ptr() as *mut c_void;
            insert(obj, a, BUCKET_FLAGS_DEFAULT, nanbox::box_from_pair(2, 1));
            insert(obj, b, BUCKET_FLAGS_DEFAULT, nanbox::box_from_pair(2, 2));
            assert_eq!(__torajs_dynobj_set_own(obj, b, 2, 9, true), 1);
            assert_eq!(value_of(obj, b), nanbox::box_from_pair(2, 9));
            assert_eq!(value_of(obj, a), nanbox::box_from_pair(2, 1));
            assert_eq!(entries_len(obj), 2);
            let flags = bucket_flags((*entries(obj).add(1)).key_ptr_tagged);
            assert_eq!(flags, BUCKET_FLAGS_DEFAULT);
            free_dynobj_blocks(obj);
        }
    }

    /// A non-writable own entry refuses (soft flavor: no throw) and
    /// keeps its value.
    #[test]
    fn set_own_hit_readonly_refuses() {
        let k = make_str("ro");
        unsafe {
            let obj = __torajs_dynobj_alloc();
            let kp = k.as_ptr() as *mut c_void;
            let before = nanbox::box_from_pair(2, 3);
            insert(obj, kp, BUCKET_FLAG_CONFIGURABLE, before);
            assert_eq!(__torajs_dynobj_set_own(obj, kp, 2, 4, false), 0);
            assert_eq!(value_of(obj, kp), before);
            free_dynobj_blocks(obj);
        }
    }

    /// The one-probe read answers the stored pair, an entry storing
    /// undefined as a hit `(5, 0)`, and an absent key as a miss that
    /// leaves the out slot alone.
    #[test]
    fn get_own_reads_the_own_pair() {
        use crate::get::{__torajs_dynobj_get_own, DYNOBJ_GET_OWN_MISS};
        let ka = make_str("a");
        let kb = make_str("b");
        let kc = make_str("c");
        unsafe {
            let obj = __torajs_dynobj_alloc();
            let a = ka.as_ptr() as *mut c_void;
            let b = kb.as_ptr() as *mut c_void;
            insert(obj, a, BUCKET_FLAGS_DEFAULT, nanbox::box_from_pair(2, 7));
            insert(obj, b, BUCKET_FLAGS_DEFAULT, nanbox::box_from_pair(5, 0));
            let mut v = 99u64;
            assert_eq!(__torajs_dynobj_get_own(obj, a, &mut v), 2);
            assert_eq!(v, 7);
            assert_eq!(__torajs_dynobj_get_own(obj, b, &mut v), 5);
            assert_eq!(v, 0);
            v = 99;
            let c = kc.as_ptr() as *const c_void;
            assert_eq!(__torajs_dynobj_get_own(obj, c, &mut v), DYNOBJ_GET_OWN_MISS);
            assert_eq!(v, 99);
            free_dynobj_blocks(obj);
        }
    }

    /// A tag-only read of an entry holding a ShortStr materializes no
    /// `Str` (nobody would release it); the value read materializes
    /// the one its caller owns.
    #[test]
    fn tag_read_of_short_str_materializes_nothing() {
        use crate::SHORT_STR_MATERIALIZED as N;
        use crate::get::{__torajs_dynobj_get_tag, __torajs_dynobj_get_value};
        let k = make_str("s");
        let short = torajs_anyvalue::nanbox::try_box_short_str(b"ab").unwrap();
        unsafe {
            let obj = __torajs_dynobj_alloc();
            let kp = k.as_ptr() as *mut c_void;
            insert(obj, kp, BUCKET_FLAGS_DEFAULT, short);
            let before = N.with(|n| n.get());
            assert_eq!(__torajs_dynobj_get_tag(obj, kp), ANY_HEAP);
            assert_eq!(N.with(|n| n.get()), before);
            assert_eq!(__torajs_dynobj_get_value(obj, kp), 0x1000);
            assert_eq!(N.with(|n| n.get()), before + 1);
            free_dynobj_blocks(obj);
        }
    }
}
