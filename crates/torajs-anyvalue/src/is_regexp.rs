//! §22.1.3.7 step 3 / §22.1.3.8 step 3 / §22.1.3.23 step 3 — the
//! `IsRegExp` abstract operation (§22.1.1.10), for the one place in
//! the String search family that runs it: the needle slot of
//! `startsWith` / `endsWith` / `includes`.
//!
//! ```text
//! 22.1.1.10 IsRegExp ( argument )
//!   1. If argument is not an Object, return false.
//!   2. Let matcher be ? Get(argument, @@match).
//!   3. If matcher is not undefined, return ToBoolean(matcher).
//!   4. If argument has a [[RegExpMatcher]] internal slot, return true.
//!   5. Return false.
//! ```
//!
//! Step 2 is a plain `Get`, NOT `GetMethod` — the difference matters
//! here and it is the reason this is a separate kernel from
//! [`crate::str_match_custom`]'s `@@match` walk. That one resolves a
//! matcher it intends to CALL, so a present-but-not-callable value is
//! its §7.3.11 step-4 TypeError; `IsRegExp` only asks whether the
//! value is truthy. `{ [Symbol.match]: true }` is a regex as far as
//! this operation is concerned, and every engine agrees.
//!
//! Step 4's subject is the ARGUMENT, not the matcher — a detail worth
//! spelling out because the two sit one line apart: what it asks is
//! whether an object whose `@@match` is absent is a regex anyway. For
//! tr the `[[RegExpMatcher]]` slot IS the cell tag.
//!
//! An accessor-shaped `@@match` runs its getter here, once — that is
//! what step 2 asks for, and a throw from it propagates through the
//! caller's throw check rather than being reported as "not a regex".

use core::ffi::c_void;

use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-str — §6.1.5.1 well-known singleton table
    /// (alphabetical property-name order; 6 = `@@match`). Owned +1.
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-rc — the universal heap-header decrement.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `WELL_KNOWN_DESCS` index of `@@match`.
const WK_MATCH: i64 = 6;

/// `AnySlotTag::Undef` — mirror of `member_get_symbol`'s pair
/// encoding, the same constant [`crate::str_match_custom`] spells.
const TAG_UNDEF: u64 = 5;

/// The §22.1.3.{7,8,23} step-5 message, per method. `which` is the
/// slot's own ordering — 0 `startsWith`, 1 `endsWith`, 2 `includes` —
/// and torajs-core's `NEEDLE_METHODS` is the one table that assigns
/// it, so the two cannot drift.
fn regexp_needle_msg(which: i64) -> &'static core::ffi::CStr {
    match which {
        1 => c"Argument to String.prototype.endsWith cannot be a RegExp",
        2 => c"Argument to String.prototype.includes cannot be a RegExp",
        _ => c"Argument to String.prototype.startsWith cannot be a RegExp",
    }
}

/// §22.1.3.{7,8,23} steps 3-5 as the caller needs them: run
/// `IsRegExp` over the needle and record the TypeError when it says
/// yes. Answers nothing — the caller's throw check is what observes
/// both this raise and a `@@match` getter's own.
///
/// Recording rather than returning a verdict is what keeps the
/// lowering branch-free: the coercion that follows is the next
/// instruction, and the throw check between them is the same one
/// every other kernel-raises-here site already emits.
///
/// # Safety
/// `arg` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_reject_regexp_needle(arg: AnyValue, which: i64) {
    // SAFETY: the caller's contract, forwarded.
    if unsafe { is_regexp(arg) } != 0 {
        // SAFETY: the message is a 'static CStr.
        unsafe { __torajs_throw_type_error(regexp_needle_msg(which).as_ptr()) };
    }
}

/// `1` when `arg` is a regex per §22.1.1.10, `0` otherwise.
///
/// A `@@match` getter that threw answers `0` with the throw still
/// pending: the caller's throw check is what observes it, and
/// reporting "is a regex" instead would raise the WRONG error — so
/// the `0` must not be read as "carry on".
///
/// # Safety
/// `arg` is a live AnyValue.
pub unsafe fn is_regexp(arg: AnyValue) -> i64 {
    unsafe {
        // Step 1 — "If argument is not an Object, return false". Not
        // `is_cell`: tr carries strings, symbols and bigints ON the
        // heap while the spec calls them primitives, so the cell test
        // would send a string needle — the overwhelmingly common one
        // — through the `@@match` walk. `av_is_object` is that
        // distinction already named.
        if !crate::iter_zip_shared::av_is_object(arg) {
            return 0;
        }
        let sym = __torajs_symbol_well_known(WK_MATCH);
        if sym.is_null() {
            return 0;
        }
        // Step 2 — one Get. An accessor face's getter runs here and
        // its answer is what decides the verdict.
        let matcher = crate::member_get_symbol::symbol_key_get(arg, sym);
        let (tag, payload) = matcher.pair();
        let _ = __torajs_rc_dec(sym);
        if __torajs_throw_check() != 0 {
            return 0;
        }
        // Step 3 — present means the verdict is its truthiness, and
        // that covers a plain RegExp too: tr's reified builtin
        // `@@match` is a function cell, which is truthy. Unlike the
        // matcher walk next door, a builtin reification is NOT
        // special-cased away here — this operation never calls what
        // it finds, so there is nothing to re-enter.
        if tag != TAG_UNDEF {
            let boxed =
                crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, payload as i64);
            return i64::from(crate::nanbox_ffi::__torajs_anyv_to_bool(boxed));
        }
        // Step 4 — does the ARGUMENT have `[[RegExpMatcher]]`, which
        // for tr is the cell tag. The subject here is `arg`, not the
        // matcher: step 3 already returned if the walk found one, so
        // what is left is an object whose @@match is absent (or was
        // shadowed with undefined), and the question is whether the
        // object is a regex anyway.
        //
        // A plain `/a/` normally never reaches this line — tr reifies
        // its builtin @@match, so step 3 answers on the truthy
        // function cell. This is the backstop for when it does not,
        // and it is the step the spec actually names.
        let cell = crate::nanbox::as_void_ptr(arg);
        // SAFETY: `av_is_object` above pinned `arg` to a live heap cell,
        // and every cell layout starts with the header whose tag sits
        // at offset 4.
        let ct = (cell.cast::<u8>().add(4) as *const u16).read();
        i64::from(ct == torajs_rc::Tag::RegExp as u16)
    }
}
