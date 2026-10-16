//! §22.1.3.13 / §22.1.3.20 String.prototype.{match,search} step 3 —
//! the custom-matcher legs over an Any-typed pattern argument:
//! GetMethod(regexp, @@match / @@search), then Call(matcher, regexp,
//! «S»). The lowering (`ssa_lower_call_str_match_custom`) branches
//! on that one GetMethod's verdict so the step-4 RegExpCreate coerce
//! lane stays the fallback for an absent or nullish matcher (GetMethod
//! §7.3.11 step 3). The well-known index rides as an argument so one
//! extern serves every symbol-dispatch method face.
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

use torajs_rc::{
    ANY_METHOD_MATCH, ANY_METHOD_MATCH_ALL, ANY_METHOD_REPLACE, ANY_METHOD_REPLACE_ALL,
    ANY_METHOD_SPLIT,
};

use crate::iter_any_get_method::callable_method;
use crate::method_call_closure_dispatch::invoke_with_this;
use crate::method_call_str::{reject_non_global_regex_search, str_method_switch};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;

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
/// / 7 = `@@matchAll` / 8 = `@@replace` / 9 = `@@search` / 11 =
/// `@@split` as the `wk_idx` operand).
pub(crate) const WK_MATCH: i64 = 6;
pub(crate) const WK_MATCH_ALL: i64 = 7;
pub(crate) const WK_REPLACE: i64 = 8;
pub(crate) const WK_SEARCH: i64 = 9;
pub(crate) const WK_SPLIT: i64 = 11;

/// The per-symbol GetMethod step-4 TypeError text, in the spec's
/// `@@name` spelling. Kept short: every program that links the
/// dispatch carries all five.
fn not_a_function_msg(wk_idx: i64) -> &'static core::ffi::CStr {
    if wk_idx == WK_SEARCH {
        c"@@search is not callable"
    } else if wk_idx == WK_REPLACE {
        c"@@replace is not callable"
    } else if wk_idx == WK_SPLIT {
        c"@@split is not callable"
    } else if wk_idx == WK_MATCH_ALL {
        c"@@matchAll is not callable"
    } else {
        c"@@match is not callable"
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
/// `argc` is 1 for the `«S»` faces (`@@match` / `@@matchAll` /
/// `@@search`) and 2 for
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
        // Step 2 asks only an Object: a primitive pattern skips to
        // step 4 whatever its prototype carries, so a RegExp protocol
        // method stored on String.prototype never sees a primitive
        // `this`.
        if !crate::iter_zip_shared::av_is_object(arg) {
            return 0;
        }
        let sym = __torajs_symbol_well_known(wk_idx);
        if sym.is_null() {
            return 0;
        }
        // GetMethod is a real Get, so an accessor-shaped matcher runs
        // its getter — once, here, and its ANSWER decides the branch.
        let matcher = crate::member_get_symbol::symbol_key_get(arg, sym);
        let (tag, payload) = matcher.pair();
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
                && let Some(mid) = crate::method_value::builtin_method_mid(cell)
            {
                // ... unless the matcher's receiver could tell the
                // kernel lane from the method's own algorithm (an own
                // `exec`, a patched prototype, a non-RegExp object
                // carrying the builtin) — then the method runs.
                if crate::method_value::builtin_method_family(cell)
                    != crate::method_value::REGEXP_PROTO_FAMILY
                {
                    return 0;
                }
                let argv = [__torajs_anyv_box_from_pair(4, recv_str as i64), extra];
                let spec = crate::dispatch_seam::__torajs_regexp_protocol_call(
                    arg,
                    mid,
                    argv.as_ptr(),
                    argc,
                );
                if spec == crate::regexp_protocol::KERNEL_LANE {
                    return 0;
                }
                *out = spec;
                return 1;
            }
        }
        // §7.3.11 step 3 — a nullish matcher means "no method", which
        // is the coerce lane. A getter is what makes this reachable
        // after the walk rather than before it.
        if tag == TAG_UNDEF || tag == TAG_NULL {
            return 0;
        }
        // The call's `env` aliases a getter-produced matcher's cell, so
        // `matcher` outliving it is what makes the one release land
        // after the invoke rather than inside it.
        *out = call_matcher(recv_str, arg, extra, argc, tag, payload, wk_idx);
        1
    }
}

/// Step 2 of every String.prototype method whose pattern argument
/// may carry a user symbol method (split, match, matchAll, replace,
/// replaceAll, search) for an any-typed string receiver, ahead of the
/// method-id switch so the arms keep only their step-4 lanes. matchAll
/// and replaceAll first reject a non-global RegExp (step 2.b). Answers
/// the custom method's result (undefined with a throw pending when
/// step 2.b or the custom method threw), else the method's own arm.
/// The caller only gates on the method id, so both of its exits stay
/// tail calls.
///
/// # Safety
/// `s` is a live Str cell; `argv` addresses `argc` valid AnyValues.
#[inline(never)]
pub(crate) unsafe fn str_method_step2(
    s: *mut u8,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    // «S, replaceValue» / «S, limit» take two arguments, the others
    // «S»; the callee reads `extra` only when told there are two
    let (wk_idx, n) = match mid {
        ANY_METHOD_SPLIT => (WK_SPLIT, 2),
        ANY_METHOD_MATCH => (WK_MATCH, 1),
        ANY_METHOD_MATCH_ALL => (WK_MATCH_ALL, 1),
        ANY_METHOD_REPLACE | ANY_METHOD_REPLACE_ALL => (WK_REPLACE, 2),
        // ANY_METHOD_SEARCH, the last id the caller lets through
        _ => (WK_SEARCH, 1),
    };
    if unsafe { reject_non_global_regex_search(mid, argv, argc) } {
        return VALUE_UNDEFINED;
    }
    let arg_at = |i: i64| {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    let mut out: AnyValue = VALUE_UNDEFINED;
    let hit = unsafe {
        __torajs_any_str_symbol_try(s.cast(), arg_at(0), arg_at(1), n, wk_idx, &raw mut out)
    };
    if hit != 0 {
        return out;
    }
    unsafe { str_method_switch(s, mid, argv, argc) }
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
        let Some(m) = callable_method(tag, payload, arg, not_a_function_msg(wk_idx)) else {
            return VALUE_UNDEFINED;
        };
        // The receiver string is the first argument; argv slots are
        // borrowed (tag 4 = Heap), and the `extra` slot is only read
        // when `argc` is 2.
        let s_any = __torajs_anyv_box_from_pair(4, recv_str as i64);
        let argv = [s_any, extra];
        invoke_with_this(m.env(), m.entry(), arg, argv.as_ptr(), argc)
    }
}
