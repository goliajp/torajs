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

use crate::reflect::PROTO_SLOT_KEY;

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
}

/// `HeapHeader::type_tag` mirror of `torajs_rc::Tag::DynObj` (locked
/// there); header field lives at byte offset 4. Shared with the
/// values/entries chooser twin (`obj_own_values.rs`).
const TAG_DYNOBJ: u16 = 14;
pub(crate) const HDR_TYPE_TAG_OFF: usize = 4;

/// `torajs_rc::Tag` mirrors for the ToObject dispatch arms
/// (chunk B1 — for-in RFC): Str / Arr / Closure / Obj cells each get
/// their own own-keys shape instead of the former non-struct throw.
pub(crate) const TAG_STR_CELL: u16 = 0;
pub(crate) const TAG_OBJ_CELL: u16 = 1;
pub(crate) const TAG_ARR_CELL: u16 = 2;
pub(crate) const TAG_CLOSURE_CELL: u16 = 3;
/// RFC 20260716 刀 5 (rotation 121 chunk 5) — primitive-wrapper cell
/// tags (`torajs_rc::Tag::{NumberWrapper,StringWrapper,BooleanWrapper}`
/// mirrors). Every wrapper carries a lazy expando dynobj at
/// `WRAPPER_PROPS_OFF` (mirror of the closure `+24` slot).
pub(crate) const TAG_NUMBER_WRAPPER: u16 = 21;
pub(crate) const TAG_STRING_WRAPPER: u16 = 22;
pub(crate) const TAG_BOOLEAN_WRAPPER: u16 = 23;
pub(crate) const WRAPPER_PROPS_OFF: usize = 16;
/// Wrapper `[[StringData]]` inner Str-cell ptr slot
/// (`torajs_rc::Tag::StringWrapper` layout: `[header:8][str_cell:8]`).
pub(crate) const WRAPPER_INNER_OFF: usize = 8;

/// torajs-arr layout mirrors — `len` u64 at +8, inline props-dynobj
/// slot at +24 (`torajs_arr::layout::ARR_PROPS_OFF`).
pub(crate) const ARR_LEN_OFF: usize = 8;
pub(crate) const ARR_PROPS_OFF: usize = 24;
/// Closure env-cell props-dynobj slot (T-27 Function-as-Object,
/// mirror `torajs_anyvalue::member_get::CLOSURE_PROPS_OFF`).
pub(crate) const CLOSURE_PROPS_OFF: usize = 24;
/// Str payload length u32 at +8 (`torajs-str` layout).
pub(crate) const STR_LEN_OFF: usize = 8;

/// ShortStr NaN-box marker (`top16 == 0x0001`) + len bits 47..40
/// (mirror `torajs_anyvalue::nanbox` SSO layout).
pub(crate) const SHORT_STR_TOP16: u64 = 0x0001;

/// `torajs_dynobj::layout::BUCKET_FLAG_ENUMERABLE` mirror (bit 1).
pub(crate) const FLAG_ENUMERABLE: u64 = 1 << 1;

/// `AnySlotTag` heap tag (mirror torajs-anyvalue) — the tag
/// `unbox_tag` reports for any heap cell, including an AccessorPair.
pub(crate) const ANY_HEAP_TAG: i64 = 4;
/// `torajs_dynobj::accessor::TAG_ACCESSOR_PAIR` mirror.
pub(crate) const TAG_ACCESSOR_PAIR: u16 = 18;
/// Elem-kind chain stamping the entries outer array (`Arr<Arr<Any>>`:
/// heap elem = 4, inner FLAG_ARR_ANY blocks self-describe) so
/// kind-aware borrow readers (Object.fromEntries) decode the slots.
pub(crate) const KIND_CHAIN_HEAP: u64 = 4;

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
unsafe fn dynobj_keys_append(
    obj: *const c_void,
    include_nonenum: i64,
    mut arr: *mut u8,
    skip_index_shadow: bool,
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
unsafe fn dynobj_keys_walk(obj: *const c_void, include_nonenum: i64) -> *mut c_void {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    let arr = unsafe { __torajs_arr_alloc(len) };
    unsafe { dynobj_keys_append(obj, include_nonenum, arr, false) as *mut c_void }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_own_keys(
    obj: *const c_void,
    static_names: *mut c_void,
    include_nonenum: i64,
) -> *mut c_void {
    if obj.is_null() || !unsafe { is_dynobj(obj) } {
        return static_names;
    }
    unsafe { __torajs_value_drop_heap(static_names) };
    unsafe { dynobj_keys_walk(obj, include_nonenum) }
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
    let out = if include_nonenum == 0 {
        unsafe { crate::own_names::__torajs_arr_keys_only_of(cell) }
    } else {
        unsafe { crate::own_names::__torajs_arr_index_strs_of(cell) }
    };
    let props =
        unsafe { (cell.cast::<u8>().add(ARR_PROPS_OFF) as *const u64).read() } as *const c_void;
    if props.is_null() {
        return out;
    }
    unsafe { dynobj_keys_append(props, include_nonenum, out as *mut u8, true) as *mut c_void }
}

/// `true` iff the live Str key spells exactly [`PROTO_SLOT_KEY`]
/// (the [[Prototype]]-slot simulation key hidden from own-keys
/// walks).
unsafe fn key_is_proto_slot(key: *const c_void) -> bool {
    let len = unsafe { key.cast::<u8>().add(8).cast::<u32>().read() } as usize;
    len == PROTO_SLOT_KEY.len()
        && unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(16), len) } == PROTO_SLOT_KEY
}

/// Canonical array-index test on a live Str key — the shadow-entry
/// filter's key shape check ([`crate::arr_reflect`] twin).
unsafe fn key_is_canonical_index(key: *const c_void) -> bool {
    let len = unsafe { key.cast::<u8>().add(8).cast::<u32>().read() } as usize;
    if len == 0 || len > 10 {
        return false;
    }
    let bytes = unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(16), len) };
    if bytes == b"0" {
        return true;
    }
    if bytes[0] == b'0' || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut v: u64 = 0;
    for &b in bytes {
        v = v * 10 + (b - b'0') as u64;
    }
    v < u32::MAX as u64
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
                let out = unsafe { __torajs_arr_alloc(0) };
                if props.is_null() {
                    out as *mut c_void
                } else {
                    unsafe { dynobj_keys_append(props, include_nonenum, out, false) as *mut c_void }
                }
            }
            TAG_OBJ_CELL => unsafe { crate::struct_enum::__torajs_anyv_struct_keys(v) },
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
                        dynobj_keys_append(props, include_nonenum, out as *mut u8, true)
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
                    unsafe { dynobj_keys_append(props, include_nonenum, out, false) as *mut c_void }
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

/// for-in keys source (chunk B2, RFC 20260711): the `Object.keys`
/// enumerable-own surface, except a null / undefined receiver
/// enumerates nothing — ES §14.7.5 ForIn/OfHeadEvaluation step 3
/// short-circuits before ToObject can throw.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern; the caller owns the
/// returned `+1`-rc `Arr<Str>`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_forin_keys(v: u64) -> *mut c_void {
    if v == crate::reflect::VALUE_NULL_IMM || v == crate::reflect::VALUE_UNDEFINED_IMM {
        return unsafe { __torajs_arr_alloc(0) as *mut c_void };
    }
    unsafe { __torajs_anyv_own_keys(v, 0) }
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
