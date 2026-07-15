//! The inherited `Object.prototype` surface arms that dispatch on
//! EVERY receiver shape — the §20.1.4.3/.5 universal own-property
//! probes and the §20.1.3.6 toString badge classifier (split from
//! `method_call.rs`, file-size limit; RFC
//! 20260713-array-proto-residual blade 2 added the badge).

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_HAS_OWN_PROPERTY, Tag};

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, is_bool, is_double, is_int32, is_null, is_short_str, is_undefined,
};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-str — allocate a fresh Str from raw bytes.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str/Substr reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-meta — Error.prototype.toString (§20.5.3.4): render
    /// `name: message` from a FLAG_ERROR OBJ instance pointer.
    fn __torajs_error_to_string(p: *const u8) -> *mut u8;
}

/// `Error.prototype.toString` (§20.5.3.4) for a struct receiver: a
/// FLAG_ERROR OBJ answers `name: message` (via the torajs-meta helper,
/// the same one the SSA typed-tier lowering emits), boxed as an owned
/// Str. `None` when the struct is not Error-derived — the caller then
/// answers the "[object Object]" badge (ES §19.1.3.6). Ordered after
/// the own-property probe by its call site, so a monkey-patched own
/// `toString` still wins.
///
/// # Safety
/// `obj` is a live `Tag::Obj` struct cell.
pub(crate) unsafe fn error_struct_tostring(obj: *mut c_void) -> Option<AnyValue> {
    let flags = unsafe { (obj.cast::<u8>().add(6) as *const u16).read() };
    if flags & torajs_rc::FLAG_ERROR == 0 {
        return None;
    }
    let s = unsafe { __torajs_error_to_string(obj.cast::<u8>()) };
    Some(unsafe { __torajs_anyv_box_pointer(s as *mut c_void) })
}

/// chunk D-1 — `hasOwnProperty` / `propertyIsEnumerable` universal
/// arm: ToPropertyKey the first argument (`anyv_to_str` — a missing
/// slot stringifies undefined per §7.1.19), probe the prop_has /
/// prop_enumerable substrate, answer a Bool box. The key temp is
/// owned and dropped here.
pub(crate) unsafe fn own_prop_probe(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let key_av = if argc >= 1 {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    let key = unsafe { crate::nanbox_ffi::__torajs_anyv_to_str(key_av) };
    let hit = if mid == ANY_METHOD_HAS_OWN_PROPERTY {
        unsafe { crate::prop_has::__torajs_any_prop_has(recv, key as *const c_void) }
    } else {
        unsafe { crate::prop_has::__torajs_any_prop_enumerable(recv, key as *const c_void) }
    };
    unsafe { __torajs_str_drop(key as *mut c_void) };
    if hit != 0 {
        crate::nanbox::VALUE_TRUE
    } else {
        crate::nanbox::VALUE_FALSE
    }
}

/// §20.1.3.6 `Object.prototype.toString` — classify the this-value
/// into its "[object X]" badge. Steps 1-2 answer Undefined / Null
/// without ToObject; the builtinTag walk maps each cell tag onto
/// the legacy badge set (Array / Function / Error / Boolean /
/// Number / String / Date / RegExp) plus the well-known
/// `Symbol.toStringTag` surfaces bun answers for the container
/// tags (Map / Set / Promise / Symbol / BigInt / WeakMap /
/// WeakSet / WeakRef). Everything else is "Object".
pub(crate) unsafe fn object_proto_to_string(recv: AnyValue) -> AnyValue {
    let badge: &'static [u8] = if is_undefined(recv) {
        b"Undefined"
    } else if is_null(recv) {
        b"Null"
    } else if is_bool(recv) {
        b"Boolean"
    } else if is_int32(recv) || is_double(recv) {
        b"Number"
    } else if is_short_str(recv) {
        b"String"
    } else if let Some((ptr, tag)) = crate::member_get::recv_cell(recv) {
        unsafe { cell_badge(ptr, tag) }
    } else {
        b"Object"
    };
    unsafe { badge_string(badge) }
}

/// The badge a heap cell classifies into. Shared with the
/// `Object.prototype.toString` fallback a builtin prototype reaches
/// when nothing else claims the call.
///
/// # Safety
/// `ptr` is a live heap cell whose header tag is `tag`.
pub(crate) unsafe fn cell_badge(ptr: *mut c_void, tag: u16) -> &'static [u8] {
    // A builtin prototype answers for what it IS, which the cell tag
    // alone cannot always say: `Number.prototype` is a dynobj in tr
    // but a Number object per §21.1.3, and the five container
    // prototypes carry a well-known `Symbol.toStringTag`. (The ones
    // that are genuinely ordinary objects — Object / RegExp / Date /
    // Error, none of which has the well-known tag — keep "Object",
    // matching bun.)
    let proto_tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
    if proto_tag >= 0 {
        return match proto_tag {
            0 => b"Number",
            2 => b"Array",
            3 => b"String",
            4 => b"Boolean",
            5 => b"Symbol",
            6 => b"BigInt",
            10 => b"Promise",
            11 => b"Map",
            12 => b"Set",
            13 => b"Function",
            _ => b"Object",
        };
    }
    match tag {
        t if t == Tag::Str as u16 => b"String",
        t if t == Tag::Arr as u16 => b"Array",
        t if t == Tag::Closure as u16 => b"Function",
        t if t == Tag::Date as u16 => b"Date",
        t if t == Tag::RegExp as u16 => b"RegExp",
        t if t == Tag::Map as u16 => b"Map",
        t if t == Tag::Set as u16 => b"Set",
        t if t == Tag::Promise as u16 => b"Promise",
        t if t == Tag::Symbol as u16 => b"Symbol",
        t if t == Tag::BigInt as u16 => b"BigInt",
        t if t == Tag::WeakMap as u16 => b"WeakMap",
        t if t == Tag::WeakSet as u16 => b"WeakSet",
        t if t == Tag::WeakRef as u16 => b"WeakRef",
        t if t == Tag::Undefined as u16 => b"Undefined",
        // RFC 20260716 刀 2 — primitive-wrapper cells classify by what
        // they wrap.
        t if t == Tag::NumberWrapper as u16 => b"Number",
        t if t == Tag::StringWrapper as u16 => b"String",
        t if t == Tag::BooleanWrapper as u16 => b"Boolean",
        // Errors are static-layout structs carrying FLAG_ERROR
        // (disjoint-by-tag bit 7).
        t if t == Tag::Obj as u16 => {
            let flags = unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() };
            if flags & torajs_rc::FLAG_ERROR != 0 {
                b"Error"
            } else {
                b"Object"
            }
        }
        _ => b"Object",
    }
}

/// The badge a receiver reaching the `Object.prototype` fallback
/// answers with. A struct receiver has no badge of its own; every
/// cell classifies through [`cell_badge`].
///
/// # Safety
/// `obj` is a live heap cell.
pub(crate) unsafe fn cell_badge_string(obj: *mut c_void, is_struct: bool) -> AnyValue {
    let badge: &'static [u8] = if is_struct {
        b"Object"
    } else {
        // HeapHeader: type_tag @ +4 (u16).
        let tag = unsafe { obj.cast::<u8>().add(4).cast::<u16>().read() };
        unsafe { cell_badge(obj, tag) }
    };
    unsafe { badge_string(badge) }
}

/// `"[object " + badge + "]"` as an owned Str box.
///
/// # Safety
/// `badge` is at most 9 bytes (every caller passes a literal).
unsafe fn badge_string(badge: &'static [u8]) -> AnyValue {
    let mut buf = [0u8; 24];
    buf[..8].copy_from_slice(b"[object ");
    buf[8..8 + badge.len()].copy_from_slice(badge);
    buf[8 + badge.len()] = b']';
    let len = 8 + badge.len() + 1;
    unsafe {
        let p = __torajs_str_alloc(buf.as_ptr(), len as i64);
        __torajs_anyv_box_pointer(p as *mut c_void)
    }
}
