//! Reflection substrate — port of `runtime_str.c` L494-549 + L881-915.
//! Step 7 NaN-box cutover: both helpers here + the descriptor cascade
//! in [`crate::reflect_get_property_descriptor`] operate on NaN-box
//! `AnyValue` immediates and route through `torajs-dynobj` for the
//! slot reads. Returned values are always owned AnyValue immediates
//! (caller takes ownership).
//!
//! This module now carries the shared constants (tag / attribute /
//! wrapper-layout mirrors) + descriptor builders
//! ([`build_data_descriptor`] / [`build_accessor_descriptor`]) +
//! extern surface. The 248-LOC `Object.getOwnPropertyDescriptor(obj,
//! key)` dispatch cascade lives in
//! [`crate::reflect_get_property_descriptor`] as of the
//! rotation-196 file-size sweep (the parent had drifted to 519 LOC
//! per `rules/torajs-file-size-debt.md`).

use core::ffi::{c_char, c_void};

unsafe extern "C" {
    pub(crate) fn __torajs_throw_type_error(msg: *const c_char);
    pub(crate) fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    pub(crate) fn __torajs_str_drop(s: *mut u8);
    pub(crate) fn __torajs_dynobj_alloc() -> *mut c_void;
    pub(crate) fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    pub(crate) fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
    pub(crate) fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    pub(crate) fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
    pub(crate) fn __torajs_dynobj_get_flags(dynobj: *const c_void, key: *const u8) -> u64;
    pub(crate) fn __torajs_accessor_get_getter(pair: *const c_void) -> *mut c_void;
    pub(crate) fn __torajs_accessor_get_setter(pair: *const c_void) -> *mut c_void;
    // RFC 20260712 chunk 2 — builtin `<Ctor>.prototype` own-method
    // probe from torajs-anyvalue/method_support.rs. Non-zero = the
    // immortal interned method cell (rc traffic no-ops on it).
    pub(crate) fn __torajs_builtin_proto_own_method_cell(
        dynobj: *const c_void,
        key: *const c_void,
    ) -> u64;
    // set-proto-cluster C2-size — the Map/Set `size` own-accessor
    // probe (same crate). Non-zero = the immortal getter cell.
    pub(crate) fn __torajs_builtin_proto_own_accessor_getter(
        dynobj: *const c_void,
        key: *const c_void,
    ) -> u64;
    // RFC 20260722 刀 3 — Function.prototype's virtual own
    // name/length member pair (same crate). 1 = hit, pair written
    // through the out params (immortal name Str).
    pub(crate) fn __torajs_builtin_proto_own_meta(
        dynobj: *const c_void,
        key: *const c_void,
        out_tag: *mut u64,
        out_val: *mut u64,
    ) -> i64;
    // torajs-regex — a RegExp instance's `lastIndex` slot (RFC
    // 20260722 刀 4).
    pub(crate) fn __torajs_regex_get_last_index(re: *const c_void) -> f64;
    /// torajs-regex — boxed-form lastIndex peek (BORROW; 0 = numeric
    /// form).
    pub(crate) fn __torajs_regex_last_index_raw(re: *const c_void) -> u64;
    /// torajs-regex — 1 while `lastIndex` is still writable
    /// (`defineProperty` can freeze it once, §10.1.6.3 step 4).
    pub(crate) fn __torajs_regex_last_index_writable(re: *const c_void) -> i64;
    /// torajs-anyvalue — NaN-box → (tag, value) pair decode for the
    /// descriptor builder.
    pub(crate) fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    pub(crate) fn __torajs_anyv_unbox_value(v: u64) -> i64;
    // torajs-rc — lazy `<Ctor>.prototype` singleton by builtin tag
    // (Array=2 / String=3 / RegExp=7 / Date=8 / Map=11 / Set=12 /
    // Function=13; `builtin_proto.rs` order).
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    // torajs-rc — the reverse: which builtin's `.prototype` a pointer
    // IS, or -1. Compared, never dereferenced.
    fn __torajs_builtin_proto_tag_of(p: *const c_void) -> i64;
}

/// `Object.prototype`'s builtin tag (`builtin_proto.rs` order) — the
/// root every other builtin prototype inherits from.
pub(crate) const OBJECT_PROTO_TAG: i64 = 1;

/// Mirror of `torajs_dynobj::layout::DYNOBJ_HDR_FLAG_NULL_PROTO`
/// (header bit 6) — set on the dict `Object.create(null)` mints.
pub(crate) const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

/// The internal [[Prototype]] simulation-slot key. The leading NUL
/// keeps it out of the user-spellable identifier space, so a user's
/// own `__proto__` DATA entry (shorthand `{__proto__}`, computed key,
/// defineProperty) never collides with the internal proto link —
/// hasOwnProperty / own-keys / descriptor reads see only real own
/// properties. (A computed key containing a literal NUL could still
/// spell it — recorded boundary.) Cross-crate twin:
/// `torajs_anyvalue::member_get_own::PROTO_SLOT_KEY`.
pub(crate) const PROTO_SLOT_KEY: &[u8] = b"\x00proto";

/// DefineOwnProperty flags byte for the internal proto-slot entry:
/// value present + all three attrs present, {W:1, E:0, C:1} —
/// enumerable stays clear so every enumeration walk (values /
/// entries / print / spread) skips the simulation entry without a
/// per-walk key filter. (`torajs_dynobj::layout` DEFINE_* bit
/// positions.)
pub(crate) const PROTO_SLOT_ATTRS: u64 =
    (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 0) | (1 << 2);

/// Builtin-proto tags of the primitive wrappers (`builtin_proto.rs`
/// order).
pub(crate) const NUMBER_PROTO_TAG: i64 = 0;
pub(crate) const STRING_PROTO_TAG: i64 = 3;
pub(crate) const BOOLEAN_PROTO_TAG: i64 = 4;

/// ShortStr's NaN-box signature — the top 16 bits are `0x0001`
/// (mirrors `is_cell_imm`'s note above).
pub(crate) const SHORT_STR_TOP16: u64 = 0x0001_0000_0000_0000;

// Tag values mirrored from torajs-anyvalue::AnySlotTag — re-declared
// here to keep this crate's dep tree narrow (no torajs-anyvalue
// Cargo dep; the i64 wire tag is part of the ABI anyway).
pub(crate) const ANY_BOOL: i64 = 1;
pub(crate) const ANY_HEAP: i64 = 4;
pub(crate) const ANY_UNDEF: i64 = 5;
/// `get_tag` accessor sentinel (mirrors `torajs_dynobj::layout::ANY_ACCESSOR`).
pub(crate) const ANY_ACCESSOR: u64 = 6;

// Tag::DynObj from torajs-rc — universal heap header at offset 0.
pub(crate) const TAG_DYNOBJ: u16 = 14;
// Tag::Obj — static-layout struct cell (W-J Phase B struct arm /
// Phase C struct enumeration arms in `struct_enum.rs`).
pub(crate) const TAG_OBJ: u16 = 1;
// Tag::Arr — array cell; gOPD routes to the length / canonical-index /
// expando arms in `arr_reflect.rs` (RFC 20260712-arr-exotic-define
// chunk A).
pub(crate) const TAG_ARR: u16 = 2;
// Tag::Closure — fn cell; gOPD routes to the virtual name/length
// descriptor arm in `closure_reflect.rs` (RFC 20260711 chunk B).
pub(crate) const TAG_CLOSURE: u16 = 3;
/// `Tag::RegExp` (torajs-rc `tag.rs`).
pub(crate) const TAG_REGEXP: u16 = 4;
// Primitive-wrapper cells (RFC 20260716 刀 2b / 刀 5). All three
// share the `[header:8][value:8][props:8]` layout — chunks 4+5
// (rotation 121) landed the +16 lazy expando dynobj on write and
// read sides. Number/Boolean have no inherent own props;
// StringWrapper has §22.1.4.1 `length` + §22.1.4.4 char-index.
pub(crate) const TAG_NUMBER_WRAPPER: u16 = 21;
pub(crate) const TAG_STRING_WRAPPER: u16 = 22;
pub(crate) const TAG_BOOLEAN_WRAPPER: u16 = 23;
/// Mirror of `torajs-wrapper::WRAPPER_PROPS_OFF` — all three wrappers
/// share `[header:8][value:8][props:8]` layout.
pub(crate) const WRAPPER_PROPS_OFF: usize = 16;

#[inline]
pub(crate) fn is_wrapper_tag(t: u16) -> bool {
    t == TAG_NUMBER_WRAPPER || t == TAG_STRING_WRAPPER || t == TAG_BOOLEAN_WRAPPER
}
/// Tag::Str / Tag::Symbol / Tag::BigInt from `torajs-rc` — primitive-in-spec
/// heap cells. RFC C4b throws TypeError on these because `Object.defineProperty(O, ...)`
/// step 1 is a strict `Type(O) is Object` check (no ToObject wrapper boxing).
pub(crate) const TAG_STR: u16 = 0;
pub(crate) const TAG_SYMBOL: u16 = 7;
pub(crate) const TAG_BIGINT: u16 = 10;
/// `torajs_rc::Tag::Proxy` (RFC 20260823-proxy-substrate) — the
/// reflection surface routes every arm to the proxy kernels.
pub(crate) const TAG_PROXY: u16 = 26;

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
pub(crate) const VALUE_FALSE_IMM: u64 = 0x06;
pub(crate) const VALUE_TRUE_IMM: u64 = 0x07;
const TAG_TYPE_NUMBER: u64 = 0xFFFE_0000_0000_0000;
const TAG_BIT_TYPE_OTHER: u64 = 0x02;
const DOUBLE_ENCODE_OFFSET: u64 = 0x0007_0000_0000_0000;
/// Step 8b-B — top-16-bit mask for strict cell detection. ShortStr
/// claims `top16 == 0x0001`; weak `& TAG_TYPE_NUMBER == 0` would
/// misclassify ShortStr as cell when its low byte happened to
/// clear bit 1 (e.g. `'a'` = 0x61).
pub(crate) const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;

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

/// Build an accessor descriptor dynobj `{ get, set, enumerable,
/// configurable }` — no `value` / `writable` per §6.2.6. Each closure
/// arrives already owned by the descriptor (callers inc beforehand);
/// a missing half is `undefined`. Shared by the dynobj AccessorPair
/// arm and the struct-layout accessor slots (RFC
/// 20260714-objlit-accessor).
pub(crate) unsafe fn build_accessor_descriptor(
    get_t: u64,
    get_v: u64,
    set_t: u64,
    set_v: u64,
    enumerable: u64,
    configurable: u64,
) -> u64 {
    let mut desc = unsafe { __torajs_dynobj_alloc() };
    let entries: [(&[u8], u64, u64); 4] = [
        (b"get", get_t, get_v),
        (b"set", set_t, set_v),
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
pub(crate) fn box_pair_imm(tag: i64, value: i64) -> u64 {
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
