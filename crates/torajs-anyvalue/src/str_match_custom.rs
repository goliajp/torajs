//! §22.1.3.13 / §22.1.3.20 String.prototype.{match,search} step 3 —
//! the custom-matcher legs over an Any-typed pattern argument:
//! GetMethod(regexp, @@match / @@search), then Call(matcher, regexp,
//! «S»). The lowering (`ssa_lower_call_str_match_custom`) branches
//! on the probe so the step-4 RegExpCreate coerce lane stays the
//! fallback for an absent or nullish matcher (GetMethod §7.3.11
//! step 3). The well-known index rides as an argument so one pair
//! of externs serves every symbol-dispatch method face.
//!
//! One call, not two legs. This was a presence probe the SSA
//! branched on plus an invoke that walked the symbol a SECOND time,
//! and the doc here claimed the probe "is a read-only dict walk (an
//! accessor face's getter does run — its throw is re-observed by the
//! invoke leg)". Under an ACCESSOR neither half of that holds: the
//! probe saw only the sentinel, so the getter never ran and there was
//! nothing to re-observe; and §7.3.11 GetMethod is ONE Get, so a
//! second walk would have run a real getter twice. The SSA branch
//! still keys off a plain I64 — it is just the verdict of the single
//! GetMethod now, with the result handed back through an out slot.

use core::ffi::c_void;

use crate::iter_any_get_method::callable_entry;
use crate::method_call_closure_dispatch::invoke_with_this;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    /// torajs-str — §6.1.5.1 well-known singleton table
    /// (alphabetical property-name order; 6 = `@@match`,
    /// 9 = `@@search`). Owned +1.
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-rc — the universal heap-header decrement.
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_throw_check() -> i64;
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

/// Steps 3.a-3.c of the §22.1.3.x symbol-dispatch shape, as the ONE
/// GetMethod the spec asks for: `1` with `*out` carrying
/// Call(method, arg, «S[, extra]») — a user throw, or the GetMethod
/// step-4 not-callable TypeError, rides along for the caller's throw
/// check — and `0` when there is no custom method, which is the
/// caller's cue to run its step-4 coerce lane.
///
/// `argc` is 1 for the `«S»` faces (`@@match` / `@@search`) and 2 for
/// the `«S, extra»` ones (`@@replace`'s replaceValue, `@@split`'s
/// raw limit).
///
/// # Safety
/// `recv_str` is a live Str cell the caller keeps alive across the
/// call; `arg` and `extra` are live AnyValues; `wk_idx` indexes
/// `WELL_KNOWN_DESCS`; `out` is a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_str_symbol_try(
    recv_str: *mut c_void,
    arg: AnyValue,
    extra: AnyValue,
    argc: i64,
    wk_idx: i64,
    out: *mut AnyValue,
) -> i64 {
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
        // GetMethod is a real Get, so an accessor-shaped matcher runs
        // its getter — once, here, and its ANSWER decides the branch.
        let (tag, payload, owned) = crate::member_get_symbol::symbol_key_get(arg, sym);
        let _ = __torajs_rc_dec(sym);
        // A getter that threw answers undefined with the throw
        // pending; reporting "no custom method" would run the coerce
        // lane with it still in flight.
        if __torajs_throw_check() != 0 {
            *out = VALUE_UNDEFINED;
            return 1;
        }
        // A builtin reified protocol cell is NOT a user override
        // (r289 — the RegExp @@match/@@search/@@split/@@matchAll/
        // @@replace reify): the kernel lane the caller falls back to
        // IS that cell's behavior, and dispatching through it would
        // re-enter from the delegation arm — the same "un-shadowed
        // builtin @@split → fast path" test every engine's split fast
        // path makes. An own-dict / monkey-patch resolution never
        // answers a mid-carrying cell unless the user stored the
        // reified builtin itself, where the fast path is again
        // behavior-identical.
        if tag == 4 {
            let cell = payload as *mut c_void;
            // SAFETY: tag 4 payloads are live heap cells; the mid
            // read only follows for Closure-tagged cells (the only
            // layout carrying the boxed-entry discriminator).
            let ct = (cell.cast::<u8>().add(4) as *const u16).read();
            if ct == torajs_rc::Tag::Closure as u16
                && crate::method_value::builtin_method_mid(cell).is_some()
            {
                release_owned(owned, tag, payload);
                return 0;
            }
        }
        // §7.3.11 step 3 — a nullish matcher means "no method", which
        // is the coerce lane. A getter is what makes this reachable
        // after the walk rather than before it.
        if tag == TAG_UNDEF || tag == TAG_NULL {
            // Nullish carries no cell, so `owned` needs no release.
            return 0;
        }
        let r = call_matcher(recv_str, arg, extra, argc, tag, payload, wk_idx);
        release_owned(owned, tag, payload);
        *out = r;
        1
    }
}

/// Release a getter-produced matcher. A non-cell answer (a number, a
/// string primitive) carries nothing to drop, which the box handles.
///
/// # Safety
/// `(tag, payload)` is a live member pair.
unsafe fn release_owned(owned: bool, tag: u64, payload: u64) {
    if owned {
        unsafe { __torajs_anyv_rc_dec(__torajs_anyv_box_from_pair(tag as i64, payload as i64)) };
    }
}

/// Step 3.c's `Call(matcher, arg, «S[, extra]»)` over an
/// already-resolved matcher, split out so its caller releases a
/// getter-produced one across both exits exactly once. A matcher that
/// is present but not callable records the §7.3.11 step-4 TypeError
/// and answers undefined for the caller's throw check.
///
/// # Safety
/// As [`__torajs_any_str_symbol_try`]'s.
unsafe fn call_matcher(
    recv_str: *mut c_void,
    arg: AnyValue,
    extra: AnyValue,
    argc: i64,
    tag: u64,
    payload: u64,
    wk_idx: i64,
) -> AnyValue {
    unsafe {
        let Some((env, entry)) = callable_entry(tag, payload, not_a_function_msg(wk_idx)) else {
            return VALUE_UNDEFINED;
        };
        // The receiver string is the first argument; argv slots are
        // borrowed (tag 4 = Heap), and the `extra` slot is only read
        // when `argc` is 2.
        let s_any = __torajs_anyv_box_from_pair(4, recv_str as i64);
        let argv = [s_any, extra];
        invoke_with_this(env, entry, arg, argv.as_ptr(), argc)
    }
}
