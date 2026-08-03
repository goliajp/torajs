//! §22.1.3.13 / §22.1.3.20 String.prototype.{match,search} step 3 —
//! the custom-matcher legs over an Any-typed pattern argument:
//! GetMethod(regexp, @@match / @@search), then Call(matcher, regexp,
//! «S»). The lowering (`ssa_lower_call_str_match_custom`) branches
//! on the probe so the step-4 RegExpCreate coerce lane stays the
//! fallback for an absent or nullish matcher (GetMethod §7.3.11
//! step 3). The well-known index rides as an argument so one pair
//! of externs serves every symbol-dispatch method face.
//!
//! Two legs instead of one call: the probe is a read-only dict walk
//! (an accessor face's getter does run — its throw is re-observed
//! by the invoke leg and propagates from there), so the invoke
//! leg's second read answers the same property and the SSA branch
//! keys off a plain I64.

use core::ffi::c_void;

use crate::iter_any_get_method::callable_entry;
use crate::method_call_closure_dispatch::invoke_with_this;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

unsafe extern "C" {
    /// torajs-str — §6.1.5.1 well-known singleton table
    /// (alphabetical property-name order; 6 = `@@match`,
    /// 9 = `@@search`). Owned +1.
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-rc — the universal heap-header decrement.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
}

/// `WELL_KNOWN_DESCS` indices — pick the invoke legs'
/// not-a-function message (torajs-core's gate passes 6 = `@@match`
/// / 8 = `@@replace` / 9 = `@@search` / 11 = `@@split` as the
/// `wk_idx` operand).
const WK_REPLACE: i64 = 8;
const WK_SEARCH: i64 = 9;
pub(crate) const WK_SPLIT: i64 = 11;

/// The per-symbol GetMethod step-4 TypeError text.
fn not_a_function_msg(wk_idx: i64) -> &'static core::ffi::CStr {
    if wk_idx == WK_SEARCH {
        c"o[Symbol.search] is not a function"
    } else if wk_idx == WK_REPLACE {
        c"o[Symbol.replace] is not a function"
    } else if wk_idx == WK_SPLIT {
        c"o[Symbol.split] is not a function"
    } else {
        c"o[Symbol.match] is not a function"
    }
}

/// `AnySlotTag::Undef` / `AnySlotTag::Null` — mirror of
/// `member_get_symbol`'s pair encoding.
const TAG_UNDEF: u64 = 5;
const TAG_NULL: u64 = 0;

/// Step 3.a probe: 1 when `arg` carries a non-nullish own/inherited
/// method under the well-known symbol `wk_idx` (the invoke leg then
/// runs — including the GetMethod TypeError for a
/// present-but-not-callable value), 0 to fall back to the step-4
/// RegExpCreate coerce lane.
///
/// # Safety
/// `arg` is a live AnyValue; `wk_idx` indexes `WELL_KNOWN_DESCS`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_str_symbol_probe(arg: AnyValue, wk_idx: i64) -> i64 {
    unsafe {
        // Step 2 — undefined / null skip to step 4; a non-cell
        // primitive has no own symbol face and tr's builtin
        // prototypes carry none.
        if !is_cell(arg) {
            return 0;
        }
        let sym = __torajs_symbol_well_known(wk_idx);
        if sym.is_null() {
            return 0;
        }
        let (tag, payload) = crate::member_get_symbol::symbol_key_pair(arg, sym);
        let _ = __torajs_rc_dec(sym);
        // A builtin reified protocol cell is NOT a user override
        // (r289 — the RegExp @@match/@@search/@@split/@@matchAll/
        // @@replace reify): the kernel lane the caller falls back to
        // IS that cell's behavior, and dispatching through it would
        // re-enter this probe from the delegation arm — the same
        // "un-shadowed builtin @@split → fast path" test every
        // engine's split fast path makes. An own-dict / monkey-patch
        // resolution never answers a mid-carrying cell unless the
        // user stored the reified builtin itself, where the fast
        // path is again behavior-identical.
        if tag == 4 {
            let cell = payload as *mut c_void;
            // SAFETY: tag 4 payloads are live heap cells; the mid
            // read only follows for Closure-tagged cells (the only
            // layout carrying the boxed-entry discriminator).
            let ct = (cell.cast::<u8>().add(4) as *const u16).read();
            if ct == torajs_rc::Tag::Closure as u16
                && crate::method_value::builtin_method_mid(cell).is_some()
            {
                return 0;
            }
        }
        i64::from(tag != TAG_UNDEF && tag != TAG_NULL)
    }
}

/// Step 3.b-c — GetMethod(arg, @@sym) then Call(matcher, arg, «S»).
/// Runs only after a positive probe; a present-but-not-callable
/// matcher records the §7.3.11 step 4 TypeError and answers
/// undefined for the caller's throw check.
///
/// # Safety
/// `recv_str` is a live Str cell the caller keeps alive across the
/// call; `arg` is a live AnyValue; `wk_idx` indexes
/// `WELL_KNOWN_DESCS`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_str_symbol_invoke(
    recv_str: *mut c_void,
    arg: AnyValue,
    wk_idx: i64,
) -> AnyValue {
    unsafe {
        let sym = __torajs_symbol_well_known(wk_idx);
        let (tag, payload) = crate::member_get_symbol::symbol_key_pair(arg, sym);
        let _ = __torajs_rc_dec(sym);
        let Some((env, entry)) = callable_entry(tag, payload, not_a_function_msg(wk_idx)) else {
            return VALUE_UNDEFINED;
        };
        // Call(matcher, regexp, «O») — the receiver string is the
        // sole argument; argv slots are borrowed (tag 4 = Heap).
        let s_any = __torajs_anyv_box_from_pair(4, recv_str as i64);
        let argv = [s_any];
        invoke_with_this(env, entry, arg, argv.as_ptr(), 1)
    }
}

/// The two-extra-argument twin — §22.1.3.19 `@@replace` calls
/// Call(replacer, searchValue, «O, replaceValue») (and a future
/// `@@split` rides the same «O, limit» shape). `extra` arrives
/// already boxed and borrowed.
///
/// # Safety
/// `recv_str` is a live Str cell and `extra` a live AnyValue the
/// caller keeps alive across the call; `arg` is a live AnyValue;
/// `wk_idx` indexes `WELL_KNOWN_DESCS`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_str_symbol_invoke2(
    recv_str: *mut c_void,
    arg: AnyValue,
    extra: AnyValue,
    wk_idx: i64,
) -> AnyValue {
    unsafe {
        let sym = __torajs_symbol_well_known(wk_idx);
        let (tag, payload) = crate::member_get_symbol::symbol_key_pair(arg, sym);
        let _ = __torajs_rc_dec(sym);
        let Some((env, entry)) = callable_entry(tag, payload, not_a_function_msg(wk_idx)) else {
            return VALUE_UNDEFINED;
        };
        let s_any = __torajs_anyv_box_from_pair(4, recv_str as i64);
        let argv = [s_any, extra];
        invoke_with_this(env, entry, arg, argv.as_ptr(), 2)
    }
}
