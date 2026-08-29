//! Cell-receiver arm of the `any` method dispatcher — the tag
//! ladder split out of [`crate::method_call::any_method_call_inner`]
//! (rotation 147, file-size fn debt).
//!
//! Returns `None` when no tag matched, so the caller keeps the
//! shared no-such-method TypeError exit: that throw is also the
//! landing spot for non-cell receivers, so the fall-through must
//! stay in the caller rather than be duplicated here.

use torajs_rc::{
    __torajs_rc_inc, ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF,
    FLAG_SUBCLASSED, Tag,
};

use core::ffi::c_void;

use crate::method_call::any_method_call_inner;
use crate::nanbox::{AnyValue, as_void_ptr};

unsafe extern "C" {
    /// torajs-str — §20.4.3.3 SymbolDescriptiveString (owned Str
    /// out, rc=1).
    fn __torajs_symbol_to_str(p: *const c_void) -> *mut u8;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// §20.4.3.3 / §20.4.3.4 — the reified `Symbol.prototype.toString` /
/// `valueOf` body: thisSymbolValue over the receiver. A Symbol cell
/// (or a SymbolWrapper, unwrapped to its [[SymbolData]]) answers its
/// descriptive string / the symbol (fresh +1 per the boxed-value
/// convention); every other receiver — number, string, plain object,
/// `Symbol.prototype` itself (an ordinary object with no
/// [[SymbolData]]) — is the spec TypeError.
pub(crate) unsafe fn symbol_proto_method(recv: AnyValue, mid: i64) -> AnyValue {
    unsafe {
        if let Some(ptr) = this_symbol_value(recv) {
            if mid == torajs_rc::ANY_METHOD_SYMBOL_VALUE_OF {
                __torajs_rc_inc(ptr);
                return crate::nanbox_encode::__torajs_anyv_box_pointer(ptr as *mut c_void);
            }
            let s = __torajs_symbol_to_str(ptr);
            return crate::nanbox_encode::__torajs_anyv_box_pointer(s as *mut c_void);
        }
        __torajs_throw_type_error(c"Symbol.prototype requires that |this| be a Symbol".as_ptr());
        crate::nanbox::VALUE_UNDEFINED
    }
}

/// §20.4.3.3 Symbol.prototype.toString — the SymbolDescriptiveString
/// ("Symbol(<desc>)"); toLocaleString is the inherited §20.1.4.6
/// invoke-this.toString. valueOf (§20.4.3.4 thisSymbolValue) already
/// answered identity through `cell_method`'s cell-wide arm. Every
/// other name is a miss.
///
/// # Safety
/// `ptr` must be a valid Symbol heap cell.
pub(crate) unsafe fn symbol_string_method(ptr: *mut c_void, mid: i64) -> AnyValue {
    unsafe {
        if mid == ANY_METHOD_TO_STRING || mid == ANY_METHOD_TO_LOCALE_STRING {
            let s = __torajs_symbol_to_str(ptr);
            return crate::nanbox_encode::__torajs_anyv_box_pointer(s as *mut c_void);
        }
        crate::method_call::method_no_such()
    }
}

/// §20.4.3 thisSymbolValue — a Symbol cell answers itself; a
/// SymbolWrapper answers its `[[SymbolData]]` inner cell; every other
/// receiver is `None` (the caller throws the spec TypeError). The
/// returned pointer is a borrow.
pub(crate) unsafe fn this_symbol_value(recv: AnyValue) -> Option<*mut c_void> {
    unsafe {
        if !crate::nanbox::is_cell(recv) {
            return None;
        }
        let ptr = as_void_ptr(recv);
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Symbol as u16 {
            return Some(ptr);
        }
        if tag == Tag::SymbolWrapper as u16 {
            return Some((ptr.cast::<u8>().add(8) as *const *mut c_void).read());
        }
        None
    }
}

/// §20.4.3.2 — the reified `get Symbol.prototype.description` body:
/// thisSymbolValue over the receiver, then the [[Description]] Str
/// (fresh +1 per the boxed-value convention; `Symbol()` answers
/// undefined). Every non-Symbol receiver is the spec TypeError.
pub(crate) unsafe fn symbol_description_getter(recv: AnyValue) -> AnyValue {
    unsafe {
        if let Some(ptr) = this_symbol_value(recv) {
            let desc = crate::member_get_layout::symbol_desc(ptr);
            if desc.is_null() {
                return crate::nanbox::VALUE_UNDEFINED;
            }
            __torajs_rc_inc(desc);
            return crate::nanbox_encode::__torajs_anyv_box_pointer(desc);
        }
        __torajs_throw_type_error(c"Symbol.prototype requires that |this| be a Symbol".as_ptr());
        crate::nanbox::VALUE_UNDEFINED
    }
}

/// RFC 20260730 blade 2 — exotic-subclass method probe for the tags
/// whose arms carry no own-expando shadow of their own (wrappers'
/// expando ran just above; Map/Set have none): on the spec chain
/// C.prototype sits between own properties and the builtin
/// prototype, so a class method (including an override of a builtin
/// name) resolves here, before the view-through / valueOf-identity /
/// per-tag surfaces. Arr keeps its own probe inside its arm (its
/// expando shadow lives there). Plain receivers pay one
/// predicted-clear branch on an already-loaded header word.
///
/// # Safety
/// `ptr` is a live heap cell whose header carries `tag`; `name_str`
/// is NULL or a live Str cell; `argv`/`argc` follow the boxed-adapter
/// convention.
unsafe fn wrapper_subclass_probe(
    ptr: *mut c_void,
    tag: u16,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    let probed = crate::member_get::is_wrapper_tag(tag)
        || tag == Tag::Map as u16
        || tag == Tag::Set as u16
        || tag == Tag::Promise as u16
        || tag == Tag::RegExp as u16
        // Rotation 373 — the weak collections and Date joined the
        // exotic-subclass table.
        || tag == Tag::WeakMap as u16
        || tag == Tag::WeakSet as u16
        || tag == Tag::Date as u16
        // Buffer-family blade — the TypedArray kinds joined the
        // exotic-subclass table.
        || tag == Tag::TypedArray as u16;
    if !probed || name_str.is_null() {
        return None;
    }
    let flags = unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() };
    if flags & FLAG_SUBCLASSED == 0 {
        return None;
    }
    unsafe { crate::method_call_subclass::subclass_method(ptr, name_str, argv, argc) }
}

/// [`cell_method`] plus the `Object.prototype` surface every cell
/// inherits once no per-tag arm claimed the mid. §20.1.3.6 toString
/// is the "[object X]" badge and §20.1.4.6 toLocaleString invokes
/// this.toString, so a Map / Set / Promise receiver — whose arms own
/// neither name — answers a badge, not a no-such TypeError. The
/// quieter half: a mid-miss floats the [`ANY_METHOD_NO_SUCH`]
/// sentinel, and OrdinaryToPrimitive used to accept it as toString's
/// answer. Its bit pattern is a quiet NaN, so `String(mapThroughAny)`
/// coerced to "NaN" — a wrong value with nothing to catch.
///
/// The Promise arm floats the sentinel rather than falling past the
/// remaining tag arms for exactly this reason; none of the arms
/// below it could match a Promise cell anyway.
///
/// # Safety
/// Same contract as [`cell_method`].
pub(crate) unsafe fn cell_method_inheriting(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
    skip_wrapper_expando: bool,
) -> Option<AnyValue> {
    let out = unsafe {
        cell_method(
            recv,
            mid,
            name_str,
            recv_slot,
            argv,
            argc,
            skip_wrapper_expando,
        )
    }?;
    if out == crate::method_call::ANY_METHOD_NO_SUCH
        && (mid == ANY_METHOD_TO_STRING || mid == ANY_METHOD_TO_LOCALE_STRING)
    {
        return Some(unsafe {
            crate::method_call_object_proto::cell_badge_string(as_void_ptr(recv))
        });
    }
    Some(out)
}

/// Dispatch a cell receiver by its heap tag. `None` = no arm
/// matched; the caller raises the no-such-method TypeError.
/// `skip_wrapper_expando` = the call is a reified-builtin cell's
/// re-dispatch (method body execution) — own-property probing is
/// over, so the expando surface must not resolve again (a same-mid
/// same-name entry would recurse forever).
pub(crate) unsafe fn cell_method(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
    skip_wrapper_expando: bool,
) -> Option<AnyValue> {
    let ptr = as_void_ptr(recv);
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    // ES own-property order — a wrapper EXPANDO entry wins over
    // the view-through surface (`__instance.charAt =
    // String.prototype.charAt` stores the reified cell; the call
    // must run it against the wrapper receiver — the §22.1.3
    // generic ToString(this) coerce — not fall to the inner
    // primitive's arm, which knows no such method).
    if !skip_wrapper_expando
        && crate::member_get::is_wrapper_tag(tag)
        && let Some(out) = unsafe {
            crate::method_call_wrapper_expando::wrapper_expando_method(
                recv, ptr, mid, name_str, argv, argc,
            )
        }
    {
        return Some(out);
    }
    // A reified builtin's re-dispatch resolved to the BUILTIN entry
    // already — re-probing a same-name override would loop.
    if !skip_wrapper_expando
        && let Some(r) = unsafe { wrapper_subclass_probe(ptr, tag, name_str, argv, argc) }
    {
        return Some(r);
    }
    if let Some(out) = unsafe { try_wrapper_arraylike_arm(ptr, tag, mid, name_str, argv, argc) } {
        return Some(out);
    }
    // RFC 20260716 刀 3 view-through (`wrapper_view_through`).
    if let Some(inner) = unsafe { crate::wrapper_view_through::resolve_inner_recv(ptr, tag) } {
        return Some(unsafe { any_method_call_inner(inner, mid, name_str, recv_slot, argv, argc) });
    }
    // §20.1.4.7 Object.prototype.valueOf is ToObject(this) —
    // identity on every cell receiver. Date keeps its own
    // valueOf (the getTime alias in its per-tag arm), the
    // Number/Boolean immediates answered above, and the
    // property-carrying shapes (dynobj / struct) resolve their
    // OWN entry first — their arms fall back here-equivalent
    // via `object_proto_fallback` so a monkey-patch wins.
    if mid == ANY_METHOD_VALUE_OF
        && tag != Tag::Date as u16
        && tag != Tag::DynObj as u16
        && tag != Tag::Obj as u16
        && tag != Tag::Arr as u16
    {
        unsafe { __torajs_rc_inc(ptr) };
        return Some(recv);
    }
    // §20.1.4.6 Object.prototype.toLocaleString invokes
    // this.toString — Str is identity, Arr delegates to the
    // §23.1.3.32 join-with-"," shape; plain objects answer
    // through their arm's own-probe-then-fallback (monkey-patch
    // order). Date / Number keep their own per-arm
    // specializations; other tags fall through to their arm (a
    // miss stays the no-such TypeError).
    if mid == ANY_METHOD_TO_LOCALE_STRING {
        if tag == Tag::Str as u16 {
            unsafe { __torajs_rc_inc(ptr) };
            return Some(recv);
        }
        if tag == Tag::Arr as u16 {
            // §23.1.3.32 — per-element toLocaleString invoke
            // (a plain join never dispatched the element hook).
            return Some(unsafe { crate::arr_locale_string::arr_to_locale_string(ptr) });
        }
    }
    if tag == Tag::Str as u16 {
        // toString is identity — hand the caller its own +1 on
        // the same cell per the boxed-value convention.
        if mid == ANY_METHOD_TO_STRING {
            unsafe { __torajs_rc_inc(ptr) };
            return Some(recv);
        }
        return Some(unsafe {
            crate::dispatch_seam::__torajs_dispatch_str_arm(
                recv, mid, name_str, recv_slot, argv, argc,
            )
        });
    }
    if tag == Tag::Arr as u16 {
        // §10.1.8.1 OrdinaryGet — an own arr-props expando entry
        // shadows the built-in method surface (`arr.getClass =
        // Object.prototype.toString; arr.getClass()` / a user
        // closure stored on the array). One props-NULL check on
        // the no-expando fast path (RFC 20260713 blade 2).
        if !name_str.is_null()
            && let Some(r) = unsafe {
                crate::method_call_dynobj::arr_expando_method(
                    ptr, recv, name_str, recv_slot, argv, argc,
                )
            }
        {
            return Some(r);
        }
        // RFC 20260730 blade 1 — subclass method probe: on the spec
        // chain C.prototype sits between own properties and
        // Array.prototype, so a class method (including an override
        // of a builtin name) resolves here, after the expando shadow
        // and before the builtin surface. Plain arrays pay one
        // predicted-clear branch on an already-loaded header word.
        if !name_str.is_null() {
            let flags = unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() };
            if flags & FLAG_SUBCLASSED != 0
                && let Some(r) = unsafe {
                    crate::method_call_subclass::subclass_method(ptr, name_str, argv, argc)
                }
            {
                return Some(r);
            }
        }
        // §20.1.4.7 Object.prototype.valueOf identity — moved
        // after the expando probe so a patched `arr.valueOf`
        // wins (the early cell-wide identity arm excludes Arr
        // for exactly this monkey-patch order).
        if mid == ANY_METHOD_VALUE_OF {
            unsafe { __torajs_rc_inc(ptr) };
            return Some(recv);
        }
        return Some(unsafe {
            crate::dispatch_seam::__torajs_dispatch_arr_arm(
                recv, mid, name_str, recv_slot, argv, argc,
            )
        });
    }
    if tag == Tag::DynObj as u16 {
        return Some(unsafe {
            crate::dispatch_seam::__torajs_dispatch_dynobj_arm(
                recv, mid, name_str, recv_slot, argv, argc,
            )
        });
    }
    // L3b #9 (chunk 524) — static-layout struct receivers probe
    // the class-layouts field metadata instead of a dynobj table.
    if tag == Tag::Obj as u16 {
        // 刀 8 G2a — the reified-cell call/apply re-dispatch (NULL
        // name: the mid is authoritative) routes an array-family mid
        // over a struct receiver through the ES generic array-like
        // arm, same gate as the dynobj arm; interior writes go
        // through the member-set dispatcher, where growth rejects
        // loud (the G10 struct-dynamic-props posture).
        if name_str.is_null() && crate::method_call_arraylike_concat::obj_supported(mid) {
            return Some(unsafe {
                crate::method_call_arraylike_concat::obj_method(ptr, mid, recv_slot, argv, argc)
            });
        }
        return Some(unsafe {
            crate::dispatch_seam::__torajs_dispatch_struct_arm(
                recv, mid, name_str, recv_slot, argv, argc,
            )
        });
    }
    // Every remaining tag routes straight through its family's
    // seam arm with no pre-arm gate — one shared table keeps the
    // ladder flat. Per-family notes: iterator family = MapIter /
    // ArrIter (lazy-helper chaining first, RFC
    // 20260730-iterator-global 刀 2) + IterHelper, chain-first
    // posture in the arm impl; buffer family = ArrayBuffer §25.1.6 /
    // TypedArray §23.2.3 (species channel §23.2.4.1 first) /
    // DataView §25.3.4, mid-misses float ANY_METHOD_NO_SUCH;
    // Promise = the `.then`/`.catch` bridge (RFC
    // 20260720-anylane-promise-methods 刀 2); BigInt §21.2.3
    // (valueOf is the cell-wide identity above); Closure =
    // Function.prototype.call / apply (chunk 710); weak family =
    // WeakMap / WeakSet (RC-2b) + WeakRef (rotation 314).
    if let Some(arm) = seam_arm_for_tag(tag) {
        return Some(unsafe { arm(recv, mid, name_str, recv_slot, argv, argc) });
    }
    None
}

/// C-ABI shape shared by every dispatch arm seam.
type SeamArm =
    unsafe extern "C" fn(AnyValue, i64, *const u8, *mut u64, *const u64, i64) -> AnyValue;

/// The gate-free half of the tag ladder — tags whose whole arm IS
/// the family seam call. Tags with pre-arm gates (Str identity, Arr
/// expando/subclass, DynObj, struct array-like) stay as explicit
/// ladder arms in [`cell_method`].
fn seam_arm_for_tag(tag: u16) -> Option<SeamArm> {
    use crate::dispatch_seam as seam;
    let t = |x: Tag| x as u16;
    Some(match tag {
        x if x == t(Tag::Map) || x == t(Tag::Set) => seam::__torajs_dispatch_mapset_arm,
        x if x == t(Tag::MapIter) || x == t(Tag::ArrIter) || x == t(Tag::IterHelper) => {
            seam::__torajs_dispatch_iter_arm
        }
        x if x == t(Tag::ArrayBuffer) || x == t(Tag::TypedArray) || x == t(Tag::DataView) => {
            seam::__torajs_dispatch_buffer_arm
        }
        x if x == t(Tag::Date) => seam::__torajs_dispatch_date_arm,
        x if x == t(Tag::Promise) => seam::__torajs_dispatch_promise_arm,
        x if x == t(Tag::RegExp) => seam::__torajs_dispatch_regexp_arm,
        x if x == t(Tag::BigInt) => seam::__torajs_dispatch_bigint_arm,
        x if x == t(Tag::Symbol) => seam::__torajs_dispatch_symbol_arm,
        x if x == t(Tag::Closure) => seam::__torajs_dispatch_closure_arm,
        x if x == t(Tag::WeakMap) || x == t(Tag::WeakSet) || x == t(Tag::WeakRef) => {
            seam::__torajs_dispatch_weak_arm
        }
        _ => return None,
    })
}

/// 刀 9 G2c — a reified-cell re-dispatch (NULL name) of a non-mutating
/// array-family mid over a Number/Boolean wrapper receiver runs the ES
/// generic array-like semantics on the wrapper's OWN face
/// (`obj.length = 2; obj[1] = true` on `new Boolean(false)` lives in the
/// `+16` expando; the view-through below would only reach the inner
/// primitive's wrapper-PROTO surface). StringWrapper stays on
/// view-through wholesale (its inner Str face owns the shared mids —
/// slice / indexOf / toString are STRING semantics there), and toString
/// keeps the primitive arm (a stored builtin toString cell must answer
/// the inner value, not an array join — the wrapper-expando-builtin-cell
/// family). Mutators stay on the primitive arm's exclusion (recorded
/// boundary — the mut family's set_at writes raw dynobj layout).
///
/// # Safety
/// Same contract as `cell_method`: `ptr` is a live cell whose tag equals
/// the `tag` argument; `argv`/`argc` describe a valid arg slot span.
unsafe fn try_wrapper_arraylike_arm(
    ptr: *mut c_void,
    tag: u16,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    if name_str.is_null()
        && (tag == Tag::NumberWrapper as u16 || tag == Tag::BooleanWrapper as u16)
        && mid != ANY_METHOD_TO_STRING
        && crate::method_call_arraylike_concat::obj_supported(mid)
        && !crate::method_call_arraylike_mut::arraylike_mut_supported(mid)
    {
        return Some(unsafe {
            crate::method_call_arraylike_concat::obj_method(
                ptr,
                mid,
                core::ptr::null_mut(),
                argv,
                argc,
            )
        });
    }
    None
}
