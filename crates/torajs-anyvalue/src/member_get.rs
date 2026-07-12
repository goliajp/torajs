//! `__torajs_any_member_get_tag` / `_value` — the tag-gated
//! `(tag, value)` probe behind arbitrary-name member reads on `any`
//! receivers (the read mirror of `member_set.rs`; RFC 20260704 C4+).
//!
//! Pre-gate the lowering's fallback handed the receiver's payload
//! bits straight to `__torajs_dynobj_get_tag/value`, reading every
//! cell as a DynObj layout — an Arr receiver's expando probe missed
//! by accident (silent `undefined`), any other tag was an
//! out-of-layout read. The pair below gates first:
//!
//! - null / undefined receiver → catchable TypeError (the tag call
//!   records it; the value call stays silent so the pair doesn't
//!   double-throw), pair answers `(ANY_UNDEF, 0)`.
//! - `Tag::DynObj` → the ordinary own-property probe, accessor
//!   sentinel included (the lowering's `emit_dynobj_get_result`
//!   consumes it unchanged).
//! - `Tag::Arr` → the `arrprops` expando probe (NULL props slot
//!   answers absent).
//! - `Tag::Closure` (L3b #11 residue, chunk 529) → the lazy
//!   `props_dynobj` at `CLOSURE_PROPS_OFF` (T-27 Function-as-Object
//!   expandos; NULL slot answers absent). STATIC `.name` / `.length`
//!   member reads route to `__torajs_any_name_get` /
//!   `__torajs_any_length_get` (chunks 715/716) and never reach this
//!   pair; a DYNAMIC key (`f[k]`, chunk D RFC 20260711) lands here
//!   and answers the same metadata through `closure_virtual_pair`
//!   (immortal interned name cells — the pair is borrow-shaped).
//! - every other receiver (and an Arr / Closure expando miss) →
//!   the builtin-method reification probe (chunk 711,
//!   `method_value`): a supported method name answers the interned
//!   function cell; everything else is `(ANY_UNDEF, 0)` — a
//!   definite absent, never a layout mis-read.
//!
//! The pair is borrow-shaped exactly like the dynobj probe it
//! wraps — and so is the BOX the lowering assembles from it:
//! `anyv_box_from_pair` is a pure bit-encode (no refcount inc; see
//! nanbox_encode.rs), so the consumer slot is a view over the
//! bucket's stake, never an owner. The special-cased member
//! intrinsics (`any_length_get` / `any_name_get` / `any_size_get` /
//! `any_regexp_prop`) answer OWNED boxes instead — that owned/
//! borrow split across the fallback's arms is the recorded
//! 32B-per-read leak lane (L3b, chunk 716 churn probe; the fix
//! unifies every arm to owned).

use core::ffi::c_void;

use torajs_rc::{AnySlotTag, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-arr — expando probe through the props slot.
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
    /// torajs-arr — kind-aware slot reads, borrow contract (RFC
    /// 20260712-arr-exotic-define chunk A dynamic-key arm).
    fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64;
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (mirror of `method_call_dynobj`'s declares).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// ABI mirror of `torajs-structmeta::FieldInfo` (the
/// `__torajs_struct_field_info` return value; `struct_enum.rs` twin).
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const CLOSURE_PROPS_OFF: usize = 24;

/// `class_tag` u32 offset inside a `Tag::Obj` instance / Str-cell
/// layout — mirrors `method_call_dynobj`'s constants.
const OBJ_CLASS_TAG_OFF: usize = 8;
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// The closure's `props_dynobj` pointer, NULL when no expando was
/// ever written.
pub(crate) unsafe fn closure_props(ptr: *mut c_void) -> *const c_void {
    unsafe { *(ptr.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64) as *const c_void }
}

/// Universal heap-header flags probe — u16 at +6 (RFC 20260711
/// chunk C consumers test the `FLAG_FN_*_DELETED` tombstones).
///
/// # Safety
/// `ptr` is a live heap cell.
pub(crate) unsafe fn header_flag(ptr: *const c_void, bit: u16) -> bool {
    unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() & bit != 0 }
}

/// Set a heap-header flag bit (read-or-write, u16 at +6).
///
/// # Safety
/// `ptr` is a live heap cell.
pub(crate) unsafe fn header_flag_set(ptr: *mut c_void, bit: u16) {
    unsafe {
        let p = ptr.cast::<u8>().add(6) as *mut u16;
        p.write(p.read() | bit);
    }
}

/// `Tag::Obj` struct-cell field probe — the class-layout reflection
/// walk (`struct_reflect::struct_cell_descriptor` twin): class_tag
/// (u32 @ +8) → layout entry → field_find by key bytes → raw 8-byte
/// slot decoded per the field's coarse type_tag. Answers the
/// `(any_tag, payload)` pair on a hit — borrow-shaped like the
/// dynobj probe (the struct keeps its stake) — or `None` for a
/// missing layout / absent field. Chunk 744: pre-fix a struct cell
/// fell to the builtin-reify arm and every field read through an
/// `any` receiver whose sid the compile-time IC couldn't see (a
/// Pass 2 fresh literal in a later-lowered fn) answered a silent
/// `undefined`.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str cell.
pub(crate) unsafe fn struct_field_pair(ptr: *mut c_void, key: *const c_void) -> Option<(u64, u64)> {
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return None;
    }
    let k = key as *const u8;
    let key_len = unsafe { k.add(STR_LEN_OFF).cast::<u32>().read() };
    let key_bytes = unsafe { k.add(STR_DATA_OFF) };
    let idx = unsafe { __torajs_struct_field_find(layout, key_bytes, key_len) };
    if idx == u32::MAX {
        return None;
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let raw = unsafe {
        ptr.cast::<u8>()
            .add(info.field_byte_offset as usize)
            .cast::<u64>()
            .read()
    };
    Some(match info.type_tag {
        // Any-typed field: the slot is a NaN-box — decode it.
        0 => (
            crate::nanbox_encode::__torajs_anyv_unbox_tag(raw) as u64,
            crate::nanbox_encode::__torajs_anyv_unbox_value(raw) as u64,
        ),
        1 => (AnySlotTag::I64 as u64, raw),
        2 => (AnySlotTag::F64 as u64, raw),
        3 => (AnySlotTag::Bool as u64, raw),
        _ => (AnySlotTag::Heap as u64, raw),
    })
}

/// Array `len` u64 offset — mirrors `torajs-arr::layout::ARR_LEN_OFF`.
const ARR_LEN_OFF: usize = 8;

/// Canonical array-index parse — ES §10.4.2 array index shape
/// (`"0"`, or nonzero-leading all-digits, value `< 2^32 - 1`).
/// `arr_reflect.rs` (torajs-meta) twin.
fn canonical_index(bytes: &[u8]) -> Option<u64> {
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
unsafe fn arr_own_pair(ptr: *mut c_void, key: *const c_void) -> Option<(u64, u64)> {
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

/// Cell tag of a dispatchable receiver, `None` for everything the
/// gate answers `(ANY_UNDEF, 0)` for.
pub(crate) fn recv_cell(recv: AnyValue) -> Option<(*mut c_void, u16)> {
    if !is_cell(recv) {
        return None;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a non-null encoded pointer; the
    // caller invariant says it points to a live heap object.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    Some((ptr, tag))
}

/// See module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_tag(recv: AnyValue, key: *const c_void) -> u64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return 5;
    }
    match recv_cell(recv) {
        // Entry miss falls through to the builtin-proto own-method
        // probe (RFC 20260712 chunk 2) — a builtin `<Ctor>.prototype`
        // singleton hands out its interned family cells so
        // `(String.prototype as any).small` reads the same immortal
        // cell the static form does. Ordinary dynobjs answer 0 there.
        Some((ptr, t)) if t == Tag::DynObj as u16 => unsafe {
            let tag = __torajs_dynobj_get_tag(ptr, key);
            if tag != 5 {
                return tag;
            }
            if crate::method_support::__torajs_builtin_proto_own_method_cell(ptr, key) != 0 {
                4
            } else {
                // Ordinary dynobj — the inherited Object.prototype
                // surface still reifies (valueOf / toLocaleString /
                // the universal probes), same fallthrough as the
                // Arr / Closure / struct arms.
                reify_tag(recv, key)
            }
        },
        Some((ptr, t)) if t == Tag::Arr as u16 => unsafe {
            if let Some((tag, _)) = arr_own_pair(ptr, key) {
                return tag;
            }
            let tag = __torajs_arrprops_get_tag(ptr, key);
            if tag != 5 {
                return tag;
            }
            reify_tag(recv, key)
        },
        Some((ptr, t)) if t == Tag::Closure as u16 => unsafe {
            let props = closure_props(ptr);
            if !props.is_null() {
                let tag = __torajs_dynobj_get_tag(props, key);
                if tag != 5 {
                    return tag;
                }
            }
            if let Some((tag, _)) = closure_virtual_pair(ptr, key) {
                return tag;
            }
            reify_tag(recv, key)
        },
        // Chunk 744 — struct cell: class-layout field probe before
        // the builtin reify (a struct has no builtin methods, so a
        // field miss falling through is exact).
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            if let Some((tag, _)) = struct_field_pair(ptr, key) {
                return tag;
            }
            reify_tag(recv, key)
        },
        _ => unsafe { reify_tag(recv, key) },
    }
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
unsafe fn closure_virtual_pair(ptr: *mut c_void, key: *const c_void) -> Option<(u64, u64)> {
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

/// Builtin-method reification probe (chunk 711) — a supported
/// method name on a builtin receiver answers a heap tag (the
/// interned function cell); everything else stays absent.
///
/// # Safety
/// `key` is NULL or a live Str cell.
unsafe fn reify_tag(recv: AnyValue, key: *const c_void) -> u64 {
    if unsafe { crate::method_value::builtin_method_lookup(recv, key) }.is_some() {
        4
    } else {
        5
    }
}

/// `o.m?.(…)` GetV-existence probe (chunk 709) — decides whether the
/// optional call's arguments evaluate. Returns 1 = the callee slot
/// resolves to a non-nullish value (or a plausibly-existing builtin
/// method): enter the call step; 0 = nullish / absent: short-circuit
/// to undefined (args never evaluate, per ES §13.3.9).
///
/// - null / undefined receiver → catchable TypeError (`o.m` itself
///   throws; the caller's throw-check propagates before branching).
/// - DynObj → own-property probe: present non-nullish (accessor
///   sentinel included) → 1; absent/nullish → 0 (a dynobj has no
///   builtin methods, so this is exact).
/// - Arr / Closure expandos → present non-nullish → 1; absent falls
///   through to the builtin test (an Arr's `push` is not an expando).
/// - struct cell (`Tag::Obj`) → class-layout field probe; found → 1;
///   miss falls through to the support table (a struct has no
///   builtin methods, so the miss short-circuits to undefined).
/// - everything else → the exact per-receiver-shape support table
///   (chunk 711's `builtin_method_supported`): a supported id
///   enters the call step; a wrong-arm id short-circuits to
///   undefined without evaluating the arguments (chunk 713 —
///   closes 709's recorded residual where `(42 as any).slice?.(f())`
///   ran `f`).
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_method_probe(
    recv: AnyValue,
    mid: i64,
    key: *const c_void,
) -> i64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot call a method of null or undefined".as_ptr());
        }
        return 0;
    }
    let non_nullish = |tag: u64| (tag != 0 && tag != 5) as i64;
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => {
            return non_nullish(unsafe { __torajs_dynobj_get_tag(ptr, key) });
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            let tag = unsafe { __torajs_arrprops_get_tag(ptr, key) };
            if non_nullish(tag) == 1 {
                return 1;
            }
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            let props = unsafe { closure_props(ptr) };
            if !props.is_null() && non_nullish(unsafe { __torajs_dynobj_get_tag(props, key) }) == 1
            {
                return 1;
            }
        }
        Some((ptr, t)) if t == Tag::Obj as u16 => {
            let class_tag =
                unsafe { (ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read() };
            let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
            if !layout.is_null() {
                let name_len = unsafe { (key.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() };
                let name_bytes = unsafe { key.cast::<u8>().add(STR_DATA_OFF) };
                if unsafe { __torajs_struct_field_find(layout, name_bytes, name_len) } != u32::MAX {
                    return 1;
                }
            }
        }
        _ => {}
    }
    // chunk 713 — exact per-receiver-shape support table (chunk
    // 711's reification table) instead of the optimistic known-id
    // test: a wrong-arm name short-circuits to undefined WITHOUT
    // evaluating the arguments (`(42 as any).slice?.(f())` no
    // longer runs `f`, closing chunk 709's recorded residual).
    crate::method_value::builtin_method_supported(recv, mid) as i64
}

/// See module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_value(recv: AnyValue, key: *const c_void) -> u64 {
    match recv_cell(recv) {
        // Miss → builtin-proto own-method cell bits (0 = absent),
        // pairing the tag channel's fallthrough above. The nonzero
        // hit path stays a single hash probe — only a 0 slot (absent
        // OR a stored 0/false/null payload) pays the tag re-probe to
        // disambiguate.
        Some((ptr, t)) if t == Tag::DynObj as u16 => unsafe {
            let v = __torajs_dynobj_get_value(ptr, key);
            if v == 0 && __torajs_dynobj_get_tag(ptr, key) == 5 {
                let cell = crate::method_support::__torajs_builtin_proto_own_method_cell(ptr, key);
                if cell != 0 {
                    return cell;
                }
                // Inherited Object.prototype reify (tag twin above).
                return reify_value(recv, key);
            }
            v
        },
        Some((ptr, t)) if t == Tag::Arr as u16 => unsafe {
            if let Some((_, val)) = arr_own_pair(ptr, key) {
                return val;
            }
            if __torajs_arrprops_get_tag(ptr, key) != 5 {
                return __torajs_arrprops_get_value(ptr, key);
            }
            reify_value(recv, key)
        },
        Some((ptr, t)) if t == Tag::Closure as u16 => unsafe {
            let props = closure_props(ptr);
            if !props.is_null() && __torajs_dynobj_get_tag(props, key) != 5 {
                return __torajs_dynobj_get_value(props, key);
            }
            if let Some((_, val)) = closure_virtual_pair(ptr, key) {
                return val;
            }
            reify_value(recv, key)
        },
        // Chunk 744 — struct cell field probe (see the tag channel).
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            if let Some((_, val)) = struct_field_pair(ptr, key) {
                return val;
            }
            reify_value(recv, key)
        },
        _ => unsafe { reify_value(recv, key) },
    }
}

/// Value channel of [`reify_tag`] — the interned cell's pointer
/// bits (immortal, borrow-shaped like every other probe answer).
///
/// # Safety
/// `key` is NULL or a live Str cell.
unsafe fn reify_value(recv: AnyValue, key: *const c_void) -> u64 {
    unsafe { crate::method_value::builtin_method_lookup(recv, key) }
        .map(|c| c as u64)
        .unwrap_or(0)
}
