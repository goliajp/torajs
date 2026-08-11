//! OrdinaryToPrimitive (ES §7.1.1.1) for heap-object Any operands.
//!
//! `ToString(obj)` / `ToNumber(obj)` must run the object's own (or
//! monkey-patched) `toString` / `valueOf` in hint order and accept
//! the FIRST primitive result — where "primitive" includes
//! `undefined` and `null` (`String({toString(){}})` is
//! `"undefined"`, not `"[object Object]"`). Only when both methods
//! answer objects does the coercion throw a catchable TypeError
//! (test262 trim 15.5.4.20-2-42 asserts both get called).
//!
//! Dispatch reuses [`crate::method_call::any_method_call_inner`] —
//! own/patched entries win, and the inherited Object.prototype
//! surface (`valueOf` → receiver, `toString` → "[object Object]")
//! provides the spec default when the receiver has neither.
//! RFC 20260712-string-proto-cluster chunk C.

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF, HeapHeader, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, box_pointer, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-str — §6.1.5.1 well-known singleton table; idx 12 is
    /// `@@toPrimitive` (alphabetical property-name order). Owned +1.
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-rc — universal heap-header decrement (for the symbol).
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
}

/// `WELL_KNOWN_DESCS` index of `Symbol.toPrimitive`.
#[cfg(not(test))]
const WK_TO_PRIMITIVE: i64 = 12;

/// `AnySlotTag::Undef` / `AnySlotTag::Null` — the §7.3.11 GetMethod
/// step-3 pair that means "no hook".
#[cfg(not(test))]
const TAG_UNDEF: u64 = 5;
#[cfg(not(test))]
const TAG_NULL: u64 = 0;

/// Method-dispatch indirection: the real
/// [`crate::method_call::any_method_call_inner`] reaches the whole
/// per-tag dispatch universe (dynobj / date / map / str-any
/// helpers), whose extern symbols only exist in the shipped
/// staticlib link. The unit-test binary must not pull that graph
/// past `-dead_strip` (coerce.rs is test-reachable), so tests get a
/// no-hook stub — the inherited-prototype default is exercised by
/// conformance, not unit tests.
#[cfg(not(test))]
#[inline]
unsafe fn dispatch_method(recv: AnyValue, mid: i64, name_str: *const u8) -> AnyValue {
    unsafe {
        crate::method_call::any_method_call_inner(
            recv,
            mid,
            name_str,
            core::ptr::null_mut(),
            core::ptr::null(),
            0,
        )
    }
}

#[cfg(test)]
#[inline]
unsafe fn dispatch_method(_recv: AnyValue, _mid: i64, _name_str: *const u8) -> AnyValue {
    crate::nanbox::VALUE_UNDEFINED
}

/// §7.1.1.1 IsCallable probe — an own struct/dynobj entry holding a
/// non-callable value makes OrdinaryToPrimitive skip that method
/// name (`{toString: void 0, valueOf: fn}` coerces through valueOf;
/// pre-fix the dispatch's not-callable TypeError was mistaken for a
/// user throw and propagated — test262 search A1_T9).
#[cfg(not(test))]
unsafe fn skip_not_callable(cell: *mut c_void, name_str: *const u8) -> bool {
    let h = unsafe { &*(cell as *const HeapHeader) };
    match h.tag() {
        Tag::Obj => unsafe {
            crate::method_call_dynobj::own_entry_not_callable(cell, true, name_str)
        },
        Tag::DynObj => unsafe {
            crate::method_call_dynobj::own_entry_not_callable(cell, false, name_str)
        },
        // An Arr's monkey-patches land in the side-props expando —
        // `arr.toString = undefined` shadows the builtin the same way
        // (RFC 20260717 own-undefined-shadow family, arr leg).
        Tag::Arr => unsafe {
            crate::method_call_dynobj::arr_own_entry_not_callable(cell, name_str)
        },
        _ => false,
    }
}

#[cfg(test)]
unsafe fn skip_not_callable(_cell: *mut c_void, _name_str: *const u8) -> bool {
    false
}

/// A NaN-boxed value is an "object" for ToPrimitive purposes iff it
/// is a heap cell whose tag is not a primitive flavor — Str cells /
/// ShortStr immediates are the string primitive, and BigInt / Symbol
/// cells are primitives too (§7.1.1.1 step 6.b accepts them; pre-fix
/// a `valueOf() { return 255n }` answer was discarded as "still an
/// object" and the walk fell through to toString). Every other
/// immediate is a primitive by construction.
#[inline]
pub(crate) fn is_object_value(v: AnyValue) -> bool {
    if !is_cell(v) {
        return false;
    }
    let h = unsafe { &*(as_void_ptr(v) as *const HeapHeader) };
    !matches!(h.tag(), Tag::Str | Tag::BigInt | Tag::Symbol)
}

/// What the §7.1.1 step-1 exotic `@@toPrimitive` probe resolved.
/// (The test stub only ever answers `NoHook` — see below.)
#[cfg_attr(test, allow(dead_code))]
enum Exotic {
    /// No usable hook — absent or nullish per §7.3.11 GetMethod
    /// step 3. The caller falls through to OrdinaryToPrimitive.
    NoHook,
    /// The hook answered — the caller returns this verbatim. `Some`
    /// carries a primitive result (or a pending-throw placeholder),
    /// `None` means the hook answered an object and the §7.1.1
    /// step 1.b.iii TypeError is already recorded.
    Answered(Option<AnyValue>),
}

/// §7.1.1 steps 1.a-1.b — `GetMethod(input, @@toPrimitive)` and, when
/// present, `Call(hook, input, [hint])` with the hint STRING the spec
/// hands a user hook (`"default"` / `"number"` / `"string"`). A hook
/// entry that is present but not callable is the GetMethod step-4
/// TypeError; a nullish one means "no hook" and OrdinaryToPrimitive
/// runs instead (`o[Symbol.toPrimitive] = null` coerces through
/// valueOf, unlike the @@iterator lane where nullish refuses).
///
/// tr's builtin prototypes carry no `@@toPrimitive` reify (the F0
/// table covers @@iterator / @@dispose / RegExp protocol only), so
/// the symbol walk only ever answers a property a user actually
/// wrote — Date's builtin §21.4.4.45 default→string mapping stays
/// the tag special-case in [`heap_to_primitive_default`].
#[cfg(not(test))]
unsafe fn exotic_to_primitive(cell: *mut c_void, hint: &[u8]) -> Exotic {
    unsafe {
        let sym = __torajs_symbol_well_known(WK_TO_PRIMITIVE);
        if sym.is_null() {
            return Exotic::NoHook;
        }
        let recv = box_pointer(cell as *mut HeapHeader);
        // Borrow-shaped pair walk — the receiver keeps its stake, so
        // the method needs no retain (iter_any_get_method's pattern).
        let (tag, payload) = crate::member_get_symbol::symbol_key_pair(recv, sym);
        let _ = __torajs_rc_dec(sym);
        if tag == TAG_UNDEF || tag == TAG_NULL {
            return Exotic::NoHook;
        }
        let entry = crate::iter_any_get_method::callable_entry(
            tag,
            payload,
            c"Symbol.toPrimitive is not a function",
        );
        let Some((env, entry)) = entry else {
            // Present but not callable — the TypeError is recorded;
            // a type-correct placeholder lets the caller unwind.
            return Exotic::Answered(Some(crate::nanbox::VALUE_UNDEFINED));
        };
        let hint_cell = __torajs_str_alloc(hint.as_ptr(), hint.len() as i64);
        let argv = [box_pointer(hint_cell as *mut HeapHeader)];
        let out = crate::method_call_closure_dispatch::invoke_with_this(
            env,
            entry,
            recv,
            argv.as_ptr(),
            1,
        );
        __torajs_str_drop(hint_cell as *mut c_void);
        if __torajs_throw_check() != 0 {
            // The hook threw — propagate through the placeholder.
            return Exotic::Answered(Some(out));
        }
        // §7.1.1 step 1.b.iii — an object answer refuses; there is
        // no OrdinaryToPrimitive fallback behind a hook that ran.
        if is_object_value(out) {
            __torajs_anyv_rc_dec(out);
            __torajs_throw_type_error(c"cannot convert object to primitive value".as_ptr());
            return Exotic::Answered(None);
        }
        Exotic::Answered(Some(out))
    }
}

/// The unit-test binary must not pull the symbol-walk / closure
/// dispatch graph past `-dead_strip` (same story as
/// [`dispatch_method`]) — tests exercise the ordinary path only.
#[cfg(test)]
unsafe fn exotic_to_primitive(_cell: *mut c_void, _hint: &[u8]) -> Exotic {
    Exotic::NoHook
}

/// `ToPrimitive(cell)` with the DEFAULT hint (§7.1.1 step 1.c): a
/// user `@@toPrimitive` receives `"default"`; without one, every
/// ordinary object treats default as number order, but a Date's
/// builtin `@@toPrimitive` maps default to string order
/// (§21.4.4.45) — `date == str` compares the toString form, not
/// the epoch millis. Loose-equality's object arm (§7.2.14 steps
/// 11-12) and the `+` operator are the consumers.
pub(crate) unsafe fn heap_to_primitive_default(cell: *mut c_void) -> Option<AnyValue> {
    match unsafe { exotic_to_primitive(cell, b"default") } {
        Exotic::Answered(r) => return r,
        Exotic::NoHook => {}
    }
    let h = unsafe { &*(cell as *const HeapHeader) };
    let hint_string = matches!(h.tag(), Tag::Date);
    unsafe { ordinary_to_primitive(cell, hint_string) }
}

/// `ToPrimitive(cell)` with hint number (`hint_string == false`) or
/// hint string — §7.1.1 step 1 probes a user `@@toPrimitive` first
/// (which receives the hint by name), then falls back to
/// OrdinaryToPrimitive. Returns the first primitive result as a
/// caller-owned boxed value; when the coercion refuses (both
/// ordinary methods answer objects, or the hook does), records a
/// catchable TypeError and returns `None`. A user method that
/// throws propagates immediately (pending-throw short circuit) —
/// the caller must answer a type-correct placeholder and let its
/// `emit_throw_check` unwind.
pub(crate) unsafe fn heap_to_primitive(cell: *mut c_void, hint_string: bool) -> Option<AnyValue> {
    let hint: &[u8] = if hint_string { b"string" } else { b"number" };
    match unsafe { exotic_to_primitive(cell, hint) } {
        Exotic::Answered(r) => return r,
        Exotic::NoHook => {}
    }
    unsafe { ordinary_to_primitive(cell, hint_string) }
}

/// OrdinaryToPrimitive (§7.1.1.1) over a heap cell. `hint_string`
/// picks the method order (`toString` → `valueOf` for hint string,
/// reversed for hint number). Same return protocol as
/// [`heap_to_primitive`].
unsafe fn ordinary_to_primitive(cell: *mut c_void, hint_string: bool) -> Option<AnyValue> {
    let recv = box_pointer(cell as *mut HeapHeader);
    let order: [(i64, &[u8]); 2] = if hint_string {
        [
            (ANY_METHOD_TO_STRING, b"toString"),
            (ANY_METHOD_VALUE_OF, b"valueOf"),
        ]
    } else {
        [
            (ANY_METHOD_VALUE_OF, b"valueOf"),
            (ANY_METHOD_TO_STRING, b"toString"),
        ]
    };
    for (mid, name) in order {
        let key = unsafe { __torajs_str_alloc(name.as_ptr(), name.len() as i64) };
        if unsafe { skip_not_callable(cell, key) } {
            unsafe { __torajs_str_drop(key as *mut c_void) };
            continue;
        }
        let out = unsafe { dispatch_method(recv, mid, key) };
        unsafe { __torajs_str_drop(key as *mut c_void) };
        if unsafe { __torajs_throw_check() } != 0 {
            // The user method threw — propagate by returning a
            // primitive placeholder; the pending throw unwinds at
            // the caller's next check point.
            return Some(out);
        }
        if !is_object_value(out) {
            return Some(out);
        }
        unsafe { __torajs_anyv_rc_dec(out) };
    }
    unsafe {
        __torajs_throw_type_error(c"cannot convert object to primitive value".as_ptr());
    }
    None
}
