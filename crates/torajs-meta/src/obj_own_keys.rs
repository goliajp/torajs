//! RC-4 F1c — runtime chooser for `Object.keys` /
//! `Object.getOwnPropertyNames` / `Reflect.ownKeys` on struct-typed
//! receivers.
//!
//! `Object.defineProperty` converts a struct receiver to a DynObj and
//! rebinds the binding (`emit_any_dynobj_writeback`), so the
//! compile-time field list the SSA arm emits can be stale — a
//! runtime-defined property (test262 gOPN accessor family) was
//! invisible to reflection. The SSA arm still builds the static list
//! (correct for a plain struct cell, zero-cost reflection), then
//! routes through [`__torajs_obj_own_keys`]:
//!
//! - receiver is a DynObj cell → drop the static list and build the
//!   key array from the live entry walk in ES §10.1.11.1 order
//!   (array-index keys ascending, then insertion order).
//!   `include_nonenum = 0` (`Object.keys`) filters enumerable-only;
//!   `1` (`getOwnPropertyNames` / `ownKeys`) includes every key.
//! - anything else → return the static list as-is.

use core::ffi::c_void;

use crate::obj_own_keys_key_shape::{key_bytes_are, key_is_canonical_index};
use crate::obj_own_keys_proto_names::push_synthesized_proto_names;
// The key-shape predicates moved to a sibling for the file-size cap;
// re-exported so every `crate::obj_own_keys::` consumer face (for-in,
// values/entries) stays unchanged.
pub(crate) use crate::obj_own_keys_key_shape::key_is_proto_slot;

unsafe extern "C" {
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_rc_inc(p: *mut c_void);
    /// `runtime_str.c` universal-drop dispatcher (settles the unused
    /// static list on the DynObj path).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-dynobj iteration surface — keys are BORROWED.
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    /// torajs-throw — ToObject on null / undefined (§20.1.2.17 step 1).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-rc — builtin `<Ctor>.prototype` singleton tag probe
    /// (Function.prototype's virtual own name/length pair, §20.2.3).
    fn __torajs_builtin_proto_tag_of(p: *const c_void) -> i64;
    /// torajs-rc — `delete`-tombstone probe for the virtual slots.
    fn __torajs_builtin_proto_is_deleted(tag: i64, mid: i64) -> i64;
}

/// `torajs_rc::flags` mirrors — `delete fn.name` / `delete fn.length`
/// tombstones on a Closure header (bits 10/11).
const FLAG_FN_NAME_DELETED: u16 = 1 << 10;
const FLAG_FN_LENGTH_DELETED: u16 = 1 << 11;
/// `torajs_rc::FLAG_FN_PROTO` mirror (bit 15, Closure-private —
/// disjoint-by-tag with Arr's exotic-index bit). Set by the compiler on
/// a plain `function` cell; clear for arrow / method forms, which own
/// no `prototype` at all (§20.2.4 vs §10.2.5).
const FLAG_FN_PROTO: u16 = 1 << 15;
/// `torajs_rc::any_method` mirrors — Function.prototype's virtual
/// own name/length tombstone slots (RFC 20260722 刀 3).
const FN_PROTO_NAME_SLOT: i64 = 164;
const FN_PROTO_LENGTH_SLOT: i64 = 165;
/// `Function.prototype`'s builtin-proto family tag.
const FUNCTION_PROTO_TAG: i64 = 13;

// Layout / tag mirror constants live in
// [`crate::obj_own_keys_layout`] (rotation 354 file-size split); the
// re-export keeps every `crate::obj_own_keys::` consumer face
// (for-in, values/entries, gOPD) unchanged.
pub(crate) use crate::obj_own_keys_layout::*;

/// Runtime chooser — see module doc. Returns a +1-rc `Arr<Str>`.
///
/// # Safety
/// `obj` is null or a live heap ptr with a universal header;
/// `static_names` is an owned +1 `Arr<Str>` this call consumes-or-
/// returns.
/// `true` iff the heap cell's `type_tag` is DynObj.
#[inline]
unsafe fn is_dynobj(obj: *const c_void) -> bool {
    unsafe { *((obj as *const u8).add(HDR_TYPE_TAG_OFF) as *const u16) == TAG_DYNOBJ }
}

/// Append a live DynObj walk's keys onto `arr` in ES §10.1.11.1
/// order. `include_nonenum = 0` filters enumerable-only. Returns the
/// (possibly reallocated) array.
/// `skip_index_shadow` — an Array receiver's props dynobj carries
/// per-index attribute shadow entries under canonical index keys
/// (RFC 20260712-arr-exotic-define chunk B); the index domain is
/// already enumerated from element storage, so those keys are
/// filtered here.
/// `skip_fn_prototype` — a plain function's `prototype` is materialized
/// lazily INTO this dict on first read (RFC 20260721 刀 9), but
/// §20.2.4 creates it at function definition, so it belongs with the
/// virtual `length` / `name` pair the closure arm emits, not at
/// whatever position the lazy mint happened to land in. The closure arm
/// always emits it and filters it here.
unsafe fn dynobj_keys_append(
    obj: *const c_void,
    include_nonenum: i64,
    mut arr: *mut u8,
    skip_index_shadow: bool,
    skip_fn_prototype: bool,
) -> *mut u8 {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let mut order = vec![0u64; len as usize];
    let n = unsafe { __torajs_dynobj_iter_order(obj, order.as_mut_ptr(), len) };
    for &i in order.iter().take(n as usize) {
        if include_nonenum == 0 {
            let flags = unsafe { __torajs_dynobj_iter_flags(obj, i) };
            if flags & FLAG_ENUMERABLE == 0 {
                continue;
            }
        }
        let key = unsafe { __torajs_dynobj_iter_key(obj, i) };
        if key.is_null() {
            continue;
        }
        if skip_index_shadow && unsafe { key_is_canonical_index(key) } {
            continue;
        }
        if skip_fn_prototype && unsafe { key_bytes_are(key, b"prototype") } {
            continue;
        }
        // PROTO_SLOT_KEY is tr's [[Prototype]]-slot simulation key
        // (class proto chains, %GeneratorPrototype% wiring — RFC
        // 20260713 blade 5), never a spec own property: hide it from
        // every own-keys surface (§10.1.11.1 lists own PROPERTY
        // keys; the prototype lives in an internal slot). The
        // enumerable-clear on the entry already covers the keys
        // face; this filter is the gOPN (include_nonenum) face's
        // gate. A user's own `__proto__` DATA entry (shorthand
        // `{__proto__}`) is a real own property and passes through.
        if unsafe { key_is_proto_slot(key) } {
            continue;
        }
        // Borrowed key → the array slot takes its own share.
        unsafe { __torajs_rc_inc(key) };
        arr = unsafe { __torajs_arr_push(arr, key as i64) };
    }
    arr
}

/// Build the key `Arr<Str>` from a live DynObj walk in ES
/// §10.1.11.1 order. `include_nonenum = 0` filters enumerable-only.
/// On the gOPN surface a `Function.prototype` singleton leads with
/// its virtual own `length` / `name` pair (§20.2.3 — CreateBuiltin-
/// Function sets length before name; tombstone slots gate deletes).
unsafe fn dynobj_keys_walk(obj: *const c_void, include_nonenum: i64) -> *mut c_void {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let mut arr = unsafe { __torajs_arr_alloc(len) };
    if include_nonenum != 0 && unsafe { __torajs_builtin_proto_tag_of(obj) } == FUNCTION_PROTO_TAG {
        for (name, slot) in [
            (&b"length"[..], FN_PROTO_LENGTH_SLOT),
            (&b"name"[..], FN_PROTO_NAME_SLOT),
        ] {
            if unsafe { __torajs_builtin_proto_is_deleted(FUNCTION_PROTO_TAG, slot) } == 0 {
                // alloc_str_key mints rc=1 — the array slot adopts it.
                let k = unsafe { crate::reflect::alloc_str_key(name) };
                arr = unsafe { __torajs_arr_push(arr, k as i64) };
            }
        }
    }
    if include_nonenum != 0 {
        arr = unsafe { push_synthesized_proto_names(obj, arr, false) };
    }
    unsafe { dynobj_keys_append(obj, include_nonenum, arr, false, false) as *mut c_void }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_own_keys(
    obj: *const c_void,
    static_names: *mut c_void,
    include_nonenum: i64,
) -> *mut c_void {
    if obj.is_null() || !unsafe { is_dynobj(obj) } {
        // RFC 20260718-error-message-own-prop — the typed tier's
        // compile-time field list can't see the error `message`
        // attributes (§20.5.6.1.1 [[Enumerable]]: false; own-absent
        // sentinel): filter it here, the one site every typed keys /
        // for-in / gOPN emit routes through.
        if !obj.is_null() && unsafe { heap_type_tag_local(obj) } == TAG_OBJ_CELL {
            let hdr_flags = unsafe { obj.cast::<u8>().add(6).cast::<u16>().read() };
            if hdr_flags & crate::struct_reflect::FLAG_ERROR != 0 {
                if include_nonenum == 0 {
                    // Enumerable-only surface: both §20.5 slots carry
                    // [[Enumerable]]: false. `stack` joins `message`
                    // here and nowhere else — it is unconditionally
                    // own, so gOPN keeps it.
                    unsafe { strip_key(static_names, b"message") };
                    unsafe { strip_key(static_names, b"stack") };
                } else if unsafe { crate::struct_enum::error_field_absent(obj, b"message") } {
                    unsafe { strip_key(static_names, b"message") };
                }
                // `name` is absent on both surfaces alike: §20.5.3.2
                // gives it to `Error.prototype`, so an instance owns
                // one only after user code assigned it
                // (`struct_enum::error_prop_skip` twin).
                if unsafe { crate::struct_enum::error_field_absent(obj, b"name") } {
                    unsafe { strip_key(static_names, b"name") };
                }
            }
            // Blade 3 (RFC 20260714-struct-dynamic-props) — fold the
            // +24 expando dict's keys in per §10.1.11.1 (NULL props
            // short-circuits inside).
            return unsafe {
                crate::obj_own_keys_struct::struct_keys_with_expandos(
                    obj,
                    static_names,
                    include_nonenum,
                )
            };
        }
        return static_names;
    }
    unsafe { __torajs_value_drop_heap(static_names) };
    unsafe { dynobj_keys_walk(obj, include_nonenum) }
}

/// Remove the first `name`-spelling Str from an owned freshly-minted
/// `Arr<Str>` in place (shift-down + len decrement, dropping the
/// removed cell). The static key lists this filters are the typed
/// keys emit's private mint — never a shared array.
unsafe fn strip_key(arr: *mut c_void, name: &[u8]) {
    let len_slot = unsafe { arr.cast::<u8>().add(ARR_LEN_OFF) } as *mut u64;
    let len = unsafe { len_slot.read() };
    let data = unsafe {
        arr.cast::<u8>()
            .add(ARR_DATA_PTR_OFF)
            .cast::<*mut u64>()
            .read()
    };
    if data.is_null() {
        return;
    }
    for i in 0..len {
        let s = unsafe { data.add(i as usize).read() } as *const c_void;
        if s.is_null() {
            continue;
        }
        let slen = unsafe { s.cast::<u8>().add(8).cast::<u32>().read() } as usize;
        if slen != name.len()
            || unsafe { core::slice::from_raw_parts(s.cast::<u8>().add(16), slen) } != name
        {
            continue;
        }
        unsafe { __torajs_value_drop_heap(s as *mut c_void) };
        for j in i..len - 1 {
            let next = unsafe { data.add(j as usize + 1).read() };
            unsafe { data.add(j as usize).write(next) };
        }
        unsafe { len_slot.write(len - 1) };
        return;
    }
}

/// Index-key list `["0", ..., "<len-1>"]`, plus a trailing
/// `"length"` on the gOPN surface (`include_nonenum = 1`) — shared by
/// the Str and Arr ToObject arms.
unsafe fn index_keys(len: i64, include_nonenum: i64) -> *mut c_void {
    if include_nonenum == 0 {
        unsafe { crate::own_names::__torajs_arr_keys_only(len) }
    } else {
        unsafe { crate::own_names::__torajs_arr_index_strs(len) }
    }
}

/// Arr-cell own keys: index keys (+ `"length"` for gOPN, §10.4.2)
/// followed by expando keys from the inline props dynobj (insertion
/// order — `length` predates any expando write, matching the ES
/// OrdinaryOwnPropertyKeys creation-order tail).
unsafe fn arr_cell_keys(cell: *const c_void, include_nonenum: i64) -> *mut c_void {
    // RFC 20260712-arr-exotic-define chunk C — both surfaces ride
    // exotic-aware helpers: keys filters per-index enumerable, gOPN
    // keeps non-enumerable indices but skips deleted (hole) ones
    // (RFC 20260713 chunk C).
    let mut out = if include_nonenum == 0 {
        unsafe { crate::own_names::__torajs_arr_keys_only_of(cell) }
    } else {
        unsafe { crate::own_names::__torajs_arr_index_strs_of(cell) }
    };
    // §23.1.3 makes `Array.prototype` an Arr cell rather than a
    // dynobj, so its synthesized method names come through here
    // instead of the walk above — same surface, other cell shape.
    if include_nonenum != 0 {
        out = unsafe { push_synthesized_proto_names(cell, out as *mut u8, true) as *mut c_void };
    }
    let props =
        unsafe { (cell.cast::<u8>().add(ARR_PROPS_OFF) as *const u64).read() } as *const c_void;
    if props.is_null() {
        return out;
    }
    unsafe {
        dynobj_keys_append(props, include_nonenum, out as *mut u8, true, false) as *mut c_void
    }
}

/// `Object.keys` / `getOwnPropertyNames` / `Reflect.ownKeys` arm for
/// an `any`-typed receiver — full ES §20.1.2.17 ToObject dispatch
/// (chunk B1, for-in RFC):
///
/// - DynObj cell → live-entry walk (enumerable filter per surface).
/// - Str cell / ShortStr imm → index keys (+ `"length"` for gOPN,
///   §22.1.5.2.4).
/// - Arr cell → index keys (+ `"length"` for gOPN) + expando keys.
/// - Closure cell → expando keys only (`length` / `name` own props
///   are not materialized — recorded divergence, both non-enumerable
///   per spec so `Object.keys` is exact).
/// - Obj (struct) cell → static-layout field walk (`struct_enum`).
/// - null / undefined → catchable TypeError (ToObject throws).
/// - every other receiver (Num imm / Bool / BigInt / Map / Set /
///   boxed primitives) → empty array: their ToObject wrappers carry
///   no own enumerable string keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_own_keys(v: u64, include_nonenum: i64) -> *mut c_void {
    // Cell-imm check mirrors `struct_enum::is_cell_imm` — the tag
    // read below is only sound for a real heap ptr bit pattern.
    if is_dynobj_imm(v) {
        return unsafe { dynobj_keys_walk(v as *const c_void, include_nonenum) };
    }
    if v == crate::reflect::VALUE_NULL_IMM || v == crate::reflect::VALUE_UNDEFINED_IMM {
        unsafe {
            __torajs_throw_type_error(c"cannot convert undefined or null to object".as_ptr());
        }
        return unsafe { __torajs_arr_alloc(0) as *mut c_void };
    }
    // ShortStr imm — len lives in bits 47..40 (SSO layout).
    if v >> 48 == SHORT_STR_TOP16 {
        let len = ((v >> 40) & 0xFF) as i64;
        return unsafe { index_keys(len, include_nonenum) };
    }
    if crate::reflect::is_cell_imm(v) {
        let cell = v as *const c_void;
        return match unsafe { heap_type_tag_local(cell) } {
            TAG_STR_CELL => {
                let len =
                    unsafe { (cell.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() } as i64;
                unsafe { index_keys(len, include_nonenum) }
            }
            TAG_ARR_CELL => unsafe { arr_cell_keys(cell, include_nonenum) },
            TAG_CLOSURE_CELL => {
                let props =
                    unsafe { (cell.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64).read() }
                        as *const c_void;
                let mut out = unsafe { __torajs_arr_alloc(0) };
                // §10.2.9/§10.2.10 (via §20.2.4) — every function's
                // virtual own `length` / `name` pair surfaces on gOPN
                // in creation order (length first; test262
                // property-order family asserts the adjacency).
                // Non-enumerable, so `Object.keys` stays exact.
                // Delete tombstones live on the header flags.
                if include_nonenum != 0 {
                    let flags = unsafe { (cell.cast::<u8>().add(6) as *const u16).read() };
                    for (name, deleted) in [
                        (&b"length"[..], flags & FLAG_FN_LENGTH_DELETED != 0),
                        (&b"name"[..], flags & FLAG_FN_NAME_DELETED != 0),
                        // §20.2.4 — a plain `function` also owns
                        // `prototype` {writable: true, enumerable:
                        // false, configurable: false}, created WITH the
                        // function, so it sits right after the
                        // name/length pair. tr materializes it lazily
                        // into the props dict on first read (RFC
                        // 20260721 刀 9), so emitting it here (and
                        // filtering it out of the props append below)
                        // makes the order independent of whether
                        // anything has read it yet. An arrow / method
                        // form has no `prototype` at all — the header
                        // flag is exactly that distinction. Not
                        // deletable (configurable: false), so no
                        // tombstone.
                        (&b"prototype"[..], flags & FLAG_FN_PROTO == 0),
                    ] {
                        if !deleted {
                            let k = unsafe { crate::reflect::alloc_str_key(name) };
                            out = unsafe { __torajs_arr_push(out, k as i64) };
                        }
                    }
                }
                if props.is_null() {
                    out as *mut c_void
                } else {
                    unsafe {
                        dynobj_keys_append(props, include_nonenum, out, false, true) as *mut c_void
                    }
                }
            }
            TAG_OBJ_CELL => unsafe {
                crate::struct_enum::__torajs_anyv_struct_keys(v, include_nonenum)
            },
            // Rotation 354 — promise cell enumerates its +32 expando
            // bag (defineProperty / plain-assign entries); no
            // inherent own keys — `then` / `catch` are prototype
            // surface. The for-in kernel rides this arm through its
            // own_keys call, so all four enumeration spellings agree.
            TAG_PROMISE_CELL => {
                let props =
                    unsafe { (cell.cast::<u8>().add(PROMISE_PROPS_OFF) as *const u64).read() }
                        as *const c_void;
                let out = unsafe { __torajs_arr_alloc(0) };
                if props.is_null() {
                    out as *mut c_void
                } else {
                    unsafe {
                        dynobj_keys_append(props, include_nonenum, out, false, false) as *mut c_void
                    }
                }
            }
            // §10.4.3.3 String exotic OwnPropertyKeys — the
            // [[StringData]] integer indices come first (+ "length"
            // for gOPN, like the bare Str arm), then the expando
            // keys with index shadows skipped (arr-arm pattern).
            // A NULL inner slot (`new String()`) is the empty string.
            TAG_STRING_WRAPPER => {
                let inner =
                    unsafe { (cell.cast::<u8>().add(WRAPPER_INNER_OFF) as *const u64).read() }
                        as *const c_void;
                let len = if inner.is_null() {
                    0i64
                } else {
                    let units =
                        unsafe { (inner.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() };
                    units as i64
                };
                let out = unsafe { index_keys(len, include_nonenum) };
                let props =
                    unsafe { (cell.cast::<u8>().add(WRAPPER_PROPS_OFF) as *const u64).read() }
                        as *const c_void;
                if props.is_null() {
                    out
                } else {
                    unsafe {
                        dynobj_keys_append(props, include_nonenum, out as *mut u8, true, false)
                            as *mut c_void
                    }
                }
            }
            // RFC 20260716 刀 5 (rotation 121 chunk 5) — wrapper
            // cells carry a lazy expando dynobj at `+16` (chunks
            // 4/5); Number / Boolean wrappers have no inherent own
            // keys, so the expando walk is the whole surface.
            TAG_NUMBER_WRAPPER | TAG_BOOLEAN_WRAPPER => {
                let props =
                    unsafe { (cell.cast::<u8>().add(WRAPPER_PROPS_OFF) as *const u64).read() }
                        as *const c_void;
                let out = unsafe { __torajs_arr_alloc(0) };
                if props.is_null() {
                    out as *mut c_void
                } else {
                    unsafe {
                        dynobj_keys_append(props, include_nonenum, out, false, false) as *mut c_void
                    }
                }
            }
            _ => unsafe { __torajs_arr_alloc(0) as *mut c_void },
        };
    }
    // Number imm / Bool sentinel / any other non-cell — ToObject
    // wrapper with no own enumerable string keys.
    unsafe { __torajs_arr_alloc(0) as *mut c_void }
}

/// Universal-header `type_tag` read (u16 at +4) — local twin of
/// `is_dynobj`'s read for the ToObject dispatch arms. Shared with
/// the values/entries chooser twin (`obj_own_values.rs`).
#[inline]
pub(crate) unsafe fn heap_type_tag_local(cell: *const c_void) -> u16 {
    unsafe { *((cell as *const u8).add(HDR_TYPE_TAG_OFF) as *const u16) }
}

/// Cell-imm + DynObj tag test on a raw NaN-box value (mirrors
/// `struct_enum::is_cell_imm` — the header read is only sound for a
/// real heap ptr bit pattern). Shared with the values/entries
/// chooser twin (`obj_own_values.rs`).
pub(crate) fn is_dynobj_imm(v: u64) -> bool {
    let top16_zero = v & 0xFFFF_0000_0000_0000 == 0;
    let not_sentinel = v & 0x2 == 0;
    top16_zero && not_sentinel && v != 0 && unsafe { is_dynobj(v as *const c_void) }
}
