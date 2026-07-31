//! §22.1.3.13 String.prototype.match step 3 — the custom-matcher
//! legs over an Any-typed pattern argument: GetMethod(regexp,
//! @@match), then Call(matcher, regexp, «S»). The lowering
//! (`ssa_lower_call_str_match_custom`) branches on the probe so the
//! step-4 RegExpCreate coerce lane stays the fallback for an absent
//! or nullish matcher (GetMethod §7.3.11 step 3).
//!
//! Two legs instead of one call: the probe is a read-only dict walk
//! (no user-visible side effects — @@match faces are data
//! properties), so the invoke leg's second read answers the same
//! property and the SSA branch keys off a plain I64.

use core::ffi::c_void;

use crate::iter_any_get_method::callable_entry;
use crate::method_call_closure_dispatch::invoke_with_this;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

unsafe extern "C" {
    /// torajs-str — §6.1.5.1 well-known singleton table; idx 6 is
    /// `@@match` (alphabetical property-name order). Owned +1.
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-rc — the universal heap-header decrement.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
}

/// `WELL_KNOWN_DESCS` index of `Symbol.match`.
const WK_MATCH: i64 = 6;

/// `AnySlotTag::Undef` / `AnySlotTag::Null` — mirror of
/// `member_get_symbol`'s pair encoding.
const TAG_UNDEF: u64 = 5;
const TAG_NULL: u64 = 0;

/// §22.1.3.13 step 3.a probe: 1 when `arg` carries a non-nullish
/// own/inherited `@@match` (the invoke leg then runs — including the
/// GetMethod TypeError for a present-but-not-callable value), 0 to
/// fall back to the step-4 RegExpCreate coerce lane.
///
/// # Safety
/// `arg` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_str_match_probe(arg: AnyValue) -> i64 {
    unsafe {
        // Step 2 — undefined / null skip to step 4; a non-cell
        // primitive has no own @@match and tr's builtin prototypes
        // carry none.
        if !is_cell(arg) {
            return 0;
        }
        let sym = __torajs_symbol_well_known(WK_MATCH);
        if sym.is_null() {
            return 0;
        }
        let (tag, _) = crate::member_get_symbol::symbol_key_pair(arg, sym);
        let _ = __torajs_rc_dec(sym);
        i64::from(tag != TAG_UNDEF && tag != TAG_NULL)
    }
}

/// §22.1.3.13 step 3.b-c — GetMethod(arg, @@match) then
/// Call(matcher, arg, «S»). Runs only after a positive probe; a
/// present-but-not-callable matcher records the §7.3.11 step 4
/// TypeError and answers undefined for the caller's throw check.
///
/// # Safety
/// `recv_str` is a live Str cell the caller keeps alive across the
/// call; `arg` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_str_match_invoke(
    recv_str: *mut c_void,
    arg: AnyValue,
) -> AnyValue {
    unsafe {
        let sym = __torajs_symbol_well_known(WK_MATCH);
        let (tag, payload) = crate::member_get_symbol::symbol_key_pair(arg, sym);
        let _ = __torajs_rc_dec(sym);
        let Some((env, entry)) = callable_entry(tag, payload, c"o[Symbol.match] is not a function")
        else {
            return VALUE_UNDEFINED;
        };
        // Call(matcher, regexp, «O») — the receiver string is the
        // sole argument; argv slots are borrowed (tag 4 = Heap).
        let s_any = __torajs_anyv_box_from_pair(4, recv_str as i64);
        let argv = [s_any];
        invoke_with_this(env, entry, arg, argv.as_ptr(), 1)
    }
}
