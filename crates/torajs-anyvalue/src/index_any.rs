//! `__torajs_any_index_get` — `recv[idx]` where the receiver is an
//! `any` value (Any-dynamic-access RFC 20260704 S3).
//!
//! Dispatch tree (ES ordinary property access semantics):
//! - `null` / `undefined` receiver → catchable TypeError (ES §13.3.2
//!   RequireObjectCoercible), returns `undefined` after recording the
//!   pending throw (caller's `emit_throw_check` propagates).
//! - numeric / bool immediates → `undefined` (primitives have no
//!   own indexed properties).
//! - ShortStr — ASCII payloads answer inline (byte == code unit);
//!   non-ASCII payloads materialize to a heap Str and reuse the heap
//!   path so UTF-16 code-unit semantics match the typed tier.
//! - `Tag::Str` cell (Str or Substr view) →
//!   `__torajs_str_index_get` (torajs-str); NULL = OOB → `undefined`.
//! - `Tag::Arr` cell → `__torajs_arr_index_get` (torajs-arr,
//!   kind-aware: FLAG_ARR_ANY NaN-box slots or `ARR_KIND_*` raw
//!   slots recorded at the boxing boundary).
//! - `Tag::DynObj` (L3b #3, chunk 527) → the numeric key stringifies
//!   to its decimal form (`o[42]` ≡ `o["42"]` per ES §7.1.19
//!   ToPropertyKey) and probes own properties; an accessor entry's
//!   getter runs (getter-as-value). Absence answers `undefined`.
//! - any other heap tag → `undefined` (no such own property).

use core::ffi::c_void;

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, box_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined, short_str_bytes, short_str_len, try_box_short_str,
};
use crate::nanbox_ffi_materialize::materialize_short_str;
use torajs_rc::Tag;

/// Primitive-wrapper lazy expando dynobj slot (mirror of
/// `member_set`'s `MEMBER_SET_WRAPPER_PROPS_OFF`; wrapper layout is
/// `[header:8][value:8][props:8]`, RFC 20260716 刀 4/5).
const WRAPPER_PROPS_OFF: usize = 16;

unsafe extern "C" {
    /// torajs-buffer — §10.4.5.4 element read; out of range answers
    /// `undefined` and does not continue up the chain.
    fn __torajs_typedarray_index_get(recv: AnyValue, index: f64) -> AnyValue;
    /// torajs-buffer §23.2.4.4 — the view's length now, or -1 with a
    /// pending throw.
    fn __torajs_typedarray_validate(recv: AnyValue) -> i64;
    /// torajs-str — `s[idx]` (Str or Substr); NULL = OOB.
    fn __torajs_str_index_get(s: *mut u8, idx: i64) -> *mut u8;
    /// torajs-arr — kind-aware `arr[idx]`; returns a balanced
    /// AnyValue (+1 for cells).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// Universal NaN-box-safe heap dropper (torajs-value-drop).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-str — release a heap Str/Substr reference. Signature
    /// mirrors the crate-local test stub (`*mut c_void`).
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError; returns
    /// normally (caller's throw-check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-str — heap Str constructor (rc=1), used to build the
    /// literal "length" probe key for DynObj receivers.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-dynobj — own-key membership probe (disambiguates the
    /// (5, 0) absent answer from an own entry storing undefined).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent →
    /// undefined by construction).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — run an accessor entry's getter; the answer is
    /// an owned AnyValue per the boxed-value convention.
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
}

/// See module doc.
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_get(recv: AnyValue, idx: i64) -> AnyValue {
    // §6.1.7 — a property key is a String, so a Proxy sees `p[0]`
    // spelled `"0"` (RFC 20260823-proxy-substrate 刀 1).
    if crate::proxy::is_proxy(recv) {
        return unsafe { crate::proxy::get_index(recv, idx) };
    }
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if is_short_str(recv) {
        let len = short_str_len(recv) as usize;
        let bytes = short_str_bytes(recv);
        let payload = &bytes[..len];
        if payload.iter().all(|b| *b < 0x80) {
            // ASCII: byte index == UTF-16 code-unit index.
            if idx < 0 || idx as usize >= len {
                return VALUE_UNDEFINED;
            }
            return try_box_short_str(&payload[idx as usize..idx as usize + 1])
                .unwrap_or(VALUE_UNDEFINED);
        }
        // Non-ASCII payload: materialize to a heap Str and reuse the
        // heap path so code-unit semantics match the typed tier. The
        // returned Substr holds its own parent reference, so the
        // temporary parent drops safely right after.
        unsafe {
            let parent = materialize_short_str(recv);
            let out = index_str_cell(parent, idx);
            __torajs_str_drop(parent as *mut c_void);
            return out;
        }
    }
    if is_int32(recv) || is_double(recv) || is_bool(recv) {
        return VALUE_UNDEFINED;
    }
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    // RFC 20260716 刀 3 — primitive-wrapper view-through
    // (see `wrapper_view_through`). Number/Boolean fall to primitive
    // immediates (undefined for indexed access); StringWrapper hands
    // its inner Str cell to the Str arm below. An undefined answer
    // falls through to the `+16` lazy expando dynobj — §9.1.8
    // OrdinaryGet reaches own numeric keys `defineProperty` planted
    // past the inherent index face.
    if let Some(inner) = unsafe { crate::wrapper_view_through::resolve_inner_recv(ptr, tag) } {
        let out = unsafe { __torajs_any_index_get(inner, idx) };
        if out != VALUE_UNDEFINED {
            return out;
        }
        let props =
            unsafe { (ptr.cast::<u8>().add(WRAPPER_PROPS_OFF) as *const u64).read() } as *mut u8;
        if !props.is_null()
            && let Some(own) = unsafe { dynobj_index_own(props as *mut c_void, idx) }
        {
            return own;
        }
        // Own miss — the chain half: the wrapper-PROTOTYPE
        // singleton's expando face (`Boolean.prototype[1] = v`,
        // §10.1.8.1 step 3), which walks on to the
        // %Object.prototype% root itself. Ordering matters: the old
        // shape sent the expando dict straight into the chain-walking
        // read, reaching the root while skipping the wrapper
        // prototype between.
        if let Some(ptag) = crate::member_get_layout::wrapper_proto_tag(tag) {
            let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(ptag) };
            if !proto.is_null() {
                return unsafe { dynobj_index_get(proto as *mut c_void, idx) };
            }
        }
        return VALUE_UNDEFINED;
    }
    if tag == Tag::Str as u16 {
        return unsafe { index_str_cell(ptr as *mut u8, idx) };
    }
    // §10.4.5.4 — an integer-indexed exotic object. An index that
    // is out of range answers `undefined` WITHOUT walking the
    // prototype chain, which is the whole difference between this
    // and an object with numeric keys.
    if tag == Tag::TypedArray as u16 {
        return unsafe { __torajs_typedarray_index_get(recv, idx as f64) };
    }
    // §25.1 / §25.3 — ArrayBuffer and DataView are ordinary off
    // their index face: the numeric key reads the expando bag (both
    // cells keep it at +32) and misses answer undefined. A DataView
    // reaches its bytes only through the §25.3.4 methods.
    if tag == Tag::ArrayBuffer as u16 || tag == Tag::DataView as u16 {
        let props = unsafe { (ptr.cast::<u8>().add(32) as *const u64).read() } as *mut c_void;
        if !props.is_null() {
            return unsafe { dynobj_index_get(props, idx) };
        }
        return VALUE_UNDEFINED;
    }
    if tag == Tag::Arr as u16 {
        return unsafe { __torajs_arr_index_get(ptr, idx) };
    }
    if tag == Tag::DynObj as u16 {
        return unsafe { dynobj_index_get(ptr, idx) };
    }
    // 406-02 — a FUNCTION value's numeric-keyed read: a computed
    // static class field with a numeric key installs into the
    // closure's expando bag (`C[10]` is how it reads back), and a
    // re-parented function value inherits through its user
    // [[Prototype]] chain. A miss stays the ordinary undefined —
    // the builtin Function surface owns no numeric keys.
    if tag == Tag::Closure as u16 {
        let props = unsafe { crate::member_get_layout::closure_props(ptr) };
        if !props.is_null() {
            let out = unsafe { dynobj_index_get(props as *mut c_void, idx) };
            if out != VALUE_UNDEFINED {
                return out;
            }
        }
        match unsafe { crate::member_get_own::closure_user_proto(ptr) } {
            Some(Some(parent)) => return unsafe { __torajs_any_index_get(parent, idx) },
            _ => return VALUE_UNDEFINED,
        }
    }
    // Chunk 744 — struct cell: ToPropertyKey the index and probe the
    // class layout (`{0:"a"} as any` boxes as an anon-struct whose
    // field NAME is the decimal spelling; pre-fix `e[0]` answered a
    // silent undefined).
    if tag == Tag::Obj as u16 {
        return unsafe { struct_index_get(ptr, idx) };
    }
    VALUE_UNDEFINED
}

/// `Tag::Obj` get arm — decimal-stringify the key and probe the class
/// layout through [`crate::member_get::struct_field_pair`]; the probe
/// is a borrow, the returned box owns its own reference (the
/// `dynobj_index_get` shape). A field miss probes the expando dict,
/// then the own named-accessor lane (`get 0() {…}` parses into a
/// `__getter_0` layout slot — the same spelling the named-getter
/// member lane resolves; ES §7.1.19 ToPropertyKey makes `o[0]` ≡
/// `o["0"]`), then the class prototype chain (rotation 441 — where
/// runtime-computed members reify), and a full miss answers
/// `undefined`.
unsafe fn struct_index_get(obj: *mut c_void, idx: i64) -> AnyValue {
    let mut buf = [0u8; 20];
    let (start, len) = i64_dec(&mut buf, idx);
    unsafe {
        let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
        let pair = crate::struct_probe::struct_field_pair(obj, key as *const c_void);
        let Some((ftag, fval)) = pair else {
            // Blade 2 (RFC 20260714-struct-dynamic-props) — a numeric
            // expando (`(c as any)[4] = v`) lives in the +24 dict
            // under its §7.1.19 decimal spelling; own face, so it
            // answers before the accessor fallback's undefined.
            let props = crate::member_get_layout::struct_props(obj);
            if !props.is_null() && __torajs_dynobj_has(props, key as *const c_void) != 0 {
                let etag = __torajs_dynobj_get_tag(props, key as *const c_void);
                let eval = __torajs_dynobj_get_value(props, key as *const c_void);
                __torajs_str_drop(key as *mut c_void);
                crate::payload_rc_inc(etag as i64, eval as i64);
                return crate::nanbox_encode::__torajs_anyv_box_from_pair(etag as i64, eval as i64);
            }
            // Own named accessor (`get 0() {…}` parses into a
            // `__getter_0` layout slot) — presence-tested so a
            // genuine miss can keep walking instead of swallowing
            // the chain behind an unconditional undefined.
            if crate::struct_probe::struct_accessor_key(obj, key as *const c_void) {
                __torajs_str_drop(key as *mut c_void);
                return crate::struct_probe::__torajs_struct_accessor_get(
                    obj,
                    buf[start..].as_ptr(),
                    len as u32,
                );
            }
            // Rotation 441 — §10.1.8.1 step 3: the class prototype
            // chain under the decimal spelling. A runtime-computed
            // member (`get [1 + 1]() {…}`) reifies onto `__proto_<C>`
            // keyed "2"; the string/symbol lanes have walked that
            // chain since L3b ⑧ / RFC 20260802 刀 3a, so the
            // integer-key lane was the one read shape still blind to
            // it (`c[2]` answered undefined while `c["2"]` invoked
            // the getter). Accessor entries invoke with the STRUCT
            // as receiver; data pairs are borrows the box promotes.
            let (ptag, pval) = crate::struct_error_msg::struct_proto_chain_pair(
                obj as *const c_void,
                key as *const c_void,
            );
            __torajs_str_drop(key as *mut c_void);
            if ptag == INDEX_ANY_ACCESSOR_TAG {
                return __torajs_accessor_invoke_getter(
                    pval as *const c_void,
                    crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64),
                );
            }
            crate::payload_rc_inc(ptag as i64, pval as i64);
            return crate::nanbox_encode::__torajs_anyv_box_from_pair(ptag as i64, pval as i64);
        };
        __torajs_str_drop(key as *mut c_void);
        crate::payload_rc_inc(ftag as i64, fval as i64);
        crate::nanbox_encode::__torajs_anyv_box_from_pair(ftag as i64, fval as i64)
    }
}

/// Stack decimal formatter for the ToPropertyKey stringification
/// (in-house — metal-level crates take no external deps). Fills the
/// 20-byte buffer backward; answers `(start, len)` into it. 20 bytes
/// covers i64::MIN (`-9223372036854775808`).
pub(crate) fn i64_dec(buf: &mut [u8; 20], v: i64) -> (usize, usize) {
    let neg = v < 0;
    // Two's-complement magnitude — safe for i64::MIN.
    let mut m = (v as i128).unsigned_abs() as u64;
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (m % 10) as u8;
        m /= 10;
        if m == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    (i, buf.len() - i)
}

/// `Tag::DynObj` get arm — decimal-stringify the key and probe own
/// properties (same borrow-then-own shape as the `"length"` probe
/// below); an accessor entry's getter answers its owned result. An
/// own miss continues per §10.1.8.1 OrdinaryGet step 3 (RFC 20260721
/// G2d): the user [[Prototype]] chain first (dynobj parents, the
/// member_get knife-2 shape), then the %Object.prototype% singleton
/// digit-key face — which is how test262 installs inherited index
/// props for the array-like generics.
unsafe fn dynobj_index_get(obj: *mut c_void, idx: i64) -> AnyValue {
    let mut buf = [0u8; 20];
    let (start, len) = i64_dec(&mut buf, idx);
    unsafe {
        let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
        let mut host = obj as *const c_void;
        let r = loop {
            if let Some(v) = dynobj_index_entry(host, key as *const c_void, obj) {
                break v;
            }
            // Walk to the parent; a null-proto host ends the chain,
            // an implicit-chain root falls to %Object.prototype%.
            // Non-dynobj parents (Object.create(arr)) stay on the
            // absent answer — their inherent faces are the recorded
            // G9d follow-up.
            match crate::member_get_own::user_proto_cell(host) {
                Some(parent)
                    if (parent as *const u8).add(4).cast::<u16>().read() == Tag::DynObj as u16 =>
                {
                    host = parent as *const c_void;
                }
                Some(_) => break VALUE_UNDEFINED,
                None => {
                    let flags = host.cast::<u8>().add(6).cast::<u16>().read();
                    if flags & crate::member_get_own::DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
                        break VALUE_UNDEFINED;
                    }
                    let proto = torajs_rc::builtin_proto::__torajs_get_builtin_prototype(
                        torajs_rc::builtin_proto::OBJECT_PROTO_TAG as i64,
                    );
                    if proto.is_null() || core::ptr::eq(proto.cast(), host) {
                        break VALUE_UNDEFINED;
                    }
                    break dynobj_index_entry(proto as *const c_void, key as *const c_void, obj)
                        .unwrap_or(VALUE_UNDEFINED);
                }
            }
        };
        __torajs_str_drop(key as *mut c_void);
        r
    }
}

/// Own-only digit-key probe over one dynobj host (key minted here) —
/// the wrapper expando read, which must NOT walk a chain of its own:
/// the dict is storage, not a spec object, and the receiver's real
/// chain (wrapper prototype → %Object.prototype%) is the caller's
/// next step.
unsafe fn dynobj_index_own(obj: *mut c_void, idx: i64) -> Option<AnyValue> {
    let mut buf = [0u8; 20];
    let (start, len) = i64_dec(&mut buf, idx);
    unsafe {
        let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
        let r = dynobj_index_entry(obj as *const c_void, key as *const c_void, obj);
        __torajs_str_drop(key as *mut c_void);
        r
    }
}

/// One-host own probe for the digit key: `None` = truly absent (the
/// has probe disambiguates an own entry STORING undefined, which
/// shadows the chain per the own-undefined-shadow family). Accessor
/// entries invoke with the ORIGINAL receiver per §10.1.8.1 [[Get]].
pub(crate) unsafe fn dynobj_index_entry(
    host: *const c_void,
    key: *const c_void,
    recv: *mut c_void,
) -> Option<AnyValue> {
    unsafe {
        let dtag = __torajs_dynobj_get_tag(host, key);
        if dtag == INDEX_ANY_ACCESSOR_TAG {
            let dval = __torajs_dynobj_get_value(host, key);
            return Some(__torajs_accessor_invoke_getter(
                dval as *const c_void,
                crate::nanbox_encode::__torajs_anyv_box_from_pair(4, recv as i64),
            ));
        }
        if dtag == 5 && __torajs_dynobj_has(host, key) == 0 {
            return None;
        }
        let dval = __torajs_dynobj_get_value(host, key);
        // The probe pair is a borrow — the returned box owns its own
        // reference.
        crate::payload_rc_inc(dtag as i64, dval as i64);
        Some(crate::nanbox_encode::__torajs_anyv_box_from_pair(
            dtag as i64,
            dval as i64,
        ))
    }
}

/// Shared `Tag::Str` arm — `str_index_get` returns a fresh rc=1
/// Substr (ownership transfers into the box) or NULL for OOB.
unsafe fn index_str_cell(s: *mut u8, idx: i64) -> AnyValue {
    unsafe {
        let sub = __torajs_str_index_get(s, idx);
        if sub.is_null() {
            VALUE_UNDEFINED
        } else {
            box_void_ptr(sub as *mut c_void)
        }
    }
}

// Layout mirrors for the length fast paths (kept inline per the
// one-constant-per-crate convention, same as torajs-arr's ANY_HEAP):
// torajs-arr `ARR_LEN_OFF` / torajs-str `STR_LEN_OFF` (u32 code
// units) / torajs-str `SUBSTR_LEN_OFF` (u64 code units) /
// `FLAG_SUBSTR_INLINE` (HeapHeader flags bit 0).
pub(crate) const MIRROR_ARR_LEN_OFF: usize = 8;
// Accessor-entry sentinel in the dynobj probe's tag channel — mirror
// of torajs-core `ssa_lower_accessor.rs::ANY_ACCESSOR_TAG`.
const INDEX_ANY_ACCESSOR_TAG: u64 = 6;
pub(crate) const MIRROR_STR_LEN_OFF: usize = 8;
pub(crate) const MIRROR_SUBSTR_LEN_OFF: usize = 8;
pub(crate) const MIRROR_FLAG_SUBSTR_INLINE: u16 = 1 << 0;

/// Count UTF-16 code units of a UTF-8 byte payload (≤ 5 bytes for a
/// ShortStr, so a simple lead-byte walk suffices; 4-byte sequences
/// decode to astral cps = 2 units).
pub(crate) fn utf16_units_of_utf8(payload: &[u8]) -> i64 {
    let mut units = 0i64;
    let mut i = 0usize;
    while i < payload.len() {
        let b = payload[i];
        let (adv, u) = if b < 0x80 {
            (1, 1)
        } else if b < 0xE0 {
            (2, 1)
        } else if b < 0xF0 {
            (3, 1)
        } else {
            (4, 2)
        };
        i += adv;
        units += u;
    }
    units
}
