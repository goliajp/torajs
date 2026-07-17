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

use crate::member_get_own::{arr_own_pair, closure_virtual_pair};
pub(crate) use crate::member_get_own::{canonical_index, strwrapper_length};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — own-entry existence (disambiguates a stored
    /// `undefined` from absent: `get_tag` answers 5 for both).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-arr — expando twin of the dynobj_has probe.
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
    /// torajs-arr — expando probe through the props slot.
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (mirror of `method_call_dynobj`'s declares). The field/accessor
    /// PROBE over a struct cell lives in `struct_probe.rs`; the method
    /// existence test below is the only direct walk left here.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const CLOSURE_PROPS_OFF: usize = 24;

/// Wrapper-cell lazy props slot — mirror of
/// `torajs-wrapper::WRAPPER_PROPS_OFF` (RFC 20260716 刀 5, rotation
/// 121). Every wrapper cell layout is `[header:8][value:8][props:8]`.
const WRAPPER_PROPS_OFF: usize = 16;

/// The wrapper's `props_dynobj` pointer, NULL when no expando was
/// ever written. Same read shape as `closure_props`.
pub(crate) unsafe fn wrapper_props(ptr: *mut c_void) -> *const c_void {
    unsafe { *(ptr.cast::<u8>().add(WRAPPER_PROPS_OFF) as *const u64) as *const c_void }
}

#[inline]
fn is_wrapper_tag(t: u16) -> bool {
    t == Tag::NumberWrapper as u16
        || t == Tag::StringWrapper as u16
        || t == Tag::BooleanWrapper as u16
}

/// `class_tag` u32 offset inside a `Tag::Obj` instance / Str-cell
/// layout — mirrors `method_call_dynobj`'s constants.
const OBJ_CLASS_TAG_OFF: usize = 8;
pub(crate) const STR_LEN_OFF: usize = 8;
pub(crate) const STR_DATA_OFF: usize = 16;

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
            // An own entry STORING undefined shadows the proto
            // surface (`o.toString = undefined` must not reify the
            // builtin) — `get_tag` answers 5 for both shapes, the
            // has probe disambiguates (777e756c's read-side leg).
            if __torajs_dynobj_has(ptr, key) != 0 {
                return 5;
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
            // Stored-undefined expando shadows the builtin surface
            // (`arr.join = undefined` reads undefined, not the
            // reified join cell).
            if __torajs_arrprops_has(ptr, key) != 0 {
                return 5;
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
                // Stored-undefined expando shadows the virtual
                // name/length pair and the builtin reify.
                if __torajs_dynobj_has(props, key) != 0 {
                    return 5;
                }
            }
            if let Some((tag, _)) = closure_virtual_pair(ptr, key) {
                return tag;
            }
            reify_tag(recv, key)
        },
        // RFC 20260716 刀 5 (rotation 121 chunk 4) — wrapper cell
        // own-property probe via the +16 lazy expando (mirror of the
        // closure arm above). Miss falls through to `reify_tag`,
        // which handles the wrapper's inherited built-in surface
        // (`.valueOf` / `.toString` / `.length` on StringWrapper etc.)
        // via the per-wrapper method tables.
        Some((ptr, t)) if is_wrapper_tag(t) => unsafe {
            if t == Tag::StringWrapper as u16 && strwrapper_length(ptr, key).is_some() {
                return AnySlotTag::I64 as u64;
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
            reify_tag(recv, key)
        },
        // Chunk 744 — struct cell: class-layout field probe before
        // the builtin reify (a struct has no builtin methods, so a
        // field miss falling through is exact).
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            if let Some((tag, _)) = crate::struct_probe::struct_field_pair(ptr, key) {
                return tag;
            }
            // Blade 5 — an accessor property answers the sentinel; the
            // probe pair must NOT invoke (it runs twice, once per
            // channel). The emitted accessor arm does the single
            // [[Get]] through `__torajs_any_accessor_get`.
            if crate::struct_probe::struct_accessor_key(ptr, key) {
                return crate::struct_probe::ANY_ACCESSOR_TAG;
            }
            reify_tag(recv, key)
        },
        _ => unsafe { reify_tag(recv, key) },
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
            // Stored-undefined expando shadows the builtin — the
            // optional call short-circuits (`arr.join = undefined;
            // arr.join?.()` is undefined, not a reified join).
            if tag == 5 && unsafe { __torajs_arrprops_has(ptr, key) } != 0 {
                return 0;
            }
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            let props = unsafe { closure_props(ptr) };
            if !props.is_null() {
                if non_nullish(unsafe { __torajs_dynobj_get_tag(props, key) }) == 1 {
                    return 1;
                }
                // Stored-undefined shadow — see the Arr arm.
                if unsafe { __torajs_dynobj_has(props, key) } != 0 {
                    return 0;
                }
            }
        }
        // RFC 20260716 刀 5 (rotation 121 chunk 4) — wrapper own-
        // property expando probe. Miss falls through to the shared
        // `builtin_method_supported` table below (mirror of the
        // Arr / Closure fall-through — a wrapper's inherited
        // `.toString` / `.valueOf` etc. surface reifies there).
        Some((ptr, t)) if is_wrapper_tag(t) => {
            let props = unsafe { wrapper_props(ptr) };
            if !props.is_null() {
                if non_nullish(unsafe { __torajs_dynobj_get_tag(props, key) }) == 1 {
                    return 1;
                }
                // Stored-undefined shadow — see the Arr arm.
                if unsafe { __torajs_dynobj_has(props, key) } != 0 {
                    return 0;
                }
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
                // Stored-undefined shadow — see the tag twin.
                if __torajs_dynobj_has(ptr, key) != 0 {
                    return 0;
                }
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
            // Stored-undefined shadow — see the tag twin.
            if __torajs_arrprops_has(ptr, key) != 0 {
                return 0;
            }
            reify_value(recv, key)
        },
        Some((ptr, t)) if t == Tag::Closure as u16 => unsafe {
            let props = closure_props(ptr);
            if !props.is_null() {
                if __torajs_dynobj_get_tag(props, key) != 5 {
                    return __torajs_dynobj_get_value(props, key);
                }
                // Stored-undefined shadow — see the tag twin.
                if __torajs_dynobj_has(props, key) != 0 {
                    return 0;
                }
            }
            if let Some((_, val)) = closure_virtual_pair(ptr, key) {
                return val;
            }
            reify_value(recv, key)
        },
        // RFC 20260716 刀 5 (rotation 121 chunk 4) — wrapper own-
        // property expando value probe (mirror of the closure arm).
        Some((ptr, t)) if is_wrapper_tag(t) => unsafe {
            if t == Tag::StringWrapper as u16
                && let Some(len) = strwrapper_length(ptr, key)
            {
                return len;
            }
            let props = wrapper_props(ptr);
            if !props.is_null() {
                if __torajs_dynobj_get_tag(props, key) != 5 {
                    return __torajs_dynobj_get_value(props, key);
                }
                // Stored-undefined shadow — see the tag twin.
                if __torajs_dynobj_has(props, key) != 0 {
                    return 0;
                }
            }
            reify_value(recv, key)
        },
        // Chunk 744 — struct cell field probe (see the tag channel).
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            if let Some((_, val)) = crate::struct_probe::struct_field_pair(ptr, key) {
                return val;
            }
            // Blade 5 — a struct accessor has no AccessorPair cell to
            // hand over: the ZERO value channel is what tells
            // `__torajs_any_accessor_get` to take the struct lane.
            if crate::struct_probe::struct_accessor_key(ptr, key) {
                return 0;
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
