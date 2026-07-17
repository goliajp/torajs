//! Own-domain key probes behind the dynamic-key member reads —
//! split from `member_get.rs` (file-size cap): the per-receiver
//! "is this key an INHERENT own property?" helpers that run before
//! the expando probe / builtin reify. All borrow-shaped, mirroring
//! the pair contract in the parent module doc.

use core::ffi::c_void;

use torajs_rc::AnySlotTag;

use crate::member_get::{STR_DATA_OFF, STR_LEN_OFF, header_flag};

unsafe extern "C" {
    /// torajs-arr — kind-aware slot reads, borrow contract (RFC
    /// 20260712-arr-exotic-define chunk A dynamic-key arm).
    fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64;
    /// torajs-str / torajs-dynobj — the `__proto__` simulation-slot
    /// probe behind [`user_proto_cell`].
    fn __torajs_str_alloc(p: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
}

/// `DYNOBJ_HDR_FLAG_NULL_PROTO` mirror (torajs-dynobj layout, header
/// +6 u16 bit 6) — an `Object.create(null)` dict inherits nothing
/// and its `__proto__` entry (if any) is plain data.
const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

/// The receiver dynobj's user [[Prototype]] as a borrowed cell box
/// (RFC 20260717-user-proto-chain knife 2). Reads the own
/// `__proto__` simulation entry: a heap-cell payload IS the parent
/// (`Object.create(parent)` / class `__proto_<C>` chains store it
/// there); `None` for the null-proto shape, an absent entry (the
/// implicit %Object.prototype% chain — the builtin reify
/// fallthrough owns that), or a non-cell payload.
///
/// # Safety
/// `ptr` is a live `Tag::DynObj` heap pointer.
pub(crate) unsafe fn user_proto_cell(ptr: *const c_void) -> Option<u64> {
    let flags = unsafe { ptr.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
        return None;
    }
    unsafe {
        let k = __torajs_str_alloc(b"__proto__".as_ptr(), 9);
        let tag = __torajs_dynobj_get_tag(ptr, k as *const c_void);
        let val = __torajs_dynobj_get_value(ptr, k as *const c_void);
        __torajs_str_drop(k as *mut c_void);
        // ANY_HEAP = 4; the entry's NaN-box IS the cell pointer bits.
        if tag == 4 && val != 0 {
            Some(val)
        } else {
            None
        }
    }
}

/// Arr cell length slot — torajs-arr `layout::ARR_LEN_OFF` mirror.
const ARR_LEN_OFF: usize = 8;

/// Canonical array-index parse — ES §10.4.2 array index shape
/// (`"0"`, or nonzero-leading all-digits, value `< 2^32 - 1`).
/// `arr_reflect.rs` (torajs-meta) twin.
pub(crate) fn canonical_index(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 10 {
        return None;
    }
    if bytes == b"0" {
        return Some(0);
    }
    if bytes[0] == b'0' || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut v: u64 = 0;
    for &b in bytes {
        v = v * 10 + (b - b'0') as u64;
    }
    if v < u32::MAX as u64 { Some(v) } else { None }
}

/// `Tag::Arr` own-property probe for a dynamic string key (RFC
/// 20260712-arr-exotic-define chunk A) — `"length"` answers the len
/// as I64; a canonical index answers the element via the kind-aware
/// slot read (in-range) or a definite `(ANY_UNDEF, 0)` (out-of-range
/// — the index domain is owned by element storage, never by the
/// expando dynobj). `None` = not an own-domain key, fall through to
/// the expando probe / builtin reify. Borrow-shaped like every
/// other probe answer. Pre-fix `o[k]` with a runtime `"length"` /
/// index key answered `undefined` for every Array receiver (the
/// literal-key form lowers to the static member read and was fine).
///
/// # Safety
/// `ptr` is a live `Tag::Arr` heap pointer; `key` is a live Str cell.
pub(crate) unsafe fn arr_own_pair(ptr: *mut c_void, key: *const c_void) -> Option<(u64, u64)> {
    let k = key as *const u8;
    let key_len = unsafe { k.add(STR_LEN_OFF).cast::<u32>().read() };
    let bytes = unsafe { core::slice::from_raw_parts(k.add(STR_DATA_OFF), key_len as usize) };
    let len = unsafe { ptr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
    if bytes == b"length" {
        return Some((AnySlotTag::I64 as u64, len));
    }
    let idx = canonical_index(bytes)?;
    if idx >= len {
        return Some((5, 0));
    }
    Some(unsafe {
        (
            __torajs_arr_get_any_tag(ptr, idx),
            __torajs_arr_get_any_value(ptr, idx),
        )
    })
}

/// `Tag::StringWrapper` inherent `"length"` probe (§10.4.3
/// StringGetOwnProperty; the get twin of prop_has's 刀 13 arm) —
/// `Some(len)` when `key` is the literal bytes `length`, reading
/// the inner Str cell's code-unit count (NULL inner = empty = 0).
/// Pre-fix the dynamic-key read (`obj[k]`, k = "length") fell to
/// the expando probe and answered undefined — test262's
/// propertyHelper reads every key dynamically, so verifyProperty
/// on `new String('')`.length saw oldValue undefined and a
/// writable store that landed in the expando (the literal-key form
/// pre-lowers to the static length read and was fine). The numeric
/// code-unit index face stays on the expando fall-through: its own
/// read would mint a fresh char cell, and this pair is
/// borrow-shaped (recorded residue, RFC 20260717-objlit-anylane-recv
/// knife-2g note).
pub(crate) unsafe fn strwrapper_length(ptr: *mut c_void, key: *const c_void) -> Option<u64> {
    let k = key as *const u8;
    let key_len = unsafe { k.add(STR_LEN_OFF).cast::<u32>().read() };
    let bytes = unsafe { core::slice::from_raw_parts(k.add(STR_DATA_OFF), key_len as usize) };
    if bytes != b"length" {
        return None;
    }
    let inner = unsafe { (ptr.cast::<u8>().add(8) as *const *const c_void).read() };
    let len = if inner.is_null() {
        0
    } else {
        unsafe { inner.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() as u64 }
    };
    Some(len)
}

/// Virtual §20.2.4 `name` / `length` pair for the borrow-shaped
/// dynamic-key probe (chunk D, RFC 20260711-closure-reflection) —
/// `f[k]` with k == "name"/"length" answers the same metadata the
/// static member read does, tombstone-gated (chunk C). `name` hands
/// out an IMMORTAL interned cell (`closure_virtual_name_cell`;
/// bound cells stay None — recorded boundary), `length` is an i64
/// immediate. `None` falls to the builtin reify probe.
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell; `key` a live Str cell.
pub(crate) unsafe fn closure_virtual_pair(
    ptr: *mut c_void,
    key: *const c_void,
) -> Option<(u64, u64)> {
    unsafe {
        if crate::prop_has::key_is(key, b"name")
            && !header_flag(ptr, torajs_rc::FLAG_FN_NAME_DELETED)
            && let Some(cell) = crate::name_get::closure_virtual_name_cell(ptr)
        {
            return Some((AnySlotTag::Heap as u64, cell as u64));
        }
        if crate::prop_has::key_is(key, b"length")
            && !header_flag(ptr, torajs_rc::FLAG_FN_LENGTH_DELETED)
        {
            let l = crate::len_get::__torajs_closure_length(ptr);
            if l >= 0 {
                return Some((AnySlotTag::I64 as u64, l as u64));
            }
        }
        None
    }
}
