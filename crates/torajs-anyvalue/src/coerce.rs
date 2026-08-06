//! `ToString` (ES §7.1.17) and `ToNumber` (ES §7.1.4) coercions for
//! Any-tagged operands.
//!
//! Two `pub(crate)` entry points wrapped by
//! [`nanbox_encode`](crate::nanbox_encode) under the NaN-box
//! AnyValue ABI:
//!
//! - [`any_to_str`] — tag-dispatch ToString. Returns a freshly-owned
//!   `*mut Str` the caller drops. Wrapped as
//!   `__torajs_anyv_to_str` / `__torajs_anyv_to_str_pair`.
//! - [`any_to_number`] — tag-dispatch ToNumber. Returns `f64`.
//!   Wrapped as `__torajs_anyv_to_number`.
//!
//! Plus `AnyView::to_number()` — idiomatic-Rust mirror for the
//! already-materialized AnyView view (renamed from `AnyValue` in
//! Step 7b — `AnyValue` is now the NaN-box u64 immediate type).
//!
//! Extracted from `lib.rs` (2026-05-25, anyvalue god-file decomp
//! batch 14).

use std::ffi::c_void;

use torajs_rc::{__torajs_rc_inc, AnySlotTag, HeapHeader, Tag};

use crate::{
    __torajs_bool_to_str, __torajs_f64_to_str, __torajs_i64_to_str, __torajs_null_to_str,
    __torajs_str_alloc_pooled, __torajs_str_to_number, __torajs_undefined_to_str, AnyView,
};

unsafe extern "C" {
    /// torajs-str — flatten a Substr view into a fresh owned Str.
    fn __torajs_substr_to_owned(s: *const u8) -> *mut c_void;
    /// torajs-throw — record a pending TypeError (caller's throw
    /// check unwinds).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-bigint §6.1.6.2.13 `BigInt::toString(x, 10)` — the
    /// decimal stringify §7.1.17 step 7 names. Owned Str out (rc=1),
    /// which is exactly this module's ToString return contract.
    fn __torajs_bigint_to_string(p: *const c_void) -> *mut u8;
}

/// `FLAG_SUBSTR_INLINE | FLAG_SUBSTR_VIEW` mirror (torajs-str
/// `substr.rs`, header flags bits 0 and 10) — both Substr shapes
/// share `Tag::Str` but their bytes live behind a parent+offset
/// indirection (INLINE = split-block tail, VIEW = standalone
/// charAt/slice view).
const FLAG_SUBSTR_ANY: u16 = (1 << 0) | (1 << 10);

/// Tag-dispatch ToString for a packed `(tag, value)` pair.
/// Always returns a freshly owned `*mut Str` (refcount = 1)
/// that the caller is responsible for dropping.
///
/// - `Null` → `"null"` (via [`__torajs_null_to_str`])
/// - `Undef` → `"undefined"` (per ES §7.1.17.1)
/// - `Bool` → `"true"` / `"false"`
/// - `I64` → decimal print
/// - `F64` → IEEE pretty-print (`f64::to_string`-like via the C
///   helper, kept C through Layer-2 so format semantics stay
///   identical to bun's `console.log`)
/// - `Heap` + `Tag::Str` → rc_inc the same Str pointer + return
///   (no new alloc; caller owns one ref)
/// - `Heap` + other tag → `"[object]"` placeholder until P3 lands
///   per-type pretty-print
///
/// # Safety
///
/// If `tag == Heap`, `value` must be null or a valid `*mut
/// HeapHeader`. The returned pointer is valid until the caller
/// `drop`s it (matches the pre-rewrite C contract).
pub(crate) unsafe fn any_to_str(tag: i64, value: i64) -> *mut c_void {
    unsafe { any_to_str_hint(tag, value, true) }
}

/// [`any_to_str`] for the STRING-CONCAT position (§13.15.3 steps
/// 1-2) — an object operand runs ToPrimitive with the DEFAULT hint
/// (valueOf → toString for ordinary objects; a Date maps default to
/// string order), then the primitive stringifies. The hint-string
/// face is what template literals / String() want; using it in `+`
/// ran toString ahead of valueOf (`"" + {valueOf: …}` observable
/// order vs bun).
pub(crate) unsafe fn any_to_str_prim(tag: i64, value: i64) -> *mut c_void {
    unsafe { any_to_str_hint(tag, value, false) }
}

unsafe fn any_to_str_hint(tag: i64, value: i64, hint_string: bool) -> *mut c_void {
    if tag == AnySlotTag::Null as i64 {
        return unsafe { __torajs_null_to_str() };
    }
    if tag == AnySlotTag::Undef as i64 {
        return unsafe { __torajs_undefined_to_str() };
    }
    if tag == AnySlotTag::Bool as i64 {
        return unsafe { __torajs_bool_to_str((value != 0) as i32) };
    }
    if tag == AnySlotTag::I64 as i64 {
        return unsafe { __torajs_i64_to_str(value) };
    }
    if tag == AnySlotTag::F64 as i64 {
        return unsafe { __torajs_f64_to_str(f64::from_bits(value as u64)) };
    }
    if tag == AnySlotTag::Heap as i64 {
        let child = value as *mut HeapHeader;
        if child.is_null() {
            return unsafe { __torajs_null_to_str() };
        }
        // SAFETY: child is non-null per the check above; runtime
        // invariant says it points to a valid header.
        let h = unsafe { &*child };
        if matches!(h.tag(), Tag::Str) {
            // A Substr VIEW flattens to a fresh owned Str first —
            // the concat consumers downstream (`try_concat_short` /
            // `__torajs_str_concat`) read the plain Str layout only,
            // so handing the view through read the parent-pointer
            // bytes as payload (garbage char) or ran off the block
            // (SIGSEGV) — test262 charAt S15.5.4.4_A1_T2's
            // three-way concat crash.
            if h.flags & FLAG_SUBSTR_ANY != 0 {
                return unsafe { __torajs_substr_to_owned(child as *const u8) };
            }
            // Plain Str: just rc_inc + return; the caller now owns
            // one (additional) reference.
            unsafe { __torajs_rc_inc(child as *mut c_void) };
            return child as *mut c_void;
        }
        // RFC 20260716 刀 2b — `String` wrapper cell:
        // ToString(StringWrapper) = [[StringData]] verbatim (spec
        // §7.1.17 step 3, ToPrimitive on String Object hits
        // `String.prototype.toString` which returns [[StringData]]).
        // Inner cell pointer lives at `STRING_WRAPPER_CELL_OFF = 8`.
        // rc_inc the shared Str cell so the caller owns a fresh
        // reference like the Tag::Str arm above.
        if matches!(h.tag(), Tag::StringWrapper) {
            let inner_ptr = unsafe { ((child as *const u8).add(8) as *const *mut c_void).read() };
            if inner_ptr.is_null() {
                // Empty-str wrapper (`new String(NULL)` sentinel):
                // hand back a fresh empty pooled Str so the ownership
                // discipline stays uniform.
                return unsafe { __torajs_str_alloc_pooled(0) as *mut c_void };
            }
            unsafe { __torajs_rc_inc(inner_ptr) };
            return inner_ptr;
        }
        // RFC 20260716 刀 2c — `Boolean` wrapper cell:
        // ToString(BooleanWrapper) = "true" / "false" via
        // OrdinaryToPrimitive → Boolean.prototype.toString → the
        // primitive's ToString (§7.1.17 → §7.1.17.5). Reuses the
        // primitive `bool_to_str` intrinsic.
        if matches!(h.tag(), Tag::BooleanWrapper) {
            let val = unsafe { *((child as *const u8).add(8)) };
            return unsafe { crate::__torajs_bool_to_str(val as i32) };
        }
        // RFC 20260716 刀 10 — `Number` wrapper cell:
        // ToString(NumberWrapper) = ToString([[NumberData]]) per
        // §7.1.17 step 3, OrdinaryToPrimitive on Number Object hits
        // `Number.prototype.toString(undefined)` which returns
        // ToString([[NumberData]]). Direct-read the f64 payload
        // at offset 8 and hand off to the primitive `f64_to_str`
        // — same shortcut as the StringWrapper / BooleanWrapper
        // arms above (dodges the general `heap_to_primitive` +
        // method dispatch round-trip for a wrapper class with no
        // user monkey-patching).
        if matches!(h.tag(), Tag::NumberWrapper) {
            let n = unsafe { ((child as *const u8).add(8) as *const f64).read() };
            return unsafe { __torajs_f64_to_str(n) };
        }
        // §7.1.17 step 2 — ToString(Symbol) throws: a Symbol is a
        // primitive, so ToPrimitive answers it unchanged and the
        // implicit coercion must reject HERE, before the
        // OrdinaryToPrimitive fallback below. The explicit lanes
        // bypass this arm — `String(sym)` intercepts in
        // `anyv_to_display_str` and `sym.toString()` rides the
        // method-mid dispatch (§20.4.3.3 SymbolDescriptiveString).
        // Pre-fix, the rotation-141 Symbol toString wiring made the
        // fallback FIND the Symbol's toString and answer
        // "Symbol(...)" silently — the 12 String.prototype
        // this-as-symbol abrupt test262 cases regressed.
        if matches!(h.tag(), Tag::Symbol) {
            unsafe {
                __torajs_throw_type_error(c"Cannot convert a symbol to a string".as_ptr());
            }
            return unsafe { __torajs_str_alloc_pooled(0) as *mut c_void };
        }
        // §7.1.17 step 7 — ToString(BigInt) converts the value
        // DIRECTLY (`BigInt::toString(argument, 10)`) and looks
        // nothing up. Step 8's `Assert: argument is an Object`
        // guards the OrdinaryToPrimitive tail below, and a bigint
        // never reaches it: the cell IS the primitive.
        //
        // Falling through instead ran the method dispatcher, which
        // answered correctly only because the BigInt arm of
        // `cell_method` is a kernel — the lookup itself was
        // observable. Measured (rotation 320, both directions): with
        // the pre-gate open, a patched `BigInt.prototype.toString`
        // leaked into template literals, `String()`, both sides of
        // `+`, `Array.prototype.join` and the §22.1.3 generic
        // ToString(this) coerce behind
        // `String.prototype.isWellFormed.call(1n)` — six routes,
        // all of which node answers `"1"` for.
        //
        // The sibling coercions already stand here: ToNumber's
        // BigInt arm throws rather than descend (below), ToBigInt
        // answers the cell itself, and arith's ToPrimitive lists
        // BigInt beside Str and Symbol. ToString was the one that
        // still treated a primitive as an object.
        if matches!(h.tag(), Tag::BigInt) {
            return unsafe { __torajs_bigint_to_string(child as *const c_void) as *mut c_void };
        }
        // Non-Str heap object — OrdinaryToPrimitive (hint string):
        // run the receiver's toString / valueOf and ToString the
        // first primitive result (chunk C; pre-C this answered a
        // static "[object]" placeholder without ever calling a
        // user toString).
        unsafe {
            let prim = if hint_string {
                crate::to_primitive::heap_to_primitive(child as *mut c_void, true)
            } else {
                crate::to_primitive::heap_to_primitive_default(child as *mut c_void)
            };
            match prim {
                Some(prim) => {
                    let s = crate::nanbox_ffi::__torajs_anyv_to_str(prim);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(prim);
                    s
                }
                // TypeError pending — type-correct empty-string
                // placeholder; the caller's throw check unwinds.
                None => __torajs_str_alloc_pooled(0) as *mut c_void,
            }
        }
    } else {
        // Unknown tag (defensive): treat as null.
        unsafe { __torajs_null_to_str() }
    }
}

/// Tag-dispatch `ToNumber` for a packed `(tag, value)` pair —
/// mirrors ES §7.1.4 over tora's tagged-Any subset:
///
/// - `Null` → `0.0`
/// - `Undef` → `NaN`
/// - `Bool` → `1.0` / `0.0`
/// - `I64` → cast to `f64`
/// - `F64` → bitcast `i64`-bits → `f64`
/// - `Heap` + null pointer → `0.0` (defensive — `Heap`-tag NULL
///   doesn't carry numeric semantics; the C ABI returned 0 here)
/// - `Heap` + [`Tag::Str`] → parse via [`__torajs_str_to_number`]
/// - `Heap` + other tag → `NaN` (objects coerce to NaN until the
///   `valueOf` method dispatch lands in a later phase)
/// - unknown tag → `NaN` (defensive)
///
/// # Safety
///
/// If `tag == AnySlotTag::Heap as i64`, `value` must be either
/// null or a valid `*const HeapHeader` pointing to a live heap
/// object.
pub(crate) unsafe fn any_to_number(tag: i64, value: i64) -> f64 {
    if tag == AnySlotTag::Null as i64 {
        return 0.0;
    }
    if tag == AnySlotTag::Undef as i64 {
        return f64::NAN;
    }
    if tag == AnySlotTag::Bool as i64 {
        return if value != 0 { 1.0 } else { 0.0 };
    }
    if tag == AnySlotTag::I64 as i64 {
        return value as f64;
    }
    if tag == AnySlotTag::F64 as i64 {
        return f64::from_bits(value as u64);
    }
    if tag == AnySlotTag::Heap as i64 {
        let child = value as *const HeapHeader;
        if child.is_null() {
            return 0.0;
        }
        // SAFETY: child non-null per the check above; runtime
        // invariant says it points to a live heap header.
        let h = unsafe { &*child };
        if matches!(h.tag(), Tag::Str) {
            // SAFETY: child is Tag::Str-headed; the C-side
            // __torajs_str_to_number reads the Str layout from
            // the header.
            return unsafe { __torajs_str_to_number(child as *const c_void) };
        }
        // RFC 20260716 刀 2a — `Number` wrapper cell:
        // ToNumber(NumberWrapper) = [[NumberData]] verbatim (spec
        // §7.1.4 step 8, OrdinaryToPrimitive on Number Object hits
        // `Number.prototype.valueOf` which returns [[NumberData]]).
        // Direct read is functionally equivalent and dodges the
        // whole method-dispatch machinery, which doesn't yet know
        // about wrapper cells. `NumberWrapper` layout mirrors
        // `torajs_wrapper::NUMBER_WRAPPER_VALUE_OFF = 8`.
        if matches!(h.tag(), Tag::NumberWrapper) {
            let value_ptr = unsafe { (child as *const u8).add(8) as *const f64 };
            return unsafe { value_ptr.read() };
        }
        // RFC 20260716 刀 2b — `String` wrapper cell:
        // ToNumber(StringWrapper) = ToNumber([[StringData]]) via
        // strtod (spec §7.1.4 step 8 → OrdinaryToPrimitive number →
        // valueOf returns Object (skipped) → toString returns
        // [[StringData]] → ToNumber(string)). Inner cell pointer at
        // `STRING_WRAPPER_CELL_OFF = 8`.
        if matches!(h.tag(), Tag::StringWrapper) {
            let inner_ptr = unsafe { ((child as *const u8).add(8) as *const *const c_void).read() };
            if inner_ptr.is_null() {
                // Empty string ("") → NaN? No — ES §7.1.4.1 ToNumber("")
                // = +0.
                return 0.0;
            }
            return unsafe { __torajs_str_to_number(inner_ptr) };
        }
        // RFC 20260716 刀 2c — `Boolean` wrapper cell:
        // ToNumber(BooleanWrapper) = [[BooleanData]] ? 1 : 0 via
        // OrdinaryToPrimitive → Boolean.prototype.valueOf →
        // ToNumber(bool). `BOOLEAN_WRAPPER_VALUE_OFF = 8`.
        if matches!(h.tag(), Tag::BooleanWrapper) {
            let val = unsafe { *((child as *const u8).add(8)) };
            return if val != 0 { 1.0 } else { 0.0 };
        }
        // §7.1.4 step 5 — ToNumber(Symbol) throws (twin of the
        // ToString arm above: a Symbol is a primitive, so it must
        // reject here rather than let OrdinaryToPrimitive stringify
        // it into NaN silently). NaN placeholder; the caller's
        // throw check unwinds.
        if matches!(h.tag(), Tag::Symbol) {
            unsafe {
                __torajs_throw_type_error(c"Cannot convert a symbol to a number".as_ptr());
            }
            return f64::NAN;
        }
        // §7.1.4 step 2 — ToNumber(BigInt) throws. Same posture as
        // the Symbol arm: a BigInt cell is a primitive, and letting
        // it fall into OrdinaryToPrimitive ran the object method
        // machinery over a non-object layout (`2n - (p: any)` was
        // silent no-output or SIGSEGV by layout). Callers with a
        // legal BigInt path (any_arith / any_add / any_compare /
        // any_bitwise) intercept the tag before calling here.
        if matches!(h.tag(), Tag::BigInt) {
            unsafe {
                __torajs_throw_type_error(
                    c"Cannot mix BigInt and other types, use explicit conversions".as_ptr(),
                );
            }
            return f64::NAN;
        }
        // Non-Str heap object — OrdinaryToPrimitive (hint number):
        // valueOf → toString, ToNumber of the first primitive
        // result (chunk C; pre-C every object answered NaN
        // without calling a user valueOf).
        return unsafe {
            match crate::to_primitive::heap_to_primitive(child as *mut c_void, false) {
                Some(prim) => {
                    let n = crate::nanbox_ffi::__torajs_anyv_to_number(prim);
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(prim);
                    n
                }
                // TypeError pending — NaN placeholder; the
                // caller's throw check unwinds.
                None => f64::NAN,
            }
        };
    }
    f64::NAN
}

impl AnyView {
    /// `ToNumber` per ES §7.1.4 — idiomatic-Rust mirror of
    /// [`any_to_number`] for already-materialized `AnyView`s.
    /// Same per-tag rules; the `Heap` case delegates to the C
    /// `__torajs_str_to_number` for `Tag::Str` and returns `NaN`
    /// for every other heap type.
    pub fn to_number(self) -> f64 {
        match self {
            AnyView::Null => 0.0,
            AnyView::Undef => f64::NAN,
            AnyView::Bool(b) => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            AnyView::I64(n) => n as f64,
            AnyView::F64(n) => n,
            AnyView::Heap(None) => 0.0,
            AnyView::Heap(Some(p)) => {
                // SAFETY: NonNull invariant — points to a live
                // HeapHeader.
                let h = unsafe { p.as_ref() };
                if matches!(h.tag(), Tag::Str) {
                    // SAFETY: pointer is Tag::Str-headed.
                    unsafe { __torajs_str_to_number(p.as_ptr() as *const c_void) }
                } else {
                    f64::NAN
                }
            }
            AnyView::Unknown => f64::NAN,
        }
    }
}
