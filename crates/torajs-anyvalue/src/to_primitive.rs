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
//! Each method name is §7.1.1.1's literal shape: `Get(O, name)`
//! through the one receiver-aware [[Get]] (own entries, accessors,
//! Proxy traps, the prototype chain, the builtin cell a prototype
//! carries), IsCallable, then `Call` with `O` as this (RFC
//! 20260922-to-primitive-get-call S4). A borrowed root toString /
//! valueOf answers through the direct table, not the method dispatch.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use torajs_rc::{HeapHeader, Tag};

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

/// §7.1.1.1 steps 5.a-5.b for one method name: `Get(O, name)` —
/// own entry, accessor getter, Proxy trap, prototype chain, the
/// builtin cell a prototype carries, all through the one [[Get]] —
/// then IsCallable, then `Call(method, O)`. `None` is "not callable,
/// try the next name"; `Some(out)` is the method's answer, or the
/// placeholder under a pending throw (from the Get or the call) the
/// caller's check unwinds.
#[cfg(not(test))]
unsafe fn get_and_call(recv: AnyValue, key: *const u8) -> Option<AnyValue> {
    unsafe {
        let key = key as *const c_void;
        let f = crate::member_get_receiver::__torajs_any_member_get_with_receiver(recv, key, recv);
        if __torajs_throw_check() != 0 {
            return Some(f);
        }
        if !crate::promise_capability::is_callable(f) {
            __torajs_anyv_rc_dec(f);
            return None;
        }
        let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            f,
            recv,
            core::ptr::null(),
            0,
        );
        __torajs_anyv_rc_dec(f);
        Some(out)
    }
}

/// The unit-test binary must not pull the [[Get]] / [[Call]] graph
/// past `-dead_strip` (coerce.rs is test-reachable, and that graph's
/// extern symbols exist only in the shipped staticlib link), so tests
/// get a method that answers undefined — the inherited-prototype walk
/// is exercised by conformance, not unit tests.
#[cfg(test)]
unsafe fn get_and_call(_recv: AnyValue, _key: *const u8) -> Option<AnyValue> {
    Some(crate::nanbox::VALUE_UNDEFINED)
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
/// The builtin hooks — §20.4.3.5 on Symbol.prototype and §21.4.4.45
/// on Date.prototype — are real entries on those prototypes
/// (`closure_proto::to_primitive_proto`), so the walk finds them, or
/// whatever a program redefined or deleted them to.
#[cfg(not(test))]
unsafe fn exotic_to_primitive(cell: *mut c_void, hint: &[u8]) -> Exotic {
    unsafe {
        let sym = __torajs_symbol_well_known(WK_TO_PRIMITIVE);
        if sym.is_null() {
            return Exotic::NoHook;
        }
        let recv = box_pointer(cell as *mut HeapHeader);
        // GetMethod is a real Get, so an ACCESSOR-shaped `@@toPrimitive`
        // runs its getter. The borrow-shaped pair answers the sentinel
        // instead, and this lane is the RFC's B shape: the sentinel is
        // neither undefined nor null, so it slipped past the nullish
        // gate and reached `callable_method`, which boxed it into
        // something no closure lookup recognises — every accessor form
        // threw "Symbol.toPrimitive is not a function" with the getter
        // never running, `throw` inside the getter included.
        let hook_pair = crate::member_get_symbol::symbol_key_get(recv, sym);
        let (tag, payload) = hook_pair.pair();
        let _ = __torajs_rc_dec(sym);
        // A getter that threw answers undefined with the throw pending;
        // falling through to OrdinaryToPrimitive would swallow it.
        if __torajs_throw_check() != 0 {
            return Exotic::Answered(Some(crate::nanbox::VALUE_UNDEFINED));
        }
        if tag == TAG_UNDEF || tag == TAG_NULL {
            // Nullish carries no cell, so the guard drops to nothing.
            return Exotic::NoHook;
        }
        // A getter-produced hook is ours; `env` below aliases its cell,
        // so `hook_pair` has to outlive the call — which, held as a
        // guard, it does by construction.
        let hook = crate::iter_any_get_method::callable_method(
            tag,
            payload,
            recv,
            c"Symbol.toPrimitive is not a function",
        );
        let Some(hook) = hook else {
            // Present but not callable — the TypeError is recorded;
            // a type-correct placeholder lets the caller unwind.
            return Exotic::Answered(Some(crate::nanbox::VALUE_UNDEFINED));
        };
        let hint_cell = __torajs_str_alloc(hint.as_ptr(), hint.len() as i64);
        let argv = [box_pointer(hint_cell as *mut HeapHeader)];
        let out = crate::method_call_closure_dispatch::invoke_with_this(
            hook.env(),
            hook.entry(),
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

/// `ToPrimitive(cell)` with the DEFAULT hint (§7.1.1 step 1.c): an
/// `@@toPrimitive` receives `"default"` (a Date's builtin one maps it
/// to string order, §21.4.4.45); without one, default is number
/// order. Loose-equality's object arm (§7.2.14 steps 11-12) and the
/// `+` operator are the consumers.
pub(crate) unsafe fn heap_to_primitive_default(cell: *mut c_void) -> Option<AnyValue> {
    match unsafe { exotic_to_primitive(cell, b"default") } {
        Exotic::Answered(r) => return r,
        Exotic::NoHook => {}
    }
    unsafe { ordinary_to_primitive(cell, false) }
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

/// The two method-name keys, minted once and never dropped — every
/// coercion of a heap value used to allocate and free both (RFC
/// 20260922-to-primitive-get-call S0). The callees below only borrow
/// the key; the static's +1 keeps the cell alive for the process.
static NAME_TO_STRING: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static NAME_VALUE_OF: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

/// The immortal Str cell spelling `name`, minted on first use.
unsafe fn name_key(slot: &AtomicPtr<u8>, name: &[u8]) -> *mut u8 {
    let key = slot.load(Ordering::Acquire);
    if !key.is_null() {
        return key;
    }
    let fresh = unsafe { __torajs_str_alloc(name.as_ptr(), name.len() as i64) };
    match slot.compare_exchange(
        core::ptr::null_mut(),
        fresh,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => fresh,
        Err(winner) => {
            unsafe { __torajs_str_drop(fresh as *mut c_void) };
            winner
        }
    }
}

/// OrdinaryToPrimitive (§7.1.1.1) over a heap cell. `hint_string`
/// picks the method order (`toString` → `valueOf` for hint string,
/// reversed for hint number). Same return protocol as
/// [`heap_to_primitive`].
pub(crate) unsafe fn ordinary_to_primitive(
    cell: *mut c_void,
    hint_string: bool,
) -> Option<AnyValue> {
    let recv = box_pointer(cell as *mut HeapHeader);
    let to_string = (&NAME_TO_STRING, &b"toString"[..]);
    let value_of = (&NAME_VALUE_OF, &b"valueOf"[..]);
    let order = if hint_string {
        [to_string, value_of]
    } else {
        [value_of, to_string]
    };
    for (slot, name) in order {
        let key = unsafe { name_key(slot, name) };
        let Some(out) = (unsafe { get_and_call(recv, key) }) else {
            continue;
        };
        if unsafe { __torajs_throw_check() } != 0 {
            // The Get or the method threw — propagate by returning a
            // placeholder; the pending throw unwinds at the caller's
            // next check point.
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
