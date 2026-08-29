//! Own-domain key probes behind the dynamic-key member reads —
//! split from `member_get.rs` (file-size cap): the per-receiver
//! "is this key an INHERENT own property?" helpers that run before
//! the expando probe / builtin reify. All borrow-shaped, mirroring
//! the pair contract in the parent module doc.

use core::ffi::c_void;

use torajs_rc::{AnySlotTag, Tag};

use crate::member_get::{CLOSURE_PROPS_OFF, STR_DATA_OFF, STR_LEN_OFF, header_flag, wrapper_props};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-arr — kind-aware slot reads, borrow contract (RFC
    /// 20260712-arr-exotic-define chunk A dynamic-key arm).
    fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64;
    /// torajs-arr — the arguments-materialization `"length"` face:
    /// 0 = plain array, 1 = arguments (intact), 2 = deleted.
    fn __torajs_arr_arguments_length_state(arr: *const c_void, key: *const c_void) -> i64;
    /// torajs-str / torajs-dynobj — the `__proto__` simulation-slot
    /// probe behind [`user_proto_cell`].
    fn __torajs_str_alloc(p: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — own-entry existence (a stored `undefined`
    /// shadows the wrapper's built-in surface).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
}

/// `DYNOBJ_HDR_FLAG_NULL_PROTO` mirror (torajs-dynobj layout, header
/// +6 u16 bit 6) — an `Object.create(null)` dict inherits nothing.
pub(crate) const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

/// §10.1.8.1 OrdinaryGet step 2 — is this dynobj's [[Prototype]]
/// explicitly null? A miss on such a host stops at null: no user
/// chain and no builtin reify surface (the index walk in
/// `index_any.rs` already honors this; the named-member get lanes
/// consult it between the user-chain walk and the builtin tail).
///
/// # Safety
/// `ptr` is a live `Tag::DynObj` heap pointer.
pub(crate) unsafe fn dynobj_null_proto(ptr: *const c_void) -> bool {
    let flags = unsafe { ptr.cast::<u8>().add(6).cast::<u16>().read() };
    flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0
}

/// The internal [[Prototype]] simulation-slot key — cross-crate twin
/// of `torajs_meta::reflect::PROTO_SLOT_KEY` (simulation-slot key
/// separation: the leading NUL keeps the internal proto link out of
/// the user-spellable property-name space, so an own `__proto__`
/// DATA entry never collides with it).
pub(crate) const PROTO_SLOT_KEY: &[u8] = b"\x00proto";

/// `Object.prototype`'s builtin tag (torajs-rc `builtin_proto.rs`
/// order) — the implicit-chain root every ordinary dynobj inherits.
pub(crate) const OBJECT_PROTO_TAG: i64 = 1;

/// The receiver dynobj's user [[Prototype]] as a borrowed cell box
/// (RFC 20260717-user-proto-chain knife 2). Reads the internal
/// [`PROTO_SLOT_KEY`] entry: a heap-cell payload IS the parent
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
        let k = proto_slot_key_cell();
        let tag = __torajs_dynobj_get_tag(ptr, k as *const c_void);
        let val = __torajs_dynobj_get_value(ptr, k as *const c_void);
        // ANY_HEAP = 4; the entry's NaN-box IS the cell pointer bits.
        if tag == 4 && val != 0 {
            Some(val)
        } else {
            None
        }
    }
}

/// Interned immortal [`PROTO_SLOT_KEY`] Str cell — the chain walks
/// (member_get / has_property / for-in / OrdinarySet) probe it on
/// every hop, so the per-call mint+drop pair became measurable
/// traffic. Lock-free lazy init; a CAS-race double-mint leaks one
/// immortal cell, benign (`name_get`'s EMPTY_NAME_CELL pattern).
unsafe fn proto_slot_key_cell() -> *mut u8 {
    use core::sync::atomic::{AtomicU64, Ordering};
    static CELL: AtomicU64 = AtomicU64::new(0);
    let p = CELL.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    let cell = crate::method_value::mint_immortal_str(PROTO_SLOT_KEY);
    CELL.store(cell as u64, Ordering::Relaxed);
    cell
}

/// A FUNCTION value's user [[Prototype]] link (405-01 substrate).
/// The link lives on the closure's lazy expando dynobj — the same
/// [`PROTO_SLOT_KEY`] simulation entry a dynobj receiver carries —
/// so the closure cell itself grows no new slot and
/// `Object.setPrototypeOf(D, P)` re-parents the function through the
/// machinery the dynobj lane already exercises.
///
/// Three states, because the read paths act differently on each:
/// `None` = never re-parented (the implicit %Function.prototype%
/// chain — the builtin fallthrough owns it); `Some(None)` = an
/// explicit null (the chain ENDS — falling through to the builtin
/// surface would resurrect what the user severed); `Some(Some(cell))`
/// = the parent.
///
/// # Safety
/// `ptr` is a live `Tag::Closure` heap pointer.
pub(crate) unsafe fn closure_user_proto(ptr: *mut c_void) -> Option<Option<u64>> {
    let props = unsafe { crate::member_get_layout::closure_props(ptr) };
    if props.is_null() {
        return None;
    }
    let flags = unsafe { props.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
        return Some(None);
    }
    unsafe { user_proto_cell(props) }.map(Some)
}

/// C face of [`user_proto_cell`] for sibling runtime crates (the
/// for-in chain walk in torajs-meta, the `in` HasProperty chain in
/// torajs-rc) — NULL when the receiver carries no user
/// [[Prototype]] link (null-proto shape / implicit chain).
///
/// # Safety
/// `ptr` is a live `Tag::DynObj` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_user_proto(ptr: *const c_void) -> *mut c_void {
    unsafe { user_proto_cell(ptr) }.map_or(core::ptr::null_mut(), |v| v as *mut c_void)
}

/// Annex B §B.2.2.1 answer for the dynamic-key read `o[k]` where `k`
/// spells `__proto__`, as a borrow-shaped `(tag, value)` pair (the
/// member-get pair contract). The own DATA probe already ran (and
/// missed) in the caller, so this is the inherited accessor's
/// answer: the receiver's [[Prototype]]. A null-proto receiver has
/// no inherited accessor — plain absent → undefined. The
/// implicit-chain root answers %Object.prototype% (borrowed from the
/// immortal singleton table — no inc); `Object.prototype` itself
/// answers null.
///
/// # Safety
/// `ptr` is a live `Tag::DynObj` heap pointer.
pub(crate) unsafe fn dynobj_proto_pair(ptr: *const c_void) -> (u64, u64) {
    let flags = unsafe { ptr.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
        return (AnySlotTag::Undef as u64, 0);
    }
    if let Some(cell) = unsafe { user_proto_cell(ptr) } {
        return (AnySlotTag::Heap as u64, cell);
    }
    // A builtin prototype answers the parent its own clause gives it,
    // which is %Object.prototype% for all but the five per-family
    // iterator prototypes (§23.1.5.2 puts those under
    // %Iterator.prototype%). Reading the root flatly here skipped that
    // middle link on the string lane, so `[1].values()`'s prototype
    // answered `Object` for `constructor` instead of `Iterator`.
    let bp_tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
    let parent_tag = if bp_tag >= 0 {
        torajs_rc::builtin_proto::proto_parent_tag(bp_tag)
    } else {
        OBJECT_PROTO_TAG
    };
    if parent_tag < 0 {
        return (AnySlotTag::Null as u64, 0);
    }
    let parent = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(parent_tag) };
    (AnySlotTag::Heap as u64, parent as u64)
}

/// The implicit %Object.prototype% hop for a dynobj whose own probe
/// and explicit-chain walk both missed — `None` when the receiver IS
/// `Object.prototype` (the chain root) or carries an explicit null
/// proto, i.e. exactly when there is no further ordinary parent.
///
/// Why this is its own step: [`dynobj_proto_pair`] has always known
/// the right answer — it is what a dynamic `__proto__` READ returns
/// — but neither get channel's chain walk ever asked it. Both went
/// straight from "no explicit parent" to the builtin reify tail,
/// which only surfaces the SPEC-GIVEN methods, so a property the
/// program installed on `Object.prototype` itself had no path to any
/// receiver at all: `Object.prototype.foo = 5; ({}).foo` answered
/// undefined while `Object.prototype.foo` answered 5, and the
/// symbol-keyed twin did the same. The property was installed and
/// simply unreachable.
///
/// Recursing through the parent's own full [[Get]] — rather than
/// probing that singleton's dict inline — is what makes the hop
/// ordinary (§10.1.8.1 OrdinaryGet step 4): a user entry shadows the
/// reified method exactly as it should, and the walk terminates
/// because the root's own pair answers Null.
///
/// # Safety
/// `ptr` is a live `Tag::DynObj` heap pointer.
pub(crate) unsafe fn implicit_proto_parent(ptr: *const c_void) -> Option<u64> {
    let (tag, val) = unsafe { dynobj_proto_pair(ptr) };
    (tag == AnySlotTag::Heap as u64).then_some(val)
}

/// `Function.prototype`'s expando dynobj (builtin-proto registry
/// tag 13) — the inheritance table a closure receiver reads through
/// after its own expando and virtual pair miss (`Function.prototype
/// .writable = true; funObj.writable` answers true). NULL until the
/// singleton is first materialized, or until something defines on it.
///
/// The singleton is itself a Closure cell (§20.2.3 makes
/// %Function.prototype% a built-in function object), so its own
/// entries live in the +24 expando like any other function's — the
/// same indirection `array_proto_props` does for the Arr-cell
/// `Array.prototype`. Handing back the CELL here made every closure
/// receiver read `fn_addr` as an entry count.
pub(crate) fn function_proto_props() -> *const c_void {
    let fp = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(13) };
    if fp.is_null() {
        return core::ptr::null();
    }
    unsafe { (fp.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const *const c_void).read() }
}

/// A primitive wrapper's prototype expando dynobj — the tag-0/3/4/5
/// singleton (Number/String/Boolean/Symbol); wrapper receivers
/// inherit through it after their own expando misses.
pub(crate) fn wrapper_proto_props(t: u16) -> *const c_void {
    let tag = if t == Tag::StringWrapper as u16 {
        3
    } else if t == Tag::BooleanWrapper as u16 {
        4
    } else if t == Tag::SymbolWrapper as u16 {
        5
    } else {
        0
    };
    unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(tag) as *const c_void }
}

/// `Array.prototype`'s expando dynobj — the tag-2 singleton is an
/// Arr cell (§23.1.3 array exotic) whose monkey-patches land in ITS
/// props table; an Arr receiver inherits through it after its own
/// expando misses.
pub(crate) fn array_proto_props() -> *const c_void {
    let ap = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(2) };
    if ap.is_null() {
        return core::ptr::null();
    }
    unsafe { (ap.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const *const c_void).read() }
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
        // A deleted arguments-materialization length (§10.4.4 hole
        // tombstone) answers a definite undefined — falling through
        // would let the expando probe read the tombstone entry
        // itself. Recorded approximation: the spec would continue
        // into the prototype walk (an Object.prototype `length`
        // accessor is observable there); the deleted state is a
        // narrow verifyProperty probe window, L3b.
        if unsafe { __torajs_arr_arguments_length_state(ptr, key) } == 2 {
            return Some((5, 0));
        }
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
        // §23.2.6.1 — `BYTES_PER_ELEMENT` is an own property of each
        // typed-array CONSTRUCTOR, and the constructor read through
        // a runtime value is an interned ctor cell. The static
        // lowering folds the same number off the name; this is the
        // face `C.BYTES_PER_ELEMENT` needs when `C` came out of an
        // array (RFC 20260823-typedarray-substrate 刀 4).
        if crate::prop_has::key_is(key, b"BYTES_PER_ELEMENT")
            && let Some(tag) = crate::method_value::ctor::ctor_tag_of_cell(ptr)
            && let Some(n) = typedarray_bytes_per_element(tag)
        {
            return Some((AnySlotTag::I64 as u64, n as u64));
        }
        None
    }
}

/// Primitive-wrapper cell `[[GetOwnProperty]]` tag probe (extracted
/// from `__torajs_any_member_get_tag`'s Number / String / Boolean /
/// Symbol Wrapper arm as the rotation-196 file-size sweep). Own-face
/// order: StringWrapper `length` inherent → expando dynobj →
/// inherited `<Wrapper>.prototype` expando → SymbolWrapper
/// `description` accessor (§20.4.3.2) → builtin-method reify tail.
///
/// # Safety
/// `ptr` is a live wrapper cell (`is_wrapper_tag(t)`); `key` is NULL
/// or a live Str cell; `recv` NaN-boxes the wrapper.
pub(crate) unsafe fn wrapper_arm_tag(
    ptr: *mut c_void,
    key: *const c_void,
    t: u16,
    recv: AnyValue,
) -> u64 {
    unsafe {
        if t == Tag::StringWrapper as u16 {
            if strwrapper_length(ptr, key).is_some() {
                return AnySlotTag::I64 as u64;
            }
            // §10.4.3 — the wrapper's inherent index face is
            // non-configurable, so it answers ahead of the expando.
            if let Some(inner) = crate::wrapper_view_through::resolve_inner_recv(ptr, t)
                && let Some((tag, _)) = crate::member_get_str::str_own_pair(inner, key)
            {
                return tag;
            }
        }
        let props = wrapper_props(ptr);
        if !props.is_null() {
            let tag = __torajs_dynobj_get_tag(props, key);
            if tag != 5 {
                return tag;
            }
            // Stored-undefined expando shadows the built-in
            // wrapper surface.
            if __torajs_dynobj_has(props, key) != 0 {
                return 5;
            }
        }
        // Inherited <Wrapper>.prototype expando.
        let wp = wrapper_proto_props(t);
        if !wp.is_null() {
            let tag = __torajs_dynobj_get_tag(wp, key);
            if tag != 5 {
                return tag;
            }
            if __torajs_dynobj_has(wp, key) != 0 {
                return 5;
            }
        }
        // §20.4.3.2 — the proto `description` accessor over a
        // SymbolWrapper receiver reads the inner cell's
        // [[Description]] (thisSymbolValue unwraps the wrapper).
        if t == Tag::SymbolWrapper as u16 && crate::prop_has::key_is(key, b"description") {
            let inner = (ptr.cast::<u8>().add(8) as *const *const c_void).read();
            if crate::member_get_layout::symbol_desc(inner).is_null() {
                return AnySlotTag::Undef as u64;
            }
            return AnySlotTag::Heap as u64;
        }
        crate::member_get::reify_tag(recv, key)
    }
}

/// §23.2.5.1 table 71, keyed by the builtin-proto family tag rather
/// than by the element kind — the eleven typed-array families sit at
/// 20-30 in element-kind order, so the two are one subtraction
/// apart. `None` for every other family, including ArrayBuffer,
/// which has no such property.
fn typedarray_bytes_per_element(family_tag: i64) -> Option<i64> {
    Some(match family_tag - 20 {
        // Int8 / Uint8 / Uint8Clamped.
        0..=2 => 1,
        // Int16 / Uint16, and Float16 which was appended at 11.
        3 | 4 | 11 => 2,
        // Int32 / Uint32 / Float32.
        5..=7 => 4,
        // Float64 / BigInt64 / BigUint64.
        8..=10 => 8,
        _ => return None,
    })
}
