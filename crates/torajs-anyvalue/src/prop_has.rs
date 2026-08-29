//! `__torajs_any_prop_has` — own-property presence probe on an `any`
//! receiver (chunk B3, RFC 20260711 for-in).
//!
//! Primary consumer: the for-in mid-loop-delete guard — ES §14.7.5
//! EnumerateObjectProperties requires that properties deleted during
//! enumeration are NOT visited, so the lowering re-checks each key
//! before running the body. Also the substrate the hasOwnProperty /
//! propertyIsEnumerable port (RFC chunk D) will consume.
//!
//! Presence — not value truthiness: an entry whose stored value is
//! `undefined` still answers 1 (mirrors `"k" in o`), which is why the
//! dynobj arms route through `__torajs_dynobj_has` instead of the
//! nullish-tag probe `any_method_probe` uses.
//!
//! Per-receiver dispatch mirrors `prop_delete`:
//!
//! - null / undefined receiver → catchable TypeError, answers 0
//!   (hasOwnProperty ToObject shape; the for-in guard never sends
//!   these — a null source enumerates nothing upstream).
//! - `Tag::DynObj` → `__torajs_dynobj_has`.
//! - `Tag::Arr` → canonical index in `[0, len)` or `"length"` → 1;
//!   otherwise the expando props dynobj.
//! - `Tag::Closure` → expando props dynobj; `"length"` / `"name"`
//!   answer 1 per §20.2.4 (own non-enumerable in spec — tr carries
//!   them virtually).
//! - `Tag::Obj` (struct cell) → class-layout field probe.
//! - `Tag::Str` cell / ShortStr imm → index in `[0, len)` or
//!   `"length"` → 1 (§22.1.5.2).
//! - every other receiver → 0 (primitive ToObject wrappers carry no
//!   own string-keyed properties).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::{closure_props, header_flag, recv_cell};
use crate::nanbox::{AnyValue, is_null, is_short_str, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — own-entry presence (1 = live entry).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-arr — the arguments-materialization `"length"` face:
    /// 0 = plain array, 1 = arguments (intact), 2 = deleted.
    fn __torajs_arr_arguments_length_state(arr: *const c_void, key: *const c_void) -> i64;
    /// torajs-arr — bare FLAG_ARR_ARGUMENTS probe (the callee arm).
    fn __torajs_arr_is_arguments(arr: *const c_void) -> i64;
    /// torajs-arr — the `"callee"` face state (S2): 0 = strict mint,
    /// 1 = live sloppy entry, 2 = deleted (hole tombstone).
    fn __torajs_arr_arguments_callee_state(arr: *const c_void, key: *const c_void) -> i64;
    /// torajs-dynobj — packed W/E/C data-attribute flags
    /// (bit 1 = enumerable); answers 0 for an absent key, which is
    /// exactly the propertyIsEnumerable miss semantics.
    fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-structmeta — class-layout lookup + field probe.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// Accessor slot standing for a plain property name (`kind` 0 =
    /// getter, 1 = setter) — RFC 20260714-objlit-accessor.
    fn __torajs_struct_accessor_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
        kind: u8,
    ) -> u32;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-arr — per-index attribute flags (RFC
    /// 20260712-arr-exotic-define chunk C).
    fn __torajs_arr_index_flags(arr: *const c_void, idx: u64) -> u64;
}

/// Layout mirrors (see `member_get`): struct `class_tag` u32 at +8;
/// Str len u32 at +8, bytes at +16; Arr len u64 at +8, inline
/// props-dynobj slot at +24 (`torajs_arr::layout::ARR_PROPS_OFF`);
/// Closure props-dynobj slot at +24.
const OBJ_CLASS_TAG_OFF: usize = 8;
pub(crate) const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
pub(crate) const ARR_LEN_OFF: usize = 8;
pub(crate) const ARR_PROPS_OFF: usize = 24;
/// `arr_index_flags` result bit 3 — the index was deleted (hole;
/// `torajs_arr::define::F_HOLE` mirror, RFC 20260713 chunk C).
pub(crate) const ARR_F_HOLE: u64 = 1 << 3;

/// Decode the key Str cell to `(bytes, len)`.
#[inline]
pub(crate) unsafe fn key_bytes(key: *const c_void) -> (*const u8, u32) {
    let len = unsafe { key.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() };
    (unsafe { key.cast::<u8>().add(STR_DATA_OFF) }, len)
}

/// Parse a canonical array-index key (`"0"`, `"12"` — all digits, no
/// leading zero except `"0"` itself). `None` for anything else.
pub(crate) unsafe fn canonical_index(key: *const c_void) -> Option<u64> {
    let (bytes, len) = unsafe { key_bytes(key) };
    if len == 0 || len > 20 {
        return None;
    }
    let s = unsafe { core::slice::from_raw_parts(bytes, len as usize) };
    if !s.iter().all(u8::is_ascii_digit) || (len > 1 && s[0] == b'0') {
        return None;
    }
    let mut v: u64 = 0;
    for &b in s {
        v = v.checked_mul(10)?.checked_add((b - b'0') as u64)?;
    }
    Some(v)
}

/// `true` iff the key spells exactly `name`.
pub(crate) unsafe fn key_is(key: *const c_void, name: &[u8]) -> bool {
    let (bytes, len) = unsafe { key_bytes(key) };
    len as usize == name.len()
        && unsafe { core::slice::from_raw_parts(bytes, len as usize) } == name
}

/// Whether a `Tag::Obj` struct carries `key` as an own property. A
/// data field matches by name; an accessor (RFC
/// 20260714-objlit-accessor) lives under a synthetic slot name that
/// the plain-name lookup can never match, so both halves are probed —
/// an accessor IS an own property (§10.4), and both `hasOwnProperty`
/// and `propertyIsEnumerable` must say so.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str cell.
pub(crate) unsafe fn struct_has_own(ptr: *const c_void, key: *const c_void) -> i64 {
    // Blade 2 — an expando entry is an own property; probed first so
    // an anonymous struct without a layout row (NULL lookup below)
    // still answers its expandos.
    let props = unsafe { crate::member_get_layout::struct_props(ptr) };
    if !props.is_null() && unsafe { __torajs_dynobj_has(props, key) } != 0 {
        return 1;
    }
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return 0;
    }
    let (name_bytes, name_len) = unsafe { key_bytes(key) };
    if unsafe { __torajs_struct_field_find(layout, name_bytes, name_len) } != u32::MAX {
        // A `message` / `name` slot holding the own-absence sentinel
        // means the property does not exist (§20.5.6.1.1: an undefined
        // ctor message defines no own `message`; a delete detaches it.
        // §20.5.3.2: `name` is the prototype's until user code assigns
        // one).
        if unsafe { crate::struct_error_msg::error_absent_key(ptr, key) } {
            return 0;
        }
        return 1;
    }
    const ACC_GETTER: u8 = 0;
    const ACC_SETTER: u8 = 1;
    let has_half = |kind: u8| {
        let idx = unsafe { __torajs_struct_accessor_find(layout, name_bytes, name_len, kind) };
        idx != u32::MAX
    };
    (has_half(ACC_GETTER) || has_half(ACC_SETTER)) as i64
}

/// See module doc. `key` is a live Str cell (the lowering interns
/// static names and materializes dynamic string keys before the
/// call).
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_prop_has(recv: AnyValue, key: *const c_void) -> i64 {
    // A Proxy has no own/inherited split of its own: §10.5.7 is the
    // only membership question it answers, and the `has` trap sees
    // both spellings. §10.5.5 [[GetOwnProperty]] — the real own-only
    // probe — is knife 4.
    if crate::proxy::is_proxy(recv) {
        return unsafe { crate::proxy_ops::has(crate::nanbox::as_void_ptr(recv), key) }
            .unwrap_or(false) as i64;
    }
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return 0;
    }
    // §6.1.7 — HasOwnProperty of a symbol key is exactly an entry-table
    // probe: every face in the cascade below (index domain, `length`,
    // fn `name`, builtin-prototype interned methods, ctor statics) is
    // string-keyed, and each reads the key's Str payload. A shape with
    // no dict simply owns no symbol-keyed property.
    if unsafe { crate::member_get_symbol::key_is_symbol(key) } {
        let Some((ptr, t)) = recv_cell(recv) else {
            return 0;
        };
        let dict = unsafe { crate::member_get_symbol::own_dict(ptr, t) };
        return (!dict.is_null() && unsafe { __torajs_dynobj_has(dict, key) } != 0) as i64;
    }
    if is_short_str(recv) {
        // ShortStr imm — len lives in bits 47..40 (SSO layout).
        let len = (recv >> 40) & 0xFF;
        return unsafe { str_index_has(len, key) };
    }
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => {
            if unsafe { __torajs_dynobj_has(ptr, key) } != 0 {
                return 1;
            }
            // Builtin `<Ctor>.prototype` singleton — an interned
            // builtin method is the prototype's own property per
            // spec even though it lives in the method-cell table,
            // not the dynobj (RFC 20260712 chunk 1; the
            // delete-tombstone shading is chunk 3).
            unsafe { builtin_proto_method_own(ptr, key) }
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            let len = unsafe { ptr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
            if let Some(i) = unsafe { canonical_index(key) }
                && i < len
            {
                // A deleted index (hole shadow entry) is absent
                // (chunk C).
                return (unsafe { __torajs_arr_index_flags(ptr, i) } & ARR_F_HOLE == 0) as i64;
            }
            if unsafe { key_is(key, b"length") } {
                // A deleted arguments-materialization length (hole
                // tombstone, §10.4.4) is absent — and must NOT fall
                // through to the bag probe below, which would see
                // the tombstone entry itself.
                return (unsafe { __torajs_arr_arguments_length_state(ptr, key) } != 2) as i64;
            }
            // §10.4.4.6 step 21 — `callee` is an OWN accessor on an
            // arguments materialization (the %ThrowTypeError% pair
            // the gOPD arm reports). S2: a sloppy mint's deleted
            // callee (hole tombstone) is ABSENT — and must not fall
            // through to the bag probe below, which would see the
            // tombstone entry itself; a live sloppy entry and the
            // strict accessor are both own.
            if unsafe { key_is(key, b"callee") } && unsafe { __torajs_arr_is_arguments(ptr) } != 0 {
                return (unsafe { __torajs_arr_arguments_callee_state(ptr, key) } != 2) as i64;
            }
            let props = unsafe {
                ptr.cast::<u8>()
                    .add(ARR_PROPS_OFF)
                    .cast::<*const c_void>()
                    .read()
            };
            if !props.is_null() && unsafe { __torajs_dynobj_has(props, key) } != 0 {
                return 1;
            }
            // `Array.prototype` is itself an Arr (ES §23.1.3), so the
            // interned-method probe the DynObj arm runs has to reach
            // it here too — `"map" in Array.prototype` is `true`.
            // Ordinary arrays answer -1 from the tag lookup inside
            // and fall out at 0.
            unsafe { builtin_proto_method_own(ptr, key) }
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            // chunk C — a tombstoned virtual prop is gone until an
            // expando write recreates it (probed below).
            if unsafe { key_is(key, b"length") }
                && !unsafe { header_flag(ptr, torajs_rc::FLAG_FN_LENGTH_DELETED) }
            {
                return 1;
            }
            if unsafe { key_is(key, b"name") }
                && !unsafe { header_flag(ptr, torajs_rc::FLAG_FN_NAME_DELETED) }
            {
                return 1;
            }
            let props = unsafe { closure_props(ptr) };
            if !props.is_null() && unsafe { __torajs_dynobj_has(props, key) } != 0 {
                return 1;
            }
            // RFC 20260721 刀 3 — a builtin ctor cell owns its table
            // statics, `prototype`, and (Number) data constants; the
            // gOPD arm already answers them, so HasOwnProperty must
            // agree.
            (unsafe { crate::method_value::ctor_own_read_cell(ptr, key) }.is_some()) as i64
        }
        // Rotation 354 — promise bag own probe (the +32 expando the
        // defineProperty / plain-assign arms write; `then` / `catch`
        // stay prototype surface, absent as own).
        Some((ptr, t)) if t == Tag::Promise as u16 => {
            let props = unsafe { crate::member_get::promise_props(ptr) };
            (!props.is_null() && unsafe { __torajs_dynobj_has(props, key) } != 0) as i64
        }
        // §10.4.5.2 / §25.1 — buffer-family own face. A typed
        // array's in-bounds canonical indices are own properties
        // (`length` / `byteLength` etc. stay PROTOTYPE accessors,
        // absent as own); everything else asks the expando bag.
        Some((ptr, t)) if crate::member_get_buffer::is_buffer_family(t) => {
            if t == Tag::TypedArray as u16
                && let Some(i) = unsafe { canonical_index(key) }
            {
                let len = unsafe { crate::member_get_buffer::typedarray_len(recv) };
                return (i < len as u64 && len >= 0) as i64;
            }
            let props = unsafe { crate::member_get_layout::buffer_props(ptr, t) };
            (!props.is_null() && unsafe { __torajs_dynobj_has(props, key) } != 0) as i64
        }
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe { struct_has_own(ptr, key) },
        Some((ptr, t)) if t == Tag::Str as u16 => {
            let len = unsafe { ptr.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() } as u64;
            unsafe { str_index_has(len, key) }
        }
        // RFC 20260716 刀 13 — `StringWrapper` receiver: `length`
        // and numeric indices `[0, len)` are own properties per
        // §22.1.4 String Exotic Object. View-through the inner Str
        // cell to read the code-unit count; empty-wrapper (NULL
        // inner sentinel) has len 0 so only `"length"` matches.
        //
        // 2026-07-16 (rotation 121 chunk 5) — after the inherent
        // §22.1.4 face, also probe the wrapper's lazy expando dynobj
        // (`new String("x").foo = 1`, chunk 4). Same fall-through
        // shape as the Arr arm above (inherent index face first,
        // expando keys after).
        Some((ptr, t)) if t == Tag::StringWrapper as u16 => {
            let inner_ptr = unsafe { (ptr.cast::<u8>().add(8) as *const *const c_void).read() };
            let len = if inner_ptr.is_null() {
                0
            } else {
                unsafe { inner_ptr.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() as u64 }
            };
            if unsafe { str_index_has(len, key) } != 0 {
                return 1;
            }
            let props = unsafe { crate::member_get::wrapper_props(ptr) };
            if props.is_null() {
                0
            } else {
                unsafe { __torajs_dynobj_has(props, key) as i64 }
            }
        }
        // RFC 20260722 刀 4 — a RegExp instance owns exactly its
        // `lastIndex` (§22.2.4.1 RegExpAlloc); the rest of the
        // surface is prototype accessors, absent as own.
        Some((_, t)) if t == Tag::RegExp as u16 => unsafe { key_is(key, b"lastIndex") as i64 },
        // RFC 20260716 刀 5 (rotation 121 chunk 5) — Number/Boolean
        // wrappers carry no inherent own string keys (§21.1 / §20.3),
        // so the expando dynobj is the whole answer.
        Some((ptr, t)) if t == Tag::NumberWrapper as u16 || t == Tag::BooleanWrapper as u16 => {
            let props = unsafe { crate::member_get::wrapper_props(ptr) };
            if props.is_null() {
                0
            } else {
                unsafe { __torajs_dynobj_has(props, key) as i64 }
            }
        }
        _ => 0,
    }
}

// Builtin-proto singleton own-face probe — moved to
// `prop_has_proto.rs` (file-size hard limit, RFC 20260721 刀 11).
use crate::prop_has_proto::builtin_proto_method_own;

/// Shared Str-receiver arm: index in `[0, len)` or `"length"` → 1.
unsafe fn str_index_has(len: u64, key: *const c_void) -> i64 {
    if let Some(i) = unsafe { canonical_index(key) }
        && i < len
    {
        return 1;
    }
    unsafe { key_is(key, b"length") as i64 }
}

/// RFC 20260716 刀 13 — typed-Str-receiver `.hasOwnProperty(key)` per
/// ES §22.1.4 String Exotic Object. The typed SSA lower path
/// `ssa_lower_call_universal_methods::try_lower` was folding the
/// call to `ConstBool(false)` for every primitive receiver ("no own
/// properties in our subset") — spec-incorrect for strings which
/// expose `length` and each integer index `[0, [[StringData]].length)`.
/// Runtime side just needs the same len + key math [`str_index_has`]
/// runs on the Any-lane's `Tag::Str` arm; expose it under a stable
/// symbol so the emit inline `Call` can bind to it.
///
/// # Safety
/// `s` is a live Tag::Str block; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_prop_has(s: *const c_void, key: *const c_void) -> i64 {
    let len = unsafe { s.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() } as u64;
    unsafe { str_index_has(len, key) }
}

/// RFC 20260716 刀 20 — typed-`Type::Str` receiver's
/// `.propertyIsEnumerable(key)` per ES §22.1.4 String Exotic Object.
/// Mirror of [`__torajs_str_prop_has`] with the enumerability filter:
/// `"length"` is spec non-enumerable, so this returns 1 ONLY for a
/// canonical index `[0, len)`. Delegates to [`str_index_enumerable`]
/// (the same helper the Any-lane's `Tag::Str` arm uses).
///
/// # Safety
/// `s` is a live Tag::Str block; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_prop_enumerable(s: *const c_void, key: *const c_void) -> i64 {
    let len = unsafe { s.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() } as u64;
    unsafe { crate::prop_enumerable::str_index_enumerable(len, key) }
}
