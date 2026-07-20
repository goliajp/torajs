//! Descriptor + argument-guard faces of the reflection substrate,
//! split out of `reflect.rs` (file-size limit; the sibling grew past
//! 500 prod lines when `Array.prototype` became an Arr cell). Two
//! groups, both leaves of the same substrate:
//!
//! - the `length` own-descriptor of an Array / String receiver, which
//!   ssa_lower calls with the length already loaded;
//! - the `Type(O)`-shaped argument guards `Object.defineProperty` /
//!   `defineProperties` / `ToPropertyDescriptor` / `Object.create`
//!   run before touching their receiver.
//!
//! Symbols and ABI are unchanged — the extern names are what
//! ssa_lower declares.

use core::ffi::c_void;

use crate::reflect::{
    SHORT_STR_TOP16, TAG_BIGINT, TAG_STR, TAG_SYMBOL, TOP_16_MASK, VALUE_NULL_IMM,
    VALUE_UNDEFINED_IMM, build_data_descriptor, heap_type_tag, is_cell_imm,
};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
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

/// §20.1.2.2 `Object.create(O, Properties)` step 3 — the outer
/// gate: `If Properties is not undefined, then Return ?
/// ObjectDefineProperties(obj, Properties)`. Only `null` reaches
/// step 3.a's `ToObject` and throws; `undefined` skips the whole
/// clause and returns `obj` untouched. Sibling of
/// [`__torajs_anyv_throw_typeerror_if_props_nullish`] (which throws
/// on both — used by `Object.defineProperties`); this null-only
/// variant is the `Object.create` gate that lets `undefined`
/// through the runtime walk (a NULL-pointer `props` there is a
/// silent no-op).
///
/// # Safety
///
/// `props_any` must carry a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_throw_typeerror_if_props_null_only(props_any: u64) {
    if props_any == VALUE_NULL_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Cannot convert undefined or null to object.".as_ptr())
        };
    }
}

/// §20.1.2.2 step 3 / §20.1.2.3.1 step 1 — resolve a non-nullish
/// ObjectDefineProperties `Properties` value to the walkable cell
/// pointer, or NULL for a spec no-op. `ToObject(primitive)` wraps
/// into a key-less object, so a primitive Properties value no-ops —
/// EXCEPT a non-empty string, whose wrapper owns enumerable index
/// properties valued by 1-char strings, so the walk's
/// ToPropertyDescriptor throws on the first one (§6.2.6.5, same
/// message the descriptor validator uses). Symbol / BigInt cells are
/// primitives too — the dynobj kernel's catch-all mistook them for
/// invalid descriptors. Pre-fix the SSA lane unboxed raw immediate
/// bits into the kernel's pointer param, so `Object.create(null, 1)`
/// dereferenced address 1 (SIGSEGV) — rotation 162 appeared case
/// staging/sm object-create-with-primitive-second-arg.
///
/// # Safety
/// `props_any` carries a valid AnyValue bit pattern; a cell payload
/// points at a live heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_define_props_source_gate(props_any: u64) -> *mut c_void {
    if is_cell_imm(props_any) {
        let p = props_any as *mut c_void;
        let tag = unsafe { heap_type_tag(p) };
        if tag == TAG_SYMBOL || tag == TAG_BIGINT {
            return core::ptr::null_mut();
        }
        if tag == TAG_STR {
            // torajs-str layout: u32 length at STR_LEN_OFF = 8.
            let len = unsafe { (p.cast::<u8>().add(8) as *const u32).read() };
            if len > 0 {
                // SAFETY: NUL-terminated static C string.
                unsafe {
                    __torajs_throw_type_error(c"Property description must be an object.".as_ptr());
                }
            }
            return core::ptr::null_mut();
        }
        return p;
    }
    // ShortStr immediate — the non-empty face throws like the
    // heap-Str arm; bits 47..40 carry the byte length.
    if props_any & TOP_16_MASK == SHORT_STR_TOP16 && (props_any >> 40) & 0xFF != 0 {
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Property description must be an object.".as_ptr());
        }
    }
    core::ptr::null_mut()
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
