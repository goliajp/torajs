//! `Tag::DynObj` + `Tag::Obj` receiver arms for
//! `__torajs_any_method_call` (split out of `method_call.rs` — the
//! dispatcher stays the id-switch, the property-probing receivers
//! live here).
//!
//! - `dynobj_method` — probe the property by the interned name Str;
//!   a closure-cell value with a boxed dual entry invokes through
//!   the uniform ABI; an accessor entry's getter runs first and its
//!   answer dispatches as the callee (C4+ chunk 523).
//! - `struct_method` (L3b #9, chunk 524) — static-layout class /
//!   anonymous-struct instances (`Tag::Obj`) store fields at fixed
//!   byte offsets, so the probe walks the toolchain-emitted
//!   `__torajs_class_layouts` table (torajs-structmeta W-J Phase A4
//!   read side): class_tag@+8 → layout lookup → field find by the
//!   name Str's bytes → the slot's static type decides the callee
//!   shape. A `Closure`-typed slot (field type_tag 8) holds the raw
//!   env cell; an `Any`-typed slot (tag 0) holds a NaN-box that may
//!   wrap one. Everything else — no layout (class_tag 0), absent
//!   field, non-callable slot type — answers the same catchable
//!   TypeError as the other arms, never a layout mis-read. This is
//!   what makes `n.inner.op(1, 1)` work when `inner: { op: (a, b) =>
//!   a + b }` nests inside an `any` ObjectLit init (the nested
//!   literal lowers as an anon struct, not a dynobj).
//!
//! Argument ledger: identical to the dispatcher — the property
//! probe borrows (a struct field slot is owned by the receiver,
//! which outlives the call); the adapter's return is caller-owned
//! per the boxed-value convention.

use core::ffi::c_void;

use torajs_rc::{
    __torajs_rc_inc, ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF,
};

use crate::method_call::{closure_boxed_entry, closure_cell_entry, not_callable};
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-str — allocate a fresh Str from raw bytes (the
    /// "[object Object]" text + the re-dispatch "toString" key).
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — property probe by Str key: the slot's ANY_TAG
    /// (5 = absent) / per-tag payload.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — run an accessor entry's getter; the answer is
    /// an owned AnyValue per the boxed-value convention.
    fn __torajs_accessor_invoke_getter(pair: *const c_void) -> u64;
    /// torajs-arr — arr-props expando probe (NULL props answers 5 =
    /// absent internally); same ANY_TAG channel as the dynobj pair.
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (NULL for class_tag 0 / past the table).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    /// torajs-structmeta — field index by name bytes (u32::MAX miss).
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// torajs-structmeta — field byte offset + coarse type tag
    /// (zeroed for a miss; a real offset is never 0).
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    /// torajs-structmeta 刀 4 — class-method boxed adapter by name
    /// bytes (NULL miss).
    fn __torajs_struct_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> *const c_void;
    /// torajs-structmeta — does a name spell an accessor SLOT
    /// (`__getter_v`)? 255 = a plain, user-callable name.
    fn __torajs_accessor_name_kind(name: *const u8, name_len: u32) -> u8;
}

/// Mirror of `torajs-structmeta::FieldInfo` (returned by value
/// across the FFI).
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of torajs-core `ssa_lower_accessor.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: u64 = 6;

/// Byte offset of the `class_tag` u32 inside a `Tag::Obj` instance —
/// mirror of torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF`.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// Interned name Str layout — mirror of torajs-str
/// `layout::{STR_LEN_OFF, STR_DATA_OFF}`.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// Field type_tags whose slot can hold a callee — mirror of
/// torajs-core `ssa::module_methods::field_type_tag_of` (0 = Any,
/// 8 = Closure).
const FIELD_TAG_ANY: u8 = 0;
const FIELD_TAG_CLOSURE: u8 = 8;

/// `Tag::DynObj` arm — probe the property by name; a closure-cell
/// value invokes through its boxed dual entry. The property probe
/// borrows (dynobj keeps its own value reference; the receiver
/// outlives the call), and the adapter's return is caller-owned per
/// the boxed-value convention. `recv_slot` threads to the generic
/// array-like mutators (dynobj relocation writeback; NULL for
/// non-variable receivers).
pub(crate) unsafe fn dynobj_method(
    obj: *mut c_void,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // RFC 20260712 chunks 2+3a — a NULL name is the reified-cell
        // `call` / `apply` re-dispatch (ordinary calls always carry
        // name bytes): the mid is authoritative, so an array-family
        // mid runs the ES generic array-like semantics over this
        // receiver instead of falling to the not-callable exit.
        if name_str.is_null() && crate::method_call_arraylike::arraylike_supported(mid) {
            return crate::method_call_arraylike::arraylike_method(obj, mid, recv_slot, argv, argc);
        }
        if !name_str.is_null() {
            let key = name_str as *const c_void;
            let dtag = __torajs_dynobj_get_tag(obj, key);
            // Absent own entry → the inherited Object.prototype
            // surface. Ordered AFTER the own probe so a user
            // monkey-patch (`o.valueOf = fn`) always wins; a
            // resolved-but-not-callable entry keeps the TypeError.
            if dtag == 5 {
                return object_proto_fallback(obj, mid, false, argv);
            }
            // ANY_HEAP = 4 — a plain closure-cell property.
            if dtag == 4 {
                let cell = __torajs_dynobj_get_value(obj, key);
                // A reified builtin cell stored as an own property
                // (`obj.pop = Array.prototype.pop`) re-dispatches
                // its CARRIED mid with this receiver — the bare
                // entry would be the this=undefined TypeError.
                // Array-family mids run the ES generic array-like
                // semantics over this receiver; every other carried
                // mid (the §20.1.3.6 badge cell, per-type methods)
                // re-enters the dispatcher with the boxed receiver
                // (RFC 20260713 blade 2 — a miss there stays loud).
                if crate::nanbox::is_cell(cell)
                    && let Some(mid2) =
                        crate::method_value::builtin_method_mid(crate::nanbox::as_void_ptr(cell))
                {
                    if crate::method_call_arraylike::arraylike_supported(mid2) {
                        return crate::method_call_arraylike::arraylike_method(
                            obj, mid2, recv_slot, argv, argc,
                        );
                    }
                    return crate::method_call::any_method_call_inner(
                        __torajs_anyv_box_pointer(obj),
                        mid2,
                        core::ptr::null(),
                        recv_slot,
                        argv,
                        argc,
                    );
                }
                // The cell's NaN-box encoding is its pointer bits.
                if let Some((env, entry)) = closure_boxed_entry(cell) {
                    return crate::method_call::invoke_boxed(env, entry, argv, argc);
                }
            }
            // C4+ chunk 523 — getter-as-callee: an accessor entry's
            // getter runs first, its (owned) answer dispatches as
            // the callee, and the reference releases after the call
            // (the invoke keeps the cell alive across it).
            if dtag == ANY_ACCESSOR_TAG {
                let pair = __torajs_dynobj_get_value(obj, key) as *const c_void;
                let got = __torajs_accessor_invoke_getter(pair);
                if let Some((env, entry)) = closure_boxed_entry(got) {
                    let r = crate::method_call::invoke_boxed(env, entry, argv, argc);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
                    return r;
                }
                crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
            }
        }
        not_callable()
    }
}

/// `Tag::Arr` own-expando probe (RFC 20260713-array-proto-residual
/// blade 2) — §10.1.8.1 OrdinaryGet: an own arr-props entry shadows
/// the built-in method surface (`arr.getClass = Object.prototype.
/// toString; arr.getClass()` / a user closure stored on the array).
/// `None` = no own entry, the caller proceeds to the builtin mid
/// dispatch; `Some` = the entry resolved (invoked, or the
/// resolved-but-not-callable TypeError fired — a shadowing
/// non-callable must NOT fall through to the builtin, per spec).
pub(crate) unsafe fn arr_expando_method(
    arr: *mut c_void,
    recv: AnyValue,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let key = name_str as *const c_void;
        let dtag = __torajs_arrprops_get_tag(arr, key);
        if dtag == 5 {
            return None;
        }
        // ANY_HEAP = 4 — a closure-cell property.
        if dtag == 4 {
            let cell = __torajs_arrprops_get_value(arr, key);
            // A reified builtin cell re-dispatches its CARRIED mid
            // with this receiver (NULL name — the mid is
            // authoritative; the badge / array-family arms apply).
            if crate::nanbox::is_cell(cell)
                && let Some(mid2) =
                    crate::method_value::builtin_method_mid(crate::nanbox::as_void_ptr(cell))
            {
                return Some(crate::method_call::any_method_call_inner(
                    recv,
                    mid2,
                    core::ptr::null(),
                    recv_slot,
                    argv,
                    argc,
                ));
            }
            if let Some((env, entry)) = closure_boxed_entry(cell) {
                return Some(crate::method_call::invoke_boxed(env, entry, argv, argc));
            }
        }
        // Accessor entry — getter runs first, its (owned) answer
        // dispatches as the callee (mirror of the dynobj arm).
        if dtag == ANY_ACCESSOR_TAG {
            let pair = __torajs_arrprops_get_value(arr, key) as *const c_void;
            let got = __torajs_accessor_invoke_getter(pair);
            if let Some((env, entry)) = closure_boxed_entry(got) {
                let r = crate::method_call::invoke_boxed(env, entry, argv, argc);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
                return Some(r);
            }
            crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
        }
        Some(not_callable())
    }
}

/// RFC 20260712 chunk C fix — OrdinaryToPrimitive's IsCallable
/// probe: true iff the receiver has an OWN entry under `name_str`
/// whose value is NOT callable. §7.1.1.1 skips such an entry and
/// tries the next method (`{toString: void 0, valueOf: fn}`), while
/// an explicit `o.toString()` method call keeps the TypeError —
/// so the skip lives here as a probe, not inside the dispatch.
/// Absent entries answer false (the inherited prototype surface is
/// callable); accessor entries answer false (the getter-as-callee
/// dispatch path resolves them).
#[cfg_attr(test, allow(dead_code))] // test builds stub the to_primitive probe
pub(crate) unsafe fn own_entry_not_callable(
    obj: *mut c_void,
    is_struct: bool,
    name_str: *const u8,
) -> bool {
    unsafe {
        if is_struct {
            let class_tag = (obj.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read();
            let layout = __torajs_struct_layout_lookup(class_tag);
            if layout.is_null() {
                return false;
            }
            let name_len = (name_str.add(STR_LEN_OFF) as *const u32).read();
            let name_bytes = name_str.add(STR_DATA_OFF);
            let idx = __torajs_struct_field_find(layout, name_bytes, name_len);
            if idx == u32::MAX {
                return false;
            }
            let info = __torajs_struct_field_info(layout, idx);
            let raw = (obj.cast::<u8>().add(info.field_byte_offset as usize) as *const u64).read();
            let pair = match info.type_tag {
                FIELD_TAG_ANY => closure_boxed_entry(raw),
                FIELD_TAG_CLOSURE => closure_cell_entry(raw as *mut c_void),
                _ => None,
            };
            pair.is_none()
        } else {
            let key = name_str as *const c_void;
            let dtag = __torajs_dynobj_get_tag(obj, key);
            if dtag == 5 {
                // Absent own key: an ordinary dict inherits the
                // builtin Object.prototype method surface, so the
                // dispatch proceeds — unless this is a null-
                // prototype dict (`Object.create(null)`), which
                // inherits nothing (mirror of torajs-dynobj's
                // DYNOBJ_HDR_FLAG_NULL_PROTO, header bit 6).
                let flags = (obj.cast::<u8>().add(6) as *const u16).read();
                return flags & (1 << 6) != 0;
            }
            if dtag == ANY_ACCESSOR_TAG {
                return false;
            }
            if dtag == 4 {
                let cell = __torajs_dynobj_get_value(obj, key);
                return closure_boxed_entry(cell).is_none();
            }
            true
        }
    }
}

/// `Tag::Obj` arm — see module doc. `obj` is the struct cell, and
/// the field slot read is a borrow (the receiver owns it and
/// outlives the call).
pub(crate) unsafe fn struct_method(
    obj: *mut c_void,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // 刀 2 (RFC 20260714-t262-top-clusters) — a NULL-name entry
        // is the reified-cell `.call` re-dispatch; an array-family
        // mid runs the ES generic array-like semantics over the
        // struct receiver (index/length reads route the chunk-744
        // struct arms). Mutators stay excluded: a static-layout
        // struct has no growable props bag to relocate.
        if name_str.is_null()
            && crate::method_call_arraylike::arraylike_supported(mid)
            && !crate::method_call_arraylike_mut::arraylike_mut_supported(mid)
        {
            return crate::method_call_arraylike::arraylike_method(
                obj,
                mid,
                core::ptr::null_mut(),
                argv,
                argc,
            );
        }
        if !name_str.is_null() {
            let class_tag = (obj.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read();
            let layout = __torajs_struct_layout_lookup(class_tag);
            if !layout.is_null() {
                let name_len = (name_str.add(STR_LEN_OFF) as *const u32).read();
                let name_bytes = name_str.add(STR_DATA_OFF);
                let idx = __torajs_struct_field_find(layout, name_bytes, name_len);
                if idx != u32::MAX {
                    let info = __torajs_struct_field_info(layout, idx);
                    let raw = (obj.cast::<u8>().add(info.field_byte_offset as usize) as *const u64)
                        .read();
                    let pair = match info.type_tag {
                        // The slot is itself a NaN-box AnyValue.
                        FIELD_TAG_ANY => closure_boxed_entry(raw),
                        // The slot is the raw closure env cell.
                        FIELD_TAG_CLOSURE => closure_cell_entry(raw as *mut c_void),
                        _ => None,
                    };
                    if let Some((env, entry)) = pair {
                        return crate::method_call::invoke_boxed(env, entry, argv, argc);
                    }
                    // A resolved field that isn't callable keeps the
                    // TypeError — never shadowed by the fallback.
                    return not_callable();
                }
                // 刀 4 (RFC 20260714-t262-top-clusters) — no such
                // FIELD: probe the class-methods dispatch table. A
                // hit invokes the `__cm_<C>__<m>` body through its
                // boxed adapter with the instance in the env slot
                // (the adapter feeds it into the `__this` param).
                //
                // RFC 20260714-objlit-accessor blade 5 — class
                // ACCESSORS ride the same table under `__getter_<p>`,
                // and that spelling is not a callable method name:
                // `o.__getter_b()` keeps the honest no-such TypeError
                // (the property read reaches the getter, the call does
                // not). A user method really named `b` still wins here.
                let adapter = if __torajs_accessor_name_kind(name_bytes, name_len) == 255 {
                    __torajs_struct_method_find(layout, name_bytes, name_len)
                } else {
                    core::ptr::null()
                };
                if !adapter.is_null() {
                    return crate::method_call::invoke_boxed(obj, adapter as u64, argv, argc);
                }
            }
        }
        // No layout / absent field → the inherited Object.prototype
        // surface, mirroring the dynobj arm's absent branch.
        object_proto_fallback(obj, mid, true, argv)
    }
}

/// Inherited `Object.prototype` surface for plain-object receivers —
/// runs only AFTER the own-property probe missed, so user
/// monkey-patches always win. `valueOf` (§20.1.4.7) answers the
/// receiver itself (fresh +1 per the boxed-value convention);
/// `toString` (§20.1.3.6) answers the "[object Object]" text;
/// `toLocaleString` (§20.1.4.6 "invoke this.toString") re-dispatches
/// as a toString call so a user own `toString` wins there too.
unsafe fn object_proto_fallback(
    obj: *mut c_void,
    mid: i64,
    is_struct: bool,
    argv: *const u64,
) -> AnyValue {
    unsafe {
        // ES §21.1.3 / §20.3.3 / §22.1.3 — `Number.prototype` IS a Number
        // object ([[NumberData]] = +0), `Boolean.prototype` a Boolean one
        // (false), `String.prototype` a String one (""). tr builds every
        // builtin prototype as an empty dynobj, so `Number.prototype
        // .toString()` fell through to here and answered "[object Object]"
        // where the spec says "0" — which is the FIRST assertion of most
        // Number/prototype/toString cases, so the whole family died on it.
        // Ordered after the own probe like the rest of this fallback: a
        // monkey-patched `Number.prototype.toString` still wins.
        if !is_struct && let Some(v) = builtin_proto_primitive(obj, mid) {
            return v;
        }
        if mid == ANY_METHOD_VALUE_OF {
            __torajs_rc_inc(obj);
            return __torajs_anyv_box_pointer(obj);
        }
        if mid == ANY_METHOD_TO_LOCALE_STRING {
            let key = __torajs_str_alloc(b"toString".as_ptr(), 8);
            let out = if is_struct {
                struct_method(obj, ANY_METHOD_TO_STRING, key as *const u8, argv, 0)
            } else {
                dynobj_method(
                    obj,
                    ANY_METHOD_TO_STRING,
                    key as *const u8,
                    core::ptr::null_mut(),
                    argv,
                    0,
                )
            };
            __torajs_str_drop(key as *mut c_void);
            return out;
        }
        if mid == ANY_METHOD_TO_STRING {
            // Error.prototype.toString (§20.5.3.4) — a FLAG_ERROR
            // struct answers `name: message` (matching the SSA
            // typed-tier lowering), not the "[object Object]" badge.
            // After the own-property probe, so a monkey-patched own
            // `toString` still wins.
            if is_struct
                && let Some(v) = crate::method_call_object_proto::error_struct_tostring(obj)
            {
                return v;
            }
            // §20.1.3.6 through the badge classifier rather than a
            // hardcoded "[object Object]": the five container
            // prototypes carry a well-known `Symbol.toStringTag`, so
            // `Map.prototype.toString()` is "[object Map]".
            return crate::method_call_object_proto::cell_badge_string(obj, is_struct);
        }
        not_callable()
    }
}

/// The primitive a builtin prototype carries as its internal slot, for
/// the two methods that read it. `None` for every other receiver (and
/// for the prototypes with no primitive data — `Object.prototype`,
/// `Map.prototype`, ... — which keep the ordinary Object.prototype
/// surface), so the caller falls through unchanged.
///
/// `Array.prototype` is deliberately absent: the spec makes it an ARRAY
/// (an empty one), not a primitive wrapper, so `Array.prototype
/// .toString()` answering "" has to come from the array lane, not here.
unsafe fn builtin_proto_primitive(obj: *mut c_void, mid: i64) -> Option<AnyValue> {
    // Tags are ssa_lower's, fixed by `torajs_rc::builtin_proto`:
    // Number=0, String=3, Boolean=4.
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(obj) };
    let is_to_string = mid == ANY_METHOD_TO_STRING;
    if !is_to_string && mid != ANY_METHOD_VALUE_OF {
        return None;
    }
    match tag {
        // §21.1.3.6 — `Number.prototype.toString(radix)` on +0 is "0" in
        // every radix, so the arg needs no inspection.
        0 if is_to_string => Some(unsafe { str_any(b"0") }),
        0 => Some(crate::nanbox_encode::__torajs_anyv_box_f64(0.0)),
        3 if is_to_string => Some(unsafe { str_any(b"") }),
        3 => Some(unsafe { str_any(b"") }),
        4 if is_to_string => Some(unsafe { str_any(b"false") }),
        4 => Some(crate::nanbox_encode::__torajs_anyv_box_bool(0)),
        _ => None,
    }
}

/// A fresh owned Str cell boxed as an AnyValue.
unsafe fn str_any(bytes: &[u8]) -> AnyValue {
    unsafe {
        let p = __torajs_str_alloc(bytes.as_ptr(), bytes.len() as i64);
        __torajs_anyv_box_pointer(p as *mut c_void)
    }
}
