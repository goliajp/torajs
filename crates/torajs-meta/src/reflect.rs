//! Reflection helpers — `Object.getPrototypeOf(any)` +
//! `Object.getOwnPropertyDescriptor(obj, key)` — port of
//! `runtime_str.c` L494-549 + L881-915. Step 7 NaN-box cutover:
//! both helpers operate on NaN-box `AnyValue` immediates and
//! route through `torajs-dynobj` for the slot reads. Returned
//! values are always owned AnyValue immediates (caller takes
//! ownership).
//!
//! `get_property_descriptor` allocates a fresh dynobj with 4 fields
//! (`value` / `writable` / `enumerable` / `configurable`) before
//! returning it as an AnyValue cell. ANY_HEAP values in the source
//! dynobj are rc-incremented so the descriptor's `value` slot owns
//! its share independently.

use core::ffi::{c_char, c_void};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_flags(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_accessor_get_getter(pair: *const c_void) -> *mut c_void;
    fn __torajs_accessor_get_setter(pair: *const c_void) -> *mut c_void;
    // RFC 20260712 chunk 2 — builtin `<Ctor>.prototype` own-method
    // probe from torajs-anyvalue/method_support.rs. Non-zero = the
    // immortal interned method cell (rc traffic no-ops on it).
    fn __torajs_builtin_proto_own_method_cell(dynobj: *const c_void, key: *const c_void) -> u64;
    // set-proto-cluster C2-size — the Map/Set `size` own-accessor
    // probe (same crate). Non-zero = the immortal getter cell.
    fn __torajs_builtin_proto_own_accessor_getter(dynobj: *const c_void, key: *const c_void)
    -> u64;
    // torajs-rc — lazy `<Ctor>.prototype` singleton by builtin tag
    // (Array=2 / String=3 / RegExp=7 / Date=8 / Map=11 / Set=12 /
    // Function=13; `builtin_proto.rs` order).
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
}

// Tag values mirrored from torajs-anyvalue::AnySlotTag — re-declared
// here to keep this crate's dep tree narrow (no torajs-anyvalue
// Cargo dep; the i64 wire tag is part of the ABI anyway).
const ANY_BOOL: i64 = 1;
const ANY_HEAP: i64 = 4;
const ANY_UNDEF: i64 = 5;
/// `get_tag` accessor sentinel (mirrors `torajs_dynobj::layout::ANY_ACCESSOR`).
const ANY_ACCESSOR: u64 = 6;

// Tag::DynObj from torajs-rc — universal heap header at offset 0.
pub(crate) const TAG_DYNOBJ: u16 = 14;
// Tag::Obj — static-layout struct cell (W-J Phase B struct arm /
// Phase C struct enumeration arms in `struct_enum.rs`).
pub(crate) const TAG_OBJ: u16 = 1;
// Tag::Arr — array cell; gOPD routes to the length / canonical-index /
// expando arms in `arr_reflect.rs` (RFC 20260712-arr-exotic-define
// chunk A).
const TAG_ARR: u16 = 2;
// Tag::Closure — fn cell; gOPD routes to the virtual name/length
// descriptor arm in `closure_reflect.rs` (RFC 20260711 chunk B).
const TAG_CLOSURE: u16 = 3;
/// Tag::Str / Tag::Symbol / Tag::BigInt from `torajs-rc` — primitive-in-spec
/// heap cells. RFC C4b throws TypeError on these because `Object.defineProperty(O, ...)`
/// step 1 is a strict `Type(O) is Object` check (no ToObject wrapper boxing).
const TAG_STR: u16 = 0;
const TAG_SYMBOL: u16 = 7;
const TAG_BIGINT: u16 = 10;

#[inline]
pub(crate) unsafe fn heap_type_tag(child: *const c_void) -> u16 {
    // Universal heap header: refcount u32 at +0, type_tag u16 at +4.
    unsafe { child.cast::<u8>().add(4).cast::<u16>().read() }
}

#[inline]
pub(crate) unsafe fn alloc_str_key(name: &[u8]) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(name.len() as u64) };
    if !name.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(name.as_ptr(), s.add(16), name.len()) };
    }
    s
}

// NaN-box constants mirrored from torajs-anyvalue::nanbox.
// null / undefined shared with own_names.rs (W-N-c ToObject guard).
pub(crate) const VALUE_NULL_IMM: u64 = 0x02;
pub(crate) const VALUE_UNDEFINED_IMM: u64 = 0x0A;
const VALUE_FALSE_IMM: u64 = 0x06;
const VALUE_TRUE_IMM: u64 = 0x07;
const TAG_TYPE_NUMBER: u64 = 0xFFFE_0000_0000_0000;
const TAG_BIT_TYPE_OTHER: u64 = 0x02;
const DOUBLE_ENCODE_OFFSET: u64 = 0x0007_0000_0000_0000;
/// Step 8b-B — top-16-bit mask for strict cell detection. ShortStr
/// claims `top16 == 0x0001`; weak `& TAG_TYPE_NUMBER == 0` would
/// misclassify ShortStr as cell when its low byte happened to
/// clear bit 1 (e.g. `'a'` = 0x61).
const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;

#[inline]
pub(crate) const fn is_cell_imm(v: u64) -> bool {
    (v & TOP_16_MASK) == 0 && (v & TAG_BIT_TYPE_OTHER) == 0 && v != 0
}

/// Build a data descriptor dynobj `{ value, writable, enumerable,
/// configurable }` — the shared tail of every gOPD arm (general
/// dynobj walk / Arr length / Str length / struct field / closure
/// virtual pair). A heap-tagged `value` transfers ownership into the
/// descriptor (callers inc beforehand when the source keeps its own
/// reference).
pub(crate) unsafe fn build_data_descriptor(
    v_tag: u64,
    v_val: u64,
    writable: u64,
    enumerable: u64,
    configurable: u64,
) -> u64 {
    let mut desc = unsafe { __torajs_dynobj_alloc() };
    let entries: [(&[u8], u64, u64); 4] = [
        (b"value", v_tag, v_val),
        (b"writable", ANY_BOOL as u64, writable),
        (b"enumerable", ANY_BOOL as u64, enumerable),
        (b"configurable", ANY_BOOL as u64, configurable),
    ];
    for &(name, t, val) in entries.iter() {
        let k = unsafe { alloc_str_key(name) };
        unsafe { __torajs_dynobj_set(&mut desc, k, t, val) };
        unsafe { __torajs_str_drop(k) };
    }
    desc as u64
}

#[inline]
fn box_pair_imm(tag: i64, value: i64) -> u64 {
    match tag {
        0 => VALUE_NULL_IMM,
        1 => {
            if value != 0 {
                VALUE_TRUE_IMM
            } else {
                VALUE_FALSE_IMM
            }
        }
        2 => {
            if let Ok(n32) = i32::try_from(value) {
                TAG_TYPE_NUMBER | (n32 as u32 as u64)
            } else {
                (value as f64).to_bits().wrapping_add(DOUBLE_ENCODE_OFFSET)
            }
        }
        3 => (value as u64).wrapping_add(DOUBLE_ENCODE_OFFSET),
        4 => {
            if value == 0 {
                VALUE_NULL_IMM
            } else {
                value as u64
            }
        }
        5 => VALUE_UNDEFINED_IMM,
        _ => VALUE_NULL_IMM,
    }
}

/// AnyValue-immediate `Object.getPrototypeOf(any)` — reads the
/// `__proto__` slot from the wrapped dynobj and returns a NaN-box
/// `AnyValue` immediate. Identity-preserving (the returned cell
/// wraps the SAME dynobj pointer the parent prototype was stored
/// at, so `getPrototypeOf(C.prototype) === B.prototype`).
///
/// # Safety
///
/// `v` carries a valid AnyValue bit pattern; cell case must
/// point to a valid heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_get_proto_of_any(v: u64) -> u64 {
    if !is_cell_imm(v) {
        return VALUE_NULL_IMM;
    }
    let dynobj = v as *const c_void;
    // SAFETY: cell pointer to valid heap object per invariant.
    let tag = unsafe { heap_type_tag(dynobj) };
    if tag != TAG_DYNOBJ {
        // RFC 20260713-array-proto-residual blade 3 — builtin-tagged
        // cells answer their `<Ctor>.prototype` singleton per
        // §10.1.1 (an Array's [[Prototype]] IS Array.prototype, so
        // `getPrototypeOf(xs) === Array.prototype` holds). Tags with
        // no proto singleton (struct / iterator internals) keep the
        // null answer (recorded boundary).
        let proto_tag = match tag {
            TAG_ARR => 2,
            0 => 3, // Str / Substr view
            TAG_CLOSURE => 13,
            4 => 7,   // RegExp
            5 => 8,   // Date
            15 => 11, // Map
            19 => 12, // Set
            _ => -1,
        };
        if proto_tag >= 0 {
            let p = unsafe { __torajs_get_builtin_prototype(proto_tag) };
            if !p.is_null() {
                unsafe { __torajs_rc_inc(p as *mut c_void) };
                return p as u64;
            }
        }
        return VALUE_NULL_IMM;
    }
    let k = unsafe { alloc_str_key(b"__proto__") };
    if !unsafe { __torajs_dynobj_has(dynobj, k) } {
        unsafe { __torajs_str_drop(k) };
        return VALUE_NULL_IMM;
    }
    let v_tag = unsafe { __torajs_dynobj_get_tag(dynobj, k) } as i64;
    let v_val = unsafe { __torajs_dynobj_get_value(dynobj, k) } as i64;
    unsafe { __torajs_str_drop(k) };
    // rc_inc heap payload — caller owns the returned reference.
    if v_tag == ANY_HEAP && v_val != 0 {
        // SAFETY: ANY_HEAP slot holds a valid heap pointer.
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }
    box_pair_imm(v_tag, v_val)
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
    // Spec §20.1.2.8 step 1 — `Let obj be ? ToObject(O)`. ToObject on
    // `undefined` / `null` throws a TypeError; every other primitive
    // (number / boolean / string) boxes to a wrapper and falls through
    // to the no-property `undefined` return below (bun parity).
    // bun JSC msg shape: `<type> is not an object (evaluating '...')`.
    // We align the prefix; the `(evaluating ...)` suffix needs source-text
    // threading through the throw helper (substrate work, deferred).
    if obj_any == VALUE_UNDEFINED_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe { __torajs_throw_type_error(c"undefined is not an object".as_ptr()) };
        return VALUE_UNDEFINED_IMM;
    }
    if obj_any == VALUE_NULL_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe { __torajs_throw_type_error(c"null is not an object".as_ptr()) };
        return VALUE_UNDEFINED_IMM;
    }
    if !is_cell_imm(obj_any) || key.is_null() {
        return VALUE_UNDEFINED_IMM;
    }
    let dynobj = obj_any as *const c_void;
    // SAFETY: cell pointer to valid heap object.
    let htag = unsafe { heap_type_tag(dynobj) };
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
        return unsafe { crate::closure_reflect::closure_cell_descriptor(dynobj, key) };
    }
    // RFC 20260712-arr-exotic-define chunk A — Array cell answers the
    // §10.4.2 length / canonical-index / expando descriptors. SAFETY:
    // `dynobj` is a live Tag::Arr cell; `key` non-NULL (above).
    if htag == TAG_ARR {
        return unsafe { crate::arr_reflect::arr_cell_descriptor(dynobj, key) };
    }
    if htag != TAG_DYNOBJ {
        return VALUE_UNDEFINED_IMM;
    }
    let k_str = key as *const u8;
    if !unsafe { __torajs_dynobj_has(dynobj, k_str) } {
        // RFC 20260712 chunk 2 — a builtin `<Ctor>.prototype`
        // singleton owns its interned family methods without dynobj
        // entries; synthesize the spec method descriptor
        // {writable: true, enumerable: false, configurable: true}.
        // The cell is immortal, so the descriptor's value slot
        // taking heap ownership is a no-op.
        let cell = unsafe { __torajs_builtin_proto_own_method_cell(dynobj, key) };
        if cell != 0 {
            return unsafe { build_data_descriptor(ANY_HEAP as u64, cell, 1, 0, 1) };
        }
        // C2-size — the Map/Set `size` own accessor synthesizes the
        // spec accessor descriptor {get, set: undefined,
        // enumerable: false, configurable: true}. The getter cell
        // is immortal, so the get slot taking ownership is a no-op.
        let getter = unsafe { __torajs_builtin_proto_own_accessor_getter(dynobj, key) };
        if getter != 0 {
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
            return desc as u64;
        }
        return VALUE_UNDEFINED_IMM;
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
        let mut desc = unsafe { __torajs_dynobj_alloc() };
        let acc_entries: [(&[u8], u64, u64); 4] = [
            (b"get", get_t, get_v),
            (b"set", set_t, set_v),
            (b"enumerable", ANY_BOOL as u64, (flags >> 1) & 1),
            (b"configurable", ANY_BOOL as u64, (flags >> 2) & 1),
        ];
        for &(name, t, val) in acc_entries.iter() {
            let k = unsafe { alloc_str_key(name) };
            unsafe { __torajs_dynobj_set(&mut desc, k, t, val) };
            unsafe { __torajs_str_drop(k) };
        }
        return desc as u64;
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

/// RFC C5a — `Object.getOwnPropertyDescriptor(arr, "length")` real
/// descriptor. Spec ES §10.4.2.4: Array's `length` own property is
/// `{value: ToNumber(len), writable: true, enumerable: false,
/// configurable: false}`. Pre-fix tora's gOPD walked dynobj entries
/// only, so `Array.length` reported undefined.
///
/// The helper takes the pre-extracted `len` (SSA Loads it directly
/// from the array's `len` slot — offset 8 in `torajs-arr::layout`)
/// so the intrinsic signature stays `(I64) -> Any` instead of
/// pretending a typed `Arr<_>` Operand is interchangeable with
/// `Type::Ptr`. bun returns the length as a Number; tagging with
/// `AnySlotTag::I64=2` keeps small lengths as number imms and lets
/// `__torajs_dynobj_set` NaN-box overflowing values to f64 wrappers
/// transparently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_arr_length_descriptor(len: u64) -> u64 {
    // tag value mirrors AnySlotTag::I64 = 2 (numeric).
    unsafe { build_data_descriptor(2, len, 1, 0, 0) }
}

/// W-M — `Object.getOwnPropertyDescriptor(str, "length")` real
/// descriptor. Spec ES §22.1.5.1: String's `length` own property is
/// `{value: len, writable: false, enumerable: false, configurable:
/// false}` — every flag false, unlike Array's `length` which is
/// writable. Reads `u32` at `torajs-str::layout::STR_LEN_OFF = 8`
/// (a four-byte len + four-byte pad share the same eight-byte slot;
/// Load-as-u32 instead of Load-as-u64 keeps the value robust to
/// future use of the pad word).
///
/// # Safety
///
/// `str_ptr` must point at a valid Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_str_length_descriptor(str_ptr: *const c_void) -> u64 {
    // SAFETY: STR_LEN_OFF=8 holds the live u32 length per torajs-str layout.
    let len = unsafe { (str_ptr.cast::<u8>().add(8) as *const u32).read() } as u64;
    unsafe { build_data_descriptor(2, len, 0, 0, 0) }
}

/// RFC C4b — `Object.defineProperty(O, ...)` / `Object.defineProperties(O, ...)`
/// step 1: `If Type(O) is not Object, throw a TypeError`. Spec ES §10.1.6.3
/// is a **strict** Type(O) check — every primitive throws, including
/// `string` / `number` / `boolean` / `bigint` / `symbol`, regardless of
/// whether tora carries the primitive as an imm or a heap cell.
/// (gOPD's ToObject semantics box primitives to wrappers; defineProperty
/// does not.)
///
/// Branch on the `O` AnyValue at runtime:
/// * `undefined` / `null` imm → throw TypeError.
/// * Non-cell imm (number / boolean inline) → throw TypeError.
/// * Cell whose `HeapHeader::type_tag` is `Str` / `BigInt` / `Symbol`
///   (tora carries these as heap cells, spec classifies them as
///   primitives) → throw TypeError.
/// * Every other cell (DynObj / Arr / Closure / RegExp / Date /
///   Promise / Map / Set / WeakRef / WeakMap / WeakSet / MapIter /
///   ArrIter / AccessorPair) is a real object → returns without throwing.
///
/// # Safety
///
/// `obj_any` must carry a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_throw_typeerror_if_not_object(obj_any: u64) {
    if obj_any == VALUE_UNDEFINED_IMM || obj_any == VALUE_NULL_IMM || !is_cell_imm(obj_any) {
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Properties can only be defined on Objects.".as_ptr())
        };
        return;
    }
    // Cell — inspect the universal heap header `type_tag` to filter out
    // spec-primitive cells (Str / BigInt / Symbol).
    let tag = unsafe { heap_type_tag(obj_any as *const c_void) };
    if matches!(tag, TAG_STR | TAG_BIGINT | TAG_SYMBOL) {
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Properties can only be defined on Objects.".as_ptr())
        };
    }
}

/// §20.1.2.3.1 ObjectDefineProperties step 2 —
/// `ToObject(Properties)` throws a TypeError on `undefined` / `null`
/// only (other primitives wrap to objects with no enumerable own
/// keys, so the walk is a no-op) — RFC
/// 20260713-defprop-residual-cluster chunk B.
///
/// # Safety
///
/// `props_any` must carry a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_throw_typeerror_if_props_nullish(props_any: u64) {
    if props_any == VALUE_UNDEFINED_IMM || props_any == VALUE_NULL_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Cannot convert undefined or null to object.".as_ptr())
        };
    }
}

/// §6.2.6.5 ToPropertyDescriptor step 1 — `If Type(Obj) is not
/// Object, throw a TypeError` (RFC 20260713-defprop-residual-cluster
/// chunk B). Same strict Type() branch as
/// [`__torajs_anyv_throw_typeerror_if_not_object`], different spec
/// message; ssa-lower gates a runtime-`Any` descriptor through this
/// BEFORE unboxing it to a pointer (an imm AnyValue's payload is not
/// a cell — the old path handed it to `define_from_desc` verbatim).
///
/// # Safety
///
/// `desc_any` must carry a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_throw_typeerror_if_not_desc_object(desc_any: u64) {
    let is_primitive_cell = desc_any != VALUE_UNDEFINED_IMM
        && desc_any != VALUE_NULL_IMM
        && is_cell_imm(desc_any)
        && matches!(
            unsafe { heap_type_tag(desc_any as *const c_void) },
            TAG_STR | TAG_BIGINT | TAG_SYMBOL
        );
    if desc_any == VALUE_UNDEFINED_IMM
        || desc_any == VALUE_NULL_IMM
        || !is_cell_imm(desc_any)
        || is_primitive_cell
    {
        // SAFETY: NUL-terminated static C string.
        unsafe { __torajs_throw_type_error(c"Property description must be an object.".as_ptr()) };
    }
}

/// RFC 20260712-object-create-define-props chunk 1 —
/// `Object.create(proto, ...)` §20.1.2.2 step 1: `If Type(O) is
/// neither Object nor Null, throw a TypeError`. Same runtime branch
/// shape as [`__torajs_anyv_throw_typeerror_if_not_object`] except
/// `null` passes (it is the one legal non-object proto).
///
/// # Safety
///
/// `proto_any` must carry a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_object_create_check_proto(proto_any: u64) {
    if proto_any == VALUE_NULL_IMM {
        return;
    }
    let primitive = if proto_any == VALUE_UNDEFINED_IMM || !is_cell_imm(proto_any) {
        true
    } else {
        // Cell — Str / BigInt / Symbol cells are spec primitives.
        let tag = unsafe { heap_type_tag(proto_any as *const c_void) };
        matches!(tag, TAG_STR | TAG_BIGINT | TAG_SYMBOL)
    };
    if primitive {
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Object prototype may only be an Object or null.".as_ptr())
        };
    }
}
