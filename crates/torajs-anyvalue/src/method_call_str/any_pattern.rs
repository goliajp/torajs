//! The pattern argument that arrived in an `any` slot — §22.1.3.{11,
//! 12,19,20} step 2/3 spelled for the TYPED-receiver lane.
//!
//! The any-receiver arms next door already branch on the argument's
//! cell tag: a real RegExp hands off to its own `@@match` / `@@replace`
//! / `@@split`, everything else takes the ToString step. The typed
//! lane could not, because it decides statically and a RegExp reaches
//! it with nothing but `any` written on it — `var re = /b/` is an
//! `any` binding once `desugar_var_hoist` has split the declaration
//! from its assignment, and so is every parameter annotated `any`.
//!
//! What that cost was not a refusal but a wrong answer: the lane
//! ToString'd the RegExp and used its own source text as the pattern,
//! so `"abc".match(re)` answered null, `"abc".search(re)` answered
//! -1, and `"abc".replace(re, "Y")` answered "abc" — silently, which
//! the design principles rank worst. `split` was already correct
//! because its separator rides `split_with_any_sep`; these are the
//! same dispatch for the callers that mint or replace.
//!
//! A child module reaches its parent's private items, so
//! [`super::regexp_cell`] and [`super::regexp_drop_if_coerced`] are
//! used directly with no visibility churn.

use super::*;

unsafe extern "C" {
    /// torajs-regex — non-zero iff the cell carries the flag bit.
    fn __torajs_regex_has_flag(re: *const c_void, bit: i64) -> i64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-str — typed-tier replace, literal-needle lane.
    fn __torajs_str_replace(
        s: *const c_void,
        needle: *const c_void,
        repl: *const c_void,
    ) -> *mut c_void;
    /// torajs-str — typed-tier replaceAll, literal-needle lane.
    fn __torajs_str_replace_all(
        s: *const c_void,
        needle: *const c_void,
        repl: *const c_void,
    ) -> *mut c_void;
    /// torajs-regex — typed-tier replace, RegExp lane (the cell's own
    /// `g` flag decides first-vs-all).
    fn __torajs_str_replace_regex(
        s: *const c_void,
        re: *const c_void,
        repl: *const c_void,
    ) -> *mut c_void;
    /// torajs-regex — typed-tier replaceAll, RegExp lane.
    fn __torajs_str_replace_all_regex(
        s: *const c_void,
        re: *const c_void,
        repl: *const c_void,
    ) -> *mut c_void;
    /// torajs-regex — the callback replacer over a RegExp pattern
    /// (`n_caps` capture Strs built per match; `has_off_input` adds
    /// the `(position, string)` tail).
    fn __torajs_str_replace_regex_fn(
        s: *const c_void,
        re: *const c_void,
        closure: *mut c_void,
        n_caps: i64,
        has_off_input: i64,
    ) -> *mut c_void;
    /// torajs-regex — the replaceAll twin.
    fn __torajs_str_replace_all_regex_fn(
        s: *const c_void,
        re: *const c_void,
        closure: *mut c_void,
        n_caps: i64,
        has_off_input: i64,
    ) -> *mut c_void;
    /// torajs-str — the callback replacer over a literal needle,
    /// which has no captures and so rides the boxed entry.
    fn __torajs_str_replace_fn(
        s: *const c_void,
        needle: *const c_void,
        closure: *mut c_void,
    ) -> *mut c_void;
    /// torajs-str — the replaceAll twin.
    fn __torajs_str_replace_all_fn(
        s: *const c_void,
        needle: *const c_void,
        closure: *mut c_void,
    ) -> *mut c_void;
}

/// `g` bit mirror (torajs-regex parser / `member_props_regexp.rs`).
const RE_FLAG_G: i64 = 0x02;

/// §22.1.3.{11,12,20} step 3 — a real RegExp cell passes through
/// BORROWED, and only everything else takes step 4's
/// `RegExpCreate(ToString(P), F)`.
///
/// The undefined case belongs to step 4 as well: §22.2.3.2
/// RegExpInitialize step 1 makes P the empty String, not the text
/// "undefined".
///
/// # Safety
/// `av` carries a valid AnyValue bit pattern; `flags` is a live Str
/// cell owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regexp_from_any(
    av: AnyValue,
    flags: *const c_void,
) -> *mut c_void {
    unsafe {
        if let Some(p) = regexp_cell(av) {
            return p as *mut c_void;
        }
        let pat = if is_undefined(av) {
            __torajs_str_alloc(c"".as_ptr().cast(), 0) as *const c_void
        } else {
            __torajs_anyv_to_str(av) as *const c_void
        };
        let re = __torajs_regex_compile(pat, flags);
        __torajs_str_drop(pat as *mut c_void);
        re
    }
}

/// The release sibling of [`__torajs_regexp_from_any`] — only the
/// cell that call minted is ours to drop; a borrowed one belongs to
/// the caller's `any` slot.
///
/// # Safety
/// `av` is the same AnyValue the mint saw; `re` is its result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regexp_drop_if_coerced(av: AnyValue, re: *mut c_void) {
    unsafe { regexp_drop_if_coerced(av, re) }
}

/// The callback-replacer twin of [`__torajs_str_replace_any_pattern`]
/// — same step-2 dispatch, with the replaceValue a function instead
/// of a string.
///
/// `n_caps` is what the CALLBACK declares, not what the pattern has:
/// the caller cannot count groups in a pattern it only knows as
/// `any`, and the callback reads exactly its own declared slots
/// anyway. A slot past the pattern's real group count reads back as
/// the non-participating sentinel, which is the same `undefined` the
/// spec would hand it. The `(position, string)` tail is excluded by
/// the caller for the one shape that would go wrong — naming it pins
/// every position, and only the pattern knows how many precede it.
///
/// A literal needle has no captures at all, so that leg rides the
/// boxed entry (§22.1.3.18 step 10's `«matched, position, string»`)
/// exactly as the typed literal lane already does.
///
/// # Safety
/// `s` is a live Str cell; `pat_av` carries a valid AnyValue bit
/// pattern; `closure` is a live closure heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_any_pattern_fn(
    s: *const c_void,
    pat_av: AnyValue,
    closure: *mut c_void,
    n_caps: i64,
    has_off_input: i64,
    all: i64,
) -> *mut c_void {
    unsafe {
        if let Some(re) = regexp_cell(pat_av) {
            if all != 0 && __torajs_regex_has_flag(re, RE_FLAG_G) == 0 {
                __torajs_throw_type_error(
                    c"String.prototype.replaceAll called with a non-global RegExp argument"
                        .as_ptr(),
                );
                return core::ptr::null_mut();
            }
            return if all != 0 {
                __torajs_str_replace_all_regex_fn(s, re, closure, n_caps, has_off_input)
            } else {
                __torajs_str_replace_regex_fn(s, re, closure, n_caps, has_off_input)
            };
        }
        let needle = __torajs_anyv_to_str(pat_av) as *const c_void;
        if __torajs_throw_check() != 0 {
            __torajs_str_drop(needle as *mut c_void);
            return core::ptr::null_mut();
        }
        let out = if all != 0 {
            __torajs_str_replace_all_fn(s, needle, closure)
        } else {
            __torajs_str_replace_fn(s, needle, closure)
        };
        __torajs_str_drop(needle as *mut c_void);
        out
    }
}

/// §22.1.3.19 step 2 / §22.1.3.20 step 2 for a typed receiver whose
/// searchValue arrived in an `any` slot: a RegExp cell hands off to
/// the regex kernel, everything else is a literal substring search
/// over `ToString(searchValue)`. Coercing the RegExp instead — which
/// is what the typed lane did — searched for the six characters of
/// its own source text and found none.
///
/// `all` picks `replaceAll`, whose step 2.b rejects a non-global
/// RegExp before anything else runs; the pending TypeError rides the
/// caller's throw check, the same one the literal-regex lane already
/// emits.
///
/// # Safety
/// `s` and `repl` are live Str cells; `pat_av` carries a valid
/// AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_any_pattern(
    s: *const c_void,
    pat_av: AnyValue,
    repl: *const c_void,
    all: i64,
) -> *mut c_void {
    unsafe {
        if let Some(re) = regexp_cell(pat_av) {
            if all != 0 && __torajs_regex_has_flag(re, RE_FLAG_G) == 0 {
                __torajs_throw_type_error(
                    c"String.prototype.replaceAll called with a non-global RegExp argument"
                        .as_ptr(),
                );
                return core::ptr::null_mut();
            }
            return if all != 0 {
                __torajs_str_replace_all_regex(s, re, repl)
            } else {
                __torajs_str_replace_regex(s, re, repl)
            };
        }
        let needle = __torajs_anyv_to_str(pat_av) as *const c_void;
        // A pending throw from ToString(searchValue) leaves `needle`
        // a placeholder — release it and let the caller's check
        // unwind rather than running the search on it.
        if __torajs_throw_check() != 0 {
            __torajs_str_drop(needle as *mut c_void);
            return core::ptr::null_mut();
        }
        let out = if all != 0 {
            __torajs_str_replace_all(s, needle, repl)
        } else {
            __torajs_str_replace(s, needle, repl)
        };
        __torajs_str_drop(needle as *mut c_void);
        out
    }
}

/// §22.1.3.19 step 5 for a replaceValue that arrived in an `any`
/// slot — `functionalReplace = IsCallable(replaceValue)` is a
/// RUNTIME question there, and the static lanes deciding it by
/// checker type alone spliced a function's source text into the
/// output (`"ab".replace("b", getFn())` — the fn-return face's
/// promoted values are exactly `any`-typed, rotation 446). A
/// Closure cell rides the functional kernels (their boxed entry
/// adapts to the callback's own declared arity — the literal
/// needle has no capture groups, so «matched, position, string»
/// is the whole spread); everything else is §22.1.3.18 step 12's
/// ToString(replaceValue) over the literal kernels.
///
/// The needle arrives as a Str the CALLER coerced (its static
/// pattern gate mirrors [`crate::ssa_lower_str_replace_fn`]'s) —
/// the pattern-`any` twin stays with the two kernels above.
///
/// # Safety
/// `s` and `needle` are live Str cells; `repl_av` carries a valid
/// AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_any_repl(
    s: *const c_void,
    needle: *const c_void,
    repl_av: AnyValue,
    all: i64,
) -> *mut c_void {
    unsafe {
        if is_cell(repl_av) {
            let p = as_void_ptr(repl_av);
            let tag = (p.cast::<u8>().add(4) as *const u16).read();
            if tag == Tag::Closure as u16 {
                return if all != 0 {
                    __torajs_str_replace_all_fn(s, needle, p)
                } else {
                    __torajs_str_replace_fn(s, needle, p)
                };
            }
        }
        let repl = __torajs_anyv_to_str(repl_av) as *const c_void;
        if __torajs_throw_check() != 0 {
            __torajs_str_drop(repl as *mut c_void);
            return core::ptr::null_mut();
        }
        let out = if all != 0 {
            __torajs_str_replace_all(s, needle, repl)
        } else {
            __torajs_str_replace(s, needle, repl)
        };
        __torajs_str_drop(repl as *mut c_void);
        out
    }
}
