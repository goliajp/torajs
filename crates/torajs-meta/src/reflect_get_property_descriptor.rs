//! `Object.getOwnPropertyDescriptor(obj, key)` — extracted from
//! [`crate::reflect`] as the rotation-196 file-size sweep. The parent
//! module had drifted to 519 LOC over the descriptor-family chunk
//! wave (rotation 185 audit); this pulls the 200 LOC extern fn +
//! its 35 LOC `builtin_proto_descriptor` helper into a sibling so
//! `reflect.rs` returns to a "constants + shared descriptor builders
//! + `Object.getPrototypeOf`" shape while the ~248 LOC descriptor
//! dispatch cascade lives here.
//!
//! Nothing about the dispatch is refactored — arms retain their
//! rotation-record comments and per-tag ordering (struct → closure →
//! regex → array → primitive-wrapper delegation → StringWrapper
//! inherent props → dynobj expando → builtin proto tail). Callers
//! (both external, via `lib.rs`'s `pub use`, and cross-crate C
//! callers via the `#[unsafe(no_mangle)]` symbol) hit the same
//! function; the fn body reads exactly the constants + builders
//! + externs that `crate::reflect` still exposes at `pub(crate)`.

use core::ffi::c_void;

use crate::reflect::{
    __torajs_accessor_get_getter, __torajs_accessor_get_setter, __torajs_anyv_unbox_tag,
    __torajs_anyv_unbox_value, __torajs_builtin_proto_own_accessor_getter,
    __torajs_builtin_proto_own_meta, __torajs_builtin_proto_own_method_cell, __torajs_dynobj_alloc,
    __torajs_dynobj_get_flags, __torajs_dynobj_get_tag, __torajs_dynobj_get_value,
    __torajs_dynobj_has, __torajs_dynobj_set, __torajs_rc_inc, __torajs_regex_get_last_index,
    __torajs_regex_last_index_raw, __torajs_str_drop, __torajs_throw_type_error, ANY_ACCESSOR,
    ANY_BOOL, ANY_HEAP, ANY_UNDEF, TAG_ARR, TAG_CLOSURE, TAG_DYNOBJ, TAG_OBJ, TAG_REGEXP,
    TAG_STRING_WRAPPER, VALUE_NULL_IMM, VALUE_UNDEFINED_IMM, WRAPPER_PROPS_OFF, alloc_str_key,
    build_accessor_descriptor, build_data_descriptor, heap_type_tag, is_cell_imm, is_wrapper_tag,
};

unsafe extern "C" {
    /// Hands back an OWNED payload — a ShortStr immediate
    /// materializes as a fresh Str cell at rc=1, which is the only
    /// way to give the char-index helper a cell to read.
    fn __torajs_anyv_unbox_value_owned(v: u64) -> i64;
    /// torajs-anyvalue — §10.5.5 [[GetOwnProperty]] on a Proxy.
    fn __torajs_proxy_get_own_descriptor(obj_any: u64, key: *const c_void) -> u64;
}

/// `torajs_dynobj::layout::TAG_SYMBOL_KEY` mirror — a property key
/// cell is a Str (tag 0) or a Symbol (tag 7) per §6.1.7.
const TAG_SYMBOL_KEY: u16 = 7;

/// `torajs_rc::Tag::Str` — the bare string cell.
const TAG_STR_CELL: u16 = 0;

/// The top 16 bits an SSO ShortStr immediate carries, compared
/// against `v >> 48`. `reflect::SHORT_STR_TOP16` spells the same
/// thing as a full-width mask, which is a different comparison —
/// `obj_own_keys` uses this shifted form and so does the arm below.
const SHORT_STR_TOP: u64 = 0x0001;

/// `torajs-str::layout` mirrors — the u32 length at +8 and the
/// payload bytes at +16.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// Buffer-family cells and their lazy expando slots (torajs-buffer
/// `arraybuffer.rs` / `typedarray.rs::PROPS_OFF` mirrors).
const TAG_ARRAYBUFFER: u16 = 27;
pub(crate) const TAG_TYPEDARRAY: u16 = 28;
/// DataView shares the ArrayBuffer bag offset (+32) by construction.
const TAG_DATAVIEW: u16 = 29;
pub(crate) const ARRAYBUFFER_PROPS_OFF: usize = 32;
pub(crate) const TYPEDARRAY_PROPS_OFF: usize = 40;

/// True when the property-key cell is a Symbol rather than a Str.
///
/// # Safety
/// `key` must be non-NULL and point at a live key cell.
#[inline]
unsafe fn key_is_symbol_cell(key: *const c_void) -> bool {
    unsafe { heap_type_tag(key) == TAG_SYMBOL_KEY }
}

/// Descriptor for a **symbol** key on a non-DynObj receiver: read it
/// out of the receiver's in-layout property dict, or `undefined`.
///
/// A Symbol key can only ever name an ENTRY-TABLE own property, since
/// every inherent / virtual own property tr synthesizes in the
/// name-keyed cascade (struct fields, fn `name` / `length`, RegExp
/// `lastIndex`, array `length` + canonical indices, wrapper `length` +
/// chars, builtin-prototype methods) is string-keyed by construction.
/// Routing the symbol here is also what keeps those arms from reading
/// Str payload offsets off a 16-byte Symbol cell.
///
/// # Safety
/// `dynobj` is a live cell whose `type_tag` is `htag`; `key` is a live
/// Symbol cell.
unsafe fn symbol_key_descriptor_via_dict(
    dynobj: *const c_void,
    htag: u16,
    key: *const c_void,
) -> u64 {
    // A class instance carries expandos in the same +24 dict (RFC
    // 20260714-struct-dynamic-props), and a symbol-keyed one has
    // nowhere else it could live — the layout metadata is name-keyed
    // by construction. Every shape reads the one shared table, so a
    // cell that grows a bag answers gOPD in the same line that makes
    // it writable.
    let props = unsafe { crate::obj_own_keys_layout::expando_props(dynobj, htag) };
    if props.is_null() {
        return VALUE_UNDEFINED_IMM;
    }
    unsafe { __torajs_anyv_get_property_descriptor(props as u64, key) }
}

/// §22.1.4.4 [[GetOwnProperty]] on a String — `length`, and the
/// canonical indices below it. Every other key is prototype surface,
/// absent as own.
///
/// Shared by the two spellings a string receiver arrives in: a heap
/// Str cell and a ShortStr immediate. The lowering has compile-time
/// fast paths for both descriptors, but they need a LITERAL key and a
/// statically `Type::Str` receiver — an `any`-typed receiver or a
/// runtime key has always fallen through to this kernel, which had no
/// arm for a string at all and answered `undefined` for a property
/// that was right there. `getOwnPropertyDescriptors` reaches it the
/// same way, one key at a time.
///
/// # Safety
/// `str_ptr` is a live Str cell of `len` code units; `key` is a live
/// Str cell.
unsafe fn str_cell_descriptor(str_ptr: *const c_void, len: u64, key: *const c_void) -> u64 {
    let k = key as *const u8;
    let key_len = unsafe { k.add(STR_LEN_OFF).cast::<u32>().read() } as usize;
    let bytes = unsafe { core::slice::from_raw_parts(k.add(STR_DATA_OFF), key_len) };
    if bytes == b"length" {
        return unsafe { build_data_descriptor(2, len, 0, 0, 0) };
    }
    if let Some(idx) = crate::arr_reflect::canonical_index(bytes)
        && idx < len
    {
        return unsafe {
            crate::str_descriptor::__torajs_anyv_str_index_descriptor(str_ptr, idx as i64)
        };
    }
    VALUE_UNDEFINED_IMM
}

/// [`str_cell_descriptor`] for the immediate spelling. A ShortStr
/// rides inline in the AnyValue, so it never clears the cell test in
/// the caller; its chars still have descriptors. The reader wants a
/// cell, so the immediate materializes into one and is released
/// straight after.
///
/// # Safety
/// `obj_any` is a ShortStr immediate; `key` is a live Str cell.
unsafe fn short_str_descriptor(obj_any: u64, key: *const c_void) -> u64 {
    let len = (obj_any >> 40) & 0xFF;
    let owned = unsafe { __torajs_anyv_unbox_value_owned(obj_any) } as *mut c_void;
    if owned.is_null() {
        return VALUE_UNDEFINED_IMM;
    }
    let d = unsafe { str_cell_descriptor(owned, len, key) };
    unsafe { __torajs_str_drop(owned as *mut u8) };
    d
}

/// Spec §20.1.2.8 step 1 — `Let obj be ? ToObject(O)`. ToObject on
/// `undefined` / `null` throws a TypeError; every other primitive
/// (number / boolean / string) boxes to a wrapper and falls through
/// to the arms below.
///
/// bun JSC msg shape: `<type> is not an object (evaluating '...')`.
/// We align the prefix; the `(evaluating ...)` suffix needs
/// source-text threading through the throw helper (substrate work,
/// deferred).
///
/// # Safety
/// `obj_any` carries a valid AnyValue bit pattern.
unsafe fn to_object_rejects(obj_any: u64) -> bool {
    if obj_any == VALUE_UNDEFINED_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe { __torajs_throw_type_error(c"undefined is not an object".as_ptr()) };
        return true;
    }
    if obj_any == VALUE_NULL_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe { __torajs_throw_type_error(c"null is not an object".as_ptr()) };
        return true;
    }
    false
}

/// AnyValue-immediate `Object.getOwnPropertyDescriptor(obj, key)`
/// — builds a fresh dynobj `{ value, writable, enumerable,
/// configurable }` from the source dynobj's slot, returns it as
/// a NaN-box `AnyValue` cell. ANY_HEAP-tagged slot values are
/// rc-incremented so the descriptor `value` field owns its share
/// independently of the source.
///
/// # Safety
///
/// `obj_any` carries a valid AnyValue bit pattern; `key` is NULL
/// or a valid Str pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_get_property_descriptor(
    obj_any: u64,
    key: *const c_void,
) -> u64 {
    if unsafe { to_object_rejects(obj_any) } || key.is_null() {
        return VALUE_UNDEFINED_IMM;
    }
    if obj_any >> 48 == SHORT_STR_TOP && !unsafe { key_is_symbol_cell(key) } {
        return unsafe { short_str_descriptor(obj_any, key) };
    }
    if !is_cell_imm(obj_any) {
        return VALUE_UNDEFINED_IMM;
    }
    let dynobj = obj_any as *const c_void;
    // SAFETY: cell pointer to valid heap object.
    let htag = unsafe { heap_type_tag(dynobj) };
    // §10.5.5 — a Proxy answers its own descriptor (the
    // `getOwnPropertyDescriptor` trap, completed per §6.2.6.6).
    if htag == crate::reflect::TAG_PROXY {
        return unsafe { __torajs_proxy_get_own_descriptor(obj_any, key) };
    }
    // §6.1.7 — a symbol key skips the name-keyed cascade entirely.
    if unsafe { key_is_symbol_cell(key) } && htag != TAG_DYNOBJ {
        return unsafe { symbol_key_descriptor_via_dict(dynobj, htag, key) };
    }
    // W-J Phase B — static-layout struct cell reads the field via the
    // class_layouts metadata instead of the dynobj hash table. SAFETY:
    // `dynobj` is a live Tag::Obj cell; `key` is non-NULL (checked above).
    if htag == TAG_OBJ {
        return unsafe { crate::struct_reflect::struct_cell_descriptor(dynobj, key) };
    }
    // RFC 20260711 chunk B — Closure cell answers the virtual ES
    // §20.2.4 name/length descriptors (expando entries win). SAFETY:
    // `dynobj` is a live Tag::Closure cell; `key` non-NULL (above).
    if htag == TAG_CLOSURE {
        let d = unsafe { crate::closure_reflect::closure_cell_descriptor(dynobj, key) };
        if d != VALUE_UNDEFINED_IMM {
            return d;
        }
        // `Function.prototype` is a Closure cell too (§20.2.3 makes it
        // a built-in FUNCTION object), and its interned family
        // methods are own properties that live in no entry table —
        // the same synthesis the `Array.prototype` Arr cell gets
        // below, for the same reason.
        return unsafe { builtin_proto_descriptor(dynobj, key) };
    }
    // The shapes whose whole own face is the bag (which ones, and
    // why, is `obj_own_keys_layout::is_bag_only_tag`).
    if crate::obj_own_keys_layout::is_bag_only_tag(htag) {
        return unsafe { bag_only_descriptor(dynobj, htag, key) };
    }
    // §25.1 / §23.2 — buffer-family expando bag (body in
    // `buffer_reflect.rs`, file-size split).
    if htag == TAG_ARRAYBUFFER || htag == TAG_TYPEDARRAY || htag == TAG_DATAVIEW {
        return unsafe { crate::buffer_reflect::buffer_cell_descriptor(dynobj, htag, key) };
    }
    // RFC 20260722 刀 4 — a RegExp instance owns exactly its
    // `lastIndex` (§22.2.4.1 RegExpAlloc: {writable: true,
    // enumerable: false, configurable: false}); every other key is
    // prototype surface, absent as own.
    if htag == TAG_REGEXP {
        return unsafe { regexp_cell_descriptor(dynobj, key) };
    }
    // RFC 20260712-arr-exotic-define chunk A — Array cell answers the
    // §10.4.2 length / canonical-index / expando descriptors. SAFETY:
    // `dynobj` is a live Tag::Arr cell; `key` non-NULL (above).
    if htag == TAG_ARR {
        let d = unsafe { crate::arr_reflect::arr_cell_descriptor(dynobj, key) };
        if d != VALUE_UNDEFINED_IMM {
            return d;
        }
        // `Array.prototype` is an Arr cell too (ES §23.1.3), and its
        // interned family methods are own properties that live in no
        // entry table — same synthesis the dynobj protos get below.
        return unsafe { builtin_proto_descriptor(dynobj, key) };
    }
    // RFC 20260716 刀 5 continuation (rotation 121, chunk following
    // 4+5) — primitive-wrapper own-property probe. Expando entry
    // wins first via delegation to the DynObj descriptor path
    // (accessor entries / attribute flags handled there), mirroring
    // `closure_reflect.rs` order. StringWrapper's §22.1.4.1 `length`
    // and §22.1.4.4 char-index inherent props take over on miss;
    // Number/Boolean wrappers have no inherent own props so a miss
    // is `undefined` per §21.1.4/§20.3.4.
    if is_wrapper_tag(htag) {
        let props =
            unsafe { (dynobj.cast::<u8>().add(WRAPPER_PROPS_OFF) as *const *const c_void).read() };
        if !props.is_null() && unsafe { __torajs_dynobj_has(props, key as *const u8) } {
            return unsafe { __torajs_anyv_get_property_descriptor(props as u64, key) };
        }
    }
    // RFC 20260716 刀 14 — StringWrapper cell: `length` is a data
    // descriptor per ES §22.1.4.1 `{value: len, writable: false,
    // enumerable: false, configurable: false}`. `[idx]` character
    // descriptors are a follow-up (their `value` is a ShortStr
    // NaN-box immediate that doesn't fit the (tag, value) shape
    // `build_data_descriptor` takes).
    if htag == TAG_STRING_WRAPPER {
        // Key Str payload as a byte slice — mirrors `arr_reflect::key_bytes`.
        // SAFETY: `key` is a live Str cell (Tag::Str layout: `len: u32`
        // at offset 8; payload at offset 16).
        let key_len = unsafe { key.cast::<u8>().add(8).cast::<u32>().read() } as usize;
        let key_data = unsafe { key.cast::<u8>().add(16) };
        let bytes = unsafe { core::slice::from_raw_parts(key_data, key_len) };
        // Inner Str cell at STRING_WRAPPER_CELL_OFF = 8; NULL sentinel
        // (`new String()` no-arg) has length 0. Shared by both the
        // `length` arm and the char-index arm below.
        let inner_ptr = unsafe { (dynobj.cast::<u8>().add(8) as *const *const c_void).read() };
        let inner_len = if inner_ptr.is_null() {
            0u64
        } else {
            // Tag::Str layout: `len: u32 @ offset 8`.
            unsafe { inner_ptr.cast::<u8>().add(8).cast::<u32>().read() as u64 }
        };
        if bytes == b"length" {
            return unsafe { build_data_descriptor(2, inner_len, 0, 0, 0) };
        }
        // RFC 20260716 刀 16 — StringWrapper char-index descriptor.
        // ES §22.1.4.4 [[GetOwnProperty]] returns for `"<idx>"` a data
        // property `{value: char, writable: false, enumerable: true,
        // configurable: false}` when `idx` is a canonical numeric index
        // (§7.1.22) less than the wrapped string's length. Because the
        // char `value` is a fresh Str cell (single code unit) rather
        // than a `(tag, immediate)` pair, we delegate to the shared
        // char-index helper in `str_descriptor.rs` which owns the alloc
        // + slot-set sequence for that case.
        if let Some(idx) = crate::arr_reflect::canonical_index(bytes) {
            if !inner_ptr.is_null() && idx < inner_len {
                return unsafe {
                    crate::str_descriptor::__torajs_anyv_str_index_descriptor(inner_ptr, idx as i64)
                };
            }
        }
        // Expando entries + proto walk still deferred (L3b).
        return VALUE_UNDEFINED_IMM;
    }
    // A bare Str cell — the heap spelling of the arm above.
    if htag == TAG_STR_CELL {
        let len = unsafe { dynobj.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() } as u64;
        return unsafe { str_cell_descriptor(dynobj, len, key) };
    }
    if htag != TAG_DYNOBJ {
        return VALUE_UNDEFINED_IMM;
    }
    let k_str = key as *const u8;
    if !unsafe { __torajs_dynobj_has(dynobj, k_str) } {
        // A missing symbol key stops here: `builtin_proto_descriptor`
        // synthesizes name-keyed prototype methods and reads the key's
        // Str payload to do it (see the §6.1.7 note above).
        if unsafe { key_is_symbol_cell(key) } {
            return VALUE_UNDEFINED_IMM;
        }
        return unsafe { builtin_proto_descriptor(dynobj, key) };
    }
    let v_tag = unsafe { __torajs_dynobj_get_tag(dynobj, k_str) };
    let v_val = unsafe { __torajs_dynobj_get_value(dynobj, k_str) };
    let flags = unsafe { __torajs_dynobj_get_flags(dynobj, k_str) };

    // RFC C3 — accessor entry: report `{ get, set, enumerable,
    // configurable }` (no value/writable). `v_val` is the AccessorPair
    // pointer; the getter/setter closures are returned as ANY_HEAP
    // cells (the desc owns a fresh ref to each) or `undefined`.
    if v_tag == ANY_ACCESSOR {
        let pair = v_val as *const c_void;
        let getter = unsafe { __torajs_accessor_get_getter(pair) };
        let setter = unsafe { __torajs_accessor_get_setter(pair) };
        let (get_t, get_v) = if getter.is_null() {
            (ANY_UNDEF as u64, 0u64)
        } else {
            unsafe { __torajs_rc_inc(getter) };
            (ANY_HEAP as u64, getter as u64)
        };
        let (set_t, set_v) = if setter.is_null() {
            (ANY_UNDEF as u64, 0u64)
        } else {
            unsafe { __torajs_rc_inc(setter) };
            (ANY_HEAP as u64, setter as u64)
        };
        return unsafe {
            build_accessor_descriptor(
                get_t,
                get_v,
                set_t,
                set_v,
                (flags >> 1) & 1,
                (flags >> 2) & 1,
            )
        };
    }

    if v_tag as i64 == ANY_HEAP && v_val != 0 {
        // SAFETY: ANY_HEAP slot holds a valid heap pointer — the
        // source dynobj keeps its share, the descriptor owns a
        // fresh one.
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }
    // desc owns rc=1 from dynobj_alloc; transferred to caller
    // via the returned cell-encoded AnyValue (pre-7d the AnyBox-
    // wrapped path rc_inc'd + dropped the local; both cancel
    // out and we skip both).
    unsafe { build_data_descriptor(v_tag, v_val, flags & 1, (flags >> 1) & 1, (flags >> 2) & 1) }
}

/// §22.2.4.1 — the RegExp cell's one own property. A non-numeric
/// any-lane store to `lastIndex` reads back VERBATIM (boxed overflow
/// slot; the descriptor takes its own stake on a heap cell); the
/// numeric form boxes the f64. Every other key is absent.
///
/// # Safety
/// `cell` is a live `Tag::RegExp` cell; `key` a live Str cell.
unsafe fn regexp_cell_descriptor(cell: *const c_void, key: *const c_void) -> u64 {
    unsafe {
        if !crate::closure_reflect::key_is(key, b"lastIndex") {
            // Every other own name a RegExp carries lives in the bag
            // the assign ladder writes (§22.2.6 ordinary face).
            return bag_only_descriptor(cell, TAG_REGEXP, key);
        }
        let raw = __torajs_regex_last_index_raw(cell);
        if raw != 0 {
            if is_cell_imm(raw) {
                __torajs_rc_inc(raw as *mut c_void);
            }
            let t = __torajs_anyv_unbox_tag(raw) as u64;
            let v = __torajs_anyv_unbox_value(raw) as u64;
            return build_data_descriptor(t, v, 1, 0, 0);
        }
        let li = __torajs_regex_get_last_index(cell);
        build_data_descriptor(3, li.to_bits(), 1, 0, 0)
    }
}

/// The whole own face of a cell whose properties live only in its
/// lazy bag — `undefined` for a shape that carries no bag. Shared
/// body in `buffer_reflect.rs`; also the tail of the RegExp arm,
/// whose one cell-held name (`lastIndex`) is answered before it.
///
/// # Safety
/// `cell` is a live heap cell whose header tag is `htag`; `key` is a
/// live Str or Symbol cell.
unsafe fn bag_only_descriptor(cell: *const c_void, htag: u16, key: *const c_void) -> u64 {
    let Some(off) = crate::obj_own_keys_layout::expando_props_off(htag) else {
        return VALUE_UNDEFINED_IMM;
    };
    unsafe { crate::buffer_reflect::expando_bag_descriptor(cell, off, key) }
}

/// The descriptor a builtin `<Ctor>.prototype` singleton owes for a
/// key it holds in no entry table (RFC 20260712 chunk 2) — its
/// interned family methods, and the Map/Set `size` accessor, are own
/// properties that live in the method-cell table. `undefined` for
/// every other receiver, so an ordinary cell falls through unchanged.
///
/// # Safety
/// `proto` is a live heap cell (only compared, never dereferenced by
/// the probes); `key` is a live Str cell.
unsafe fn builtin_proto_descriptor(proto: *const c_void, key: *const c_void) -> u64 {
    // Interned method → {writable: true, enumerable: false,
    // configurable: true}. The cell is immortal, so the descriptor's
    // value slot taking heap ownership is a no-op.
    let cell = unsafe { __torajs_builtin_proto_own_method_cell(proto, key) };
    if cell != 0 {
        return unsafe { build_data_descriptor(ANY_HEAP as u64, cell, 1, 0, 1) };
    }
    // Function.prototype's virtual own name/length pair (§20.2.3,
    // RFC 20260722 刀 3) → {writable: false, enumerable: false,
    // configurable: true}; the name Str is immortal.
    let (mut m_tag, mut m_val) = (0u64, 0u64);
    if unsafe { __torajs_builtin_proto_own_meta(proto, key, &mut m_tag, &mut m_val) } != 0 {
        return unsafe { build_data_descriptor(m_tag, m_val, 0, 0, 1) };
    }
    // C2-size — the Map/Set `size` own accessor → {get, set:
    // undefined, enumerable: false, configurable: true}. The getter
    // cell is immortal, so the get slot taking ownership is a no-op.
    let getter = unsafe { __torajs_builtin_proto_own_accessor_getter(proto, key) };
    if getter == 0 {
        return VALUE_UNDEFINED_IMM;
    }
    let mut desc = unsafe { __torajs_dynobj_alloc() };
    let acc_entries: [(&[u8], u64, u64); 4] = [
        (b"get", ANY_HEAP as u64, getter),
        (b"set", ANY_UNDEF as u64, 0),
        (b"enumerable", ANY_BOOL as u64, 0),
        (b"configurable", ANY_BOOL as u64, 1),
    ];
    for &(name, t, val) in acc_entries.iter() {
        let k = unsafe { alloc_str_key(name) };
        unsafe { __torajs_dynobj_set(&mut desc, k, t, val) };
        unsafe { __torajs_str_drop(k) };
    }
    desc as u64
}
