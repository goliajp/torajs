//! One raiser per error type — the `__torajs_throw_<kind>` family the
//! lowering calls to record a pending throw.
//!
//! Split out of [`super`] in rotation 606, when adding the
//! super-delete raiser put that file one line over the 500-line limit.
//! The seam is the one its own layout already had: the parent answers
//! *how the pending-throw channel works* (the TLS slot, `set` /
//! `check` / `take`, and `throw_native` underneath), this file answers
//! *how each kind of error gets raised into it*.

use super::*;

/// The most code units a Str holds: torajs-str's `STR_MAX_LEN`,
/// mirrored like the layout offsets.
const STR_MAX_LEN: usize = (1 << 31) - 1;

/// How much of the Str `s` a message can quote beside `rest` units of
/// its own text: a quote near the longest Str is cut rather than let
/// the message pass it.
///
/// # Safety
///
/// `s` is a valid Str block.
pub(crate) unsafe fn quote_room(s: *const u8, rest: usize) -> usize {
    let len = unsafe { (s.add(STR_LEN_OFF) as *const u32).read() } as usize;
    len.min(STR_MAX_LEN - rest)
}

/// Cross-TU wrapper: torajs-bigint / torajs-regex / etc. call
/// this to raise a catchable `RangeError` (div-by-zero,
/// negative exponent, shift-too-large, `s.matchAll(re)` without
/// `g` flag, ...). The ssa_lower-side `emit_throw_check` after the
/// call propagates to the user's try/catch.
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_range_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_RANGE_ERROR as i64, msg) };
}

/// Cross-TU wrapper for `TypeError`. Parallel to
/// [`__torajs_throw_range_error`]; used by torajs-regex and
/// any future cross-TU caller raising a catchable TypeError.
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_type_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_TYPE_ERROR as i64, msg) };
}

/// Cross-TU wrapper for `ReferenceError` — RFC
/// 20260718-error-message-own-prop 刀 3 (derived-ctor no-super).
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_reference_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_REFERENCE_ERROR as i64, msg) };
}

/// §13.5.1.2 step 4.b — `delete super.x` is a ReferenceError whatever
/// the property is and whether or not it exists, because the operand
/// is a Super Reference. No argument: the message does not name the
/// property, and matches bun's text verbatim.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_reference_error_super_delete() {
    // SAFETY: a 'static NUL-terminated literal.
    unsafe { __torajs_throw_reference_error(c"Cannot delete a super property".as_ptr()) };
}

/// RFC 20260730-undeclared-ident — §6.2.5.5 GetValue on an
/// unresolvable Reference. `name` is the identifier as a tr Str
/// (the lowerer's interned literal); the message is
/// `<name> is not defined`, matching bun / V8 text.
///
/// # Safety
///
/// `name` must be a valid Str block pointer. Borrowed — only its
/// bytes are read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_reference_error_name(name: *mut c_void) {
    const SUFFIX: &[u8] = b" is not defined";
    let name = name as *const u8;
    // SAFETY: name is a valid Str block per caller invariant; len
    // field lives at STR_LEN_OFF.
    let name_len = unsafe { quote_room(name, SUFFIX.len()) };
    let total = (name_len + SUFFIX.len()) as u64;
    // SAFETY: __torajs_str_alloc_pooled returns an initialized Str
    // header with the len field set; we own the rc = 1 allocation.
    let err = unsafe { __torajs_str_alloc_pooled(total) };
    // SAFETY: dst spans `total` bytes past the payload offset of the
    // just-allocated block; both srcs are readable for their lengths
    // and non-overlapping with the fresh allocation.
    unsafe {
        ptr::copy_nonoverlapping(name.add(STR_HDR_SIZE), err.add(STR_HDR_SIZE), name_len);
        ptr::copy_nonoverlapping(
            SUFFIX.as_ptr(),
            err.add(STR_HDR_SIZE + name_len),
            SUFFIX.len(),
        );
        throw_native_str(SLOT_REFERENCE_ERROR as i64, err);
    }
}

/// §8.1.1.1 — GetValue on a `let` / `const` binding still in its
/// temporal dead zone. `name` is the identifier as a tr Str; the
/// message is `Cannot access '<name>' before initialization.`,
/// matching bun's text verbatim (JSC keeps the trailing period V8
/// omits, and the fixtures compare against bun).
///
/// # Safety
///
/// `name` must be a valid Str block pointer. Borrowed — only its
/// bytes are read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_reference_error_tdz(name: *mut c_void) {
    const PREFIX: &[u8] = b"Cannot access '";
    const SUFFIX: &[u8] = b"' before initialization.";
    let name = name as *const u8;
    // SAFETY: name is a valid Str block per caller invariant; len
    // field lives at STR_LEN_OFF.
    let name_len = unsafe { quote_room(name, PREFIX.len() + SUFFIX.len()) };
    let total = (PREFIX.len() + name_len + SUFFIX.len()) as u64;
    // SAFETY: __torajs_str_alloc_pooled returns an initialized Str
    // header with the len field set; we own the rc = 1 allocation.
    let err = unsafe { __torajs_str_alloc_pooled(total) };
    // SAFETY: dst spans `total` bytes past the payload offset of the
    // just-allocated block; all three srcs are readable for their
    // lengths and non-overlapping with the fresh allocation.
    unsafe {
        ptr::copy_nonoverlapping(PREFIX.as_ptr(), err.add(STR_HDR_SIZE), PREFIX.len());
        ptr::copy_nonoverlapping(
            name.add(STR_HDR_SIZE),
            err.add(STR_HDR_SIZE + PREFIX.len()),
            name_len,
        );
        ptr::copy_nonoverlapping(
            SUFFIX.as_ptr(),
            err.add(STR_HDR_SIZE + PREFIX.len() + name_len),
            SUFFIX.len(),
        );
        throw_native_str(SLOT_REFERENCE_ERROR as i64, err);
    }
}

/// Cross-TU wrapper for `SyntaxError` — RFC 20260720 刀 5b (the
/// §7.1.14 StringToBigInt parse-failure raise in torajs-bigint).
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_syntax_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_SYNTAX_ERROR as i64, msg) };
}

/// Cross-TU wrapper for `URIError` — the §19.2.6 malformed-URI
/// raise from the torajs-str `uri.rs` Encode / Decode kernels.
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_uri_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_URI_ERROR as i64, msg) };
}

/// The two ReferenceErrors a derived constructor's this-binding
/// raises. `bound == 0`: §9.1.1.3.4 GetThisBinding on a binding
/// `super()` has not initialized yet — a `this` read, a super property
/// access, or §10.2.2 step 15's implicit `return this`. `bound != 0`:
/// §9.1.1.3.1 BindThisValue on a binding already initialized — a
/// second `super()` call. The message spellings match bun/JSC.
///
/// # Safety
/// `extern "C"` ABI; records a pending throw only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_this_binding_throw(bound: i64) {
    let msg = if bound == 0 {
        c"'super()' must be called in derived constructor before accessing |this| or returning non-object."
    } else {
        c"'super()' can't be called more than once in a constructor."
    };
    unsafe { throw_native(SLOT_REFERENCE_ERROR as i64, msg.as_ptr()) };
}

/// A TypeError whose message the COMPILER computed. The typed lane
/// reaches this when the checker already knows the operation can only
/// throw — `1n & 1` is the canonical one: §13.15.3 does ToNumeric on
/// both sides and throws when the types disagree, and both types are
/// known at compile time. bun has to run the two ToNumeric calls to
/// find that out because it threw the types away; tsc knows but
/// produces no executable. Here the decision is made once, at compile
/// time, and what runs is this call — no type dispatch at all.
///
/// Distinct from [`__torajs_throw_type_error`] only in the argument:
/// that one takes a C string owned by a runtime kernel, this one takes
/// the interned tr Str the lowering already knows how to mint.
///
/// # Safety
///
/// `msg` must be a valid Str block pointer. Borrowed — only its bytes
/// are read, so an interned literal is a legal argument.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_type_error_str(msg: *mut c_void) {
    let msg = msg as *const u8;
    // SAFETY: msg is a valid Str block per caller invariant; len lives
    // at STR_LEN_OFF.
    let len = unsafe { (msg.add(STR_LEN_OFF) as *const u32).read() } as usize;
    // SAFETY: returns an initialized Str header with len set; we own
    // the rc = 1 allocation and hand it to throw_native_str below.
    let err = unsafe { __torajs_str_alloc_pooled(len as u64) };
    // SAFETY: dst spans `len` bytes past the payload offset of the
    // fresh allocation; src is readable for `len` and cannot overlap
    // it. The copy is what lets the caller pass a static literal.
    unsafe {
        ptr::copy_nonoverlapping(msg.add(STR_HDR_SIZE), err.add(STR_HDR_SIZE), len);
        throw_native_str(SLOT_TYPE_ERROR as i64, err);
    }
}
