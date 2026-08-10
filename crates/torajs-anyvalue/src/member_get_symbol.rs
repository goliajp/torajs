//! §6.1.7 symbol-keyed member reads — the `o[sym]` lane.
//!
//! A symbol key is a wholly separate key domain from a string key, so
//! it gets its own short walk instead of threading through the
//! string cascade in [`crate::member_get`]. Every arm of that cascade
//! is a three-step shape — entry-table probe, then **name-keyed**
//! fallbacks (`key_is(key, b"prototype")`, the virtual `name` /
//! `length` pair, canonical-index decode, builtin-prototype method
//! reify), then the prototype chain. Only the first and last steps
//! mean anything for a symbol key, and the middle ones would read Str
//! payload offsets off a 16-byte Symbol cell.
//!
//! So this lane is exactly: probe the receiver's own property dict,
//! then walk what it inherits. §10.1.8.1 OrdinaryGet does not care
//! which key kind it is looking up, which is why the chain walk is
//! shared and the recursion covers grandparents.
//!
//! Both `get_tag` and `get_value` route here, so the pair is resolved
//! once per shape and the two extern faces cannot disagree.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::{closure_props, is_wrapper_tag, recv_cell, wrapper_props};
use crate::member_get_own::{array_proto_props, function_proto_props, user_proto_cell};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-meta — borrow read of `PROTOS_BY_TAG_IMM[tag]`
    /// (RFC 20260802 刀 3a struct symbol-key inheritance).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// Disambiguates "absent" from "present, storing undefined" — both
    /// answer tag 5 from `get_tag`.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
}

/// `torajs_dynobj::layout::TAG_SYMBOL_KEY` mirror — a property-key
/// cell is a Str (tag 0) or a Symbol (tag 7).
const TAG_SYMBOL_KEY: u16 = 7;

/// `AnySlotTag::Undef` — the absent / undefined answer.
const TAG_UNDEF: u64 = 5;

/// True when the property-key cell is a Symbol rather than a Str.
///
/// # Safety
/// `key` must be non-NULL and point at a live key cell.
#[inline]
pub(crate) unsafe fn key_is_symbol(key: *const c_void) -> bool {
    unsafe { key.cast::<u8>().add(4).cast::<u16>().read() == TAG_SYMBOL_KEY }
}

/// The receiver's own property dict — the cell itself for a DynObj,
/// else the in-layout expando slot its shape carries. NULL when the
/// shape has no dict (or has not lazily allocated one yet), in which
/// case it holds no symbol-keyed property.
///
/// Shared by every symbol-key face in this crate (read / has / delete)
/// so "where does this shape keep its properties" is answered in one
/// place.
///
/// # Safety
/// `ptr` is a live cell whose `type_tag` is `t`.
pub(crate) unsafe fn own_dict(ptr: *mut c_void, t: u16) -> *const c_void {
    if t == Tag::DynObj as u16 {
        return ptr;
    }
    if t == Tag::Arr as u16 || t == Tag::Closure as u16 {
        // Both keep the expando dict in the same +24 slot.
        return unsafe { closure_props(ptr) };
    }
    // Rotation 354 — promise bag at +32 (the +24 slot is the
    // callback list).
    if t == Tag::Promise as u16 {
        return unsafe { crate::member_get::promise_props(ptr) };
    }
    // Blade 2 (RFC 20260714-struct-dynamic-props) — a struct cell
    // carries the same +24 expando slot; a symbol-keyed expando
    // (`(c as any)[sym] = v`) lives there like any other key.
    if t == Tag::Obj as u16 {
        return unsafe { crate::member_get_layout::struct_props(ptr) };
    }
    if is_wrapper_tag(t) {
        return unsafe { wrapper_props(ptr) };
    }
    core::ptr::null()
}

/// What the receiver INHERITS a symbol key from, as a borrowed cell
/// box: an explicit user [[Prototype]] for a DynObj, else the builtin
/// prototype singleton's own expando dict, which is where a
/// `Object.defineProperty(Array.prototype, sym, …)` monkey-patch
/// lands.
///
/// # Safety
/// `ptr` is a live cell whose `type_tag` is `t`.
unsafe fn inherited_dict(ptr: *mut c_void, t: u16) -> InheritedFrom {
    if t == Tag::DynObj as u16 {
        return match unsafe { user_proto_cell(ptr) } {
            Some(parent) => InheritedFrom::Receiver(parent),
            None => InheritedFrom::Nothing,
        };
    }
    // RFC 20260802-class-computed-member 刀 3a — a struct instance
    // inherits symbol keys through its class prototype chain: a
    // runtime-computed `[Symbol(...)]()` member lands on
    // `__proto_<C>` as a symbol-keyed own entry, which the recursive
    // DynObj probe above then answers (grandparents via
    // `user_proto_cell`). The raw table read is a borrow (the
    // registry slot is process-lifetime); 0 = unregistered tag.
    if t == Tag::Obj as u16 {
        let class_tag = unsafe { ptr.cast::<u8>().add(8).cast::<u32>().read() };
        let root = unsafe { __torajs_proto_cell_raw(class_tag as i64) };
        if crate::nanbox::is_cell(root) {
            return InheritedFrom::Receiver(root);
        }
        return InheritedFrom::Nothing;
    }
    let proto_props = if t == Tag::Arr as u16 {
        array_proto_props()
    } else if t == Tag::Closure as u16 {
        function_proto_props()
    } else if crate::member_get_layout::is_wrapper_tag(t) {
        // A wrapper receiver inherits through its primitive's
        // prototype singleton — where `Object.defineProperty(
        // Boolean.prototype, sym, …)` lands (test262 concat's
        // iterable-primitive-wrapper-objects face).
        crate::member_get_own::wrapper_proto_props(t)
    } else {
        core::ptr::null()
    };
    if proto_props.is_null() {
        InheritedFrom::Nothing
    } else {
        InheritedFrom::Dict(proto_props)
    }
}

/// Where [`symbol_key_pair`] looks next on an own miss.
enum InheritedFrom {
    /// Another receiver — recurse (covers grandparents).
    Receiver(AnyValue),
    /// A prototype's property dict — one more probe, no recursion
    /// (the builtin prototypes are chain roots).
    Dict(*const c_void),
    Nothing,
}

/// Probe one dict for `key`. `Some(pair)` when the key is present,
/// including when it is present but stores `undefined`; `None` on a
/// genuine miss so the caller keeps walking.
///
/// # Safety
/// `dict` is NULL or a live DynObj; `key` is a live Symbol cell.
unsafe fn probe_dict(dict: *const c_void, key: *const c_void) -> Option<(u64, u64)> {
    if dict.is_null() {
        return None;
    }
    let tag = unsafe { __torajs_dynobj_get_tag(dict, key) };
    if tag != TAG_UNDEF {
        return Some((tag, unsafe { __torajs_dynobj_get_value(dict, key) }));
    }
    // An own entry STORING undefined is present, and shadows whatever
    // the chain would have answered.
    if unsafe { __torajs_dynobj_has(dict, key) } != 0 {
        return Some((TAG_UNDEF, 0));
    }
    None
}

/// `(tag, value)` for a symbol-keyed read — `(5, 0)` when absent.
/// Borrow-shaped like the string-key pair helpers: no rc traffic, the
/// dict keeps its own share of the value.
///
/// # Safety
/// Cell receivers are live heap pointers; `key` is a live Symbol cell.
pub(crate) unsafe fn symbol_key_pair(recv: AnyValue, key: *const c_void) -> (u64, u64) {
    // A primitive receiver boxes to a wrapper with no own symbol-keyed
    // property, and tr's builtin prototypes carry none either — the
    // only inherited symbol face a primitive CAN see is the F0
    // builtin `@@iterator` reify. A ShortStr is not a cell, so it
    // consults the reify table directly under its Str family.
    let Some((ptr, t)) = recv_cell(recv) else {
        if crate::nanbox::is_short_str(recv)
            && let Some(cell) =
                unsafe { crate::method_value::builtin_symbol_method_lookup(Tag::Str as u16, key) }
        {
            return (4, cell as u64);
        }
        return (TAG_UNDEF, 0);
    };
    if let Some(pair) = unsafe { probe_dict(own_dict(ptr, t), key) } {
        return pair;
    }
    match unsafe { inherited_dict(ptr, t) } {
        InheritedFrom::Receiver(parent) => return unsafe { symbol_key_pair(parent, key) },
        InheritedFrom::Dict(dict) => {
            if let Some(pair) = unsafe { probe_dict(dict, key) } {
                return pair;
            }
        }
        InheritedFrom::Nothing => {}
    }
    // RFC 20260728-gen-forof-yieldstar F0 — a native iterable tag's
    // `[Symbol.iterator]` reifies its aliased prototype method (a
    // monkey-patch in the dicts above shadows it, per the probes'
    // order). Tag 4 = Heap; the cell is immortal, borrow-shaped.
    // SAFETY: key is a live Symbol cell per the caller contract.
    if let Some(cell) = unsafe { crate::method_value::builtin_symbol_method_lookup(t, key) } {
        return (4, cell as u64);
    }
    (TAG_UNDEF, 0)
}
