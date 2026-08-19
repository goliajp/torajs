//! `Tag::Str` arm of `__torajs_any_method_call` (Any-method-call
//! RFC 20260704 C2 + RC-2 match) — split out of `method_call.rs` by
//! the 500-line file discipline, mirroring the mapset / weak
//! siblings. Dispatches the interned method id onto the torajs-str
//! glue kernels; `match` routes a RegExp-cell argument through the
//! typed tier's match kernel (RFC 20260706-test262-bug-corpus RC-2).

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_ANCHOR, ANY_METHOD_AT, ANY_METHOD_CHAR_AT, ANY_METHOD_CHAR_CODE_AT,
    ANY_METHOD_CODE_POINT_AT, ANY_METHOD_CONCAT, ANY_METHOD_ENDS_WITH, ANY_METHOD_INCLUDES,
    ANY_METHOD_INDEX_OF, ANY_METHOD_IS_WELL_FORMED, ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_LINK,
    ANY_METHOD_LOCALE_COMPARE, ANY_METHOD_MATCH, ANY_METHOD_MATCH_ALL, ANY_METHOD_NORMALIZE,
    ANY_METHOD_PAD_END, ANY_METHOD_PAD_START, ANY_METHOD_REPEAT, ANY_METHOD_REPLACE,
    ANY_METHOD_REPLACE_ALL, ANY_METHOD_SEARCH, ANY_METHOD_SLICE, ANY_METHOD_SPLIT,
    ANY_METHOD_STARTS_WITH, ANY_METHOD_SUBSTR, ANY_METHOD_SUBSTRING, ANY_METHOD_SUP,
    ANY_METHOD_TO_LOCALE_LOWER_CASE, ANY_METHOD_TO_LOCALE_UPPER_CASE, ANY_METHOD_TO_LOWER_CASE,
    ANY_METHOD_TO_UPPER_CASE, ANY_METHOD_TO_WELL_FORMED, ANY_METHOD_TRIM, ANY_METHOD_TRIM_END,
    ANY_METHOD_TRIM_START, Tag,
};

use crate::method_call::{method_no_such, to_index};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_undefined};
use crate::nanbox_encode::{
    __torajs_anyv_box_from_pair, __torajs_anyv_box_i64, __torajs_anyv_box_pointer,
};
use crate::nanbox_ffi::{__torajs_anyv_to_number, __torajs_anyv_to_str};

unsafe extern "C" {
    /// torajs-str — charAt glue (empty string for OOB).
    fn __torajs_str_any_char_at(s: *mut u8, idx: i64) -> u64;
    /// torajs-str — toUpperCase / toLowerCase glue.
    fn __torajs_str_any_case(s: *const u8, upper: i64) -> u64;
    /// torajs-str — indexOf glue (found code-unit index or -1).
    fn __torajs_str_any_index_of(s: *const u8, needle: *const u8, from: i64) -> i64;
    /// torajs-str — slice glue (missing end rides as i64::MAX).
    fn __torajs_str_any_slice(s: *const u8, start: i64, end: i64) -> u64;
    /// torajs-str — substring glue (missing end rides as i64::MAX).
    fn __torajs_str_any_substring(s: *const u8, start: i64, end: i64) -> u64;
    /// torajs-str — annexB substr glue (missing length = i64::MAX).
    fn __torajs_str_any_substr(s: *const u8, start: i64, length: i64) -> u64;
    /// torajs-str — at glue (OOB answers the boxed undefined).
    fn __torajs_str_any_at(s: *const u8, i: i64) -> u64;
    /// torajs-str — charCodeAt glue (-1 = OOB, boxed as NaN here).
    fn __torajs_str_any_char_code_at(s: *const u8, i: i64) -> i64;
    /// torajs-str — startsWith glue (1/0; kernel clamps pos).
    fn __torajs_str_any_starts_with(s: *const u8, needle: *const u8, pos: i64) -> i64;
    /// torajs-str — endsWith glue (1/0; missing end rides as i64::MAX).
    fn __torajs_str_any_ends_with(s: *const u8, needle: *const u8, end: i64) -> i64;
    /// torajs-str — split glue (NULL sep = no separator argument).
    fn __torajs_str_any_split(s: *const u8, sep: *const u8) -> u64;
    /// torajs-str — split glue, RegExp-separator lane (§22.1.3.23
    /// step 2 @@split hand-off).
    fn __torajs_str_any_split_regex(s: *const u8, re: *const c_void) -> u64;
    /// torajs-str — trim glue (mode: 0 both, 1 start, 2 end).
    fn __torajs_str_any_trim(s: *const u8, mode: i64) -> u64;
    /// torajs-str — match glue (owned_src materialize + heap-chain
    /// mark + boxed-null no-match).
    fn __torajs_str_any_match(s: *const u8, re: *const c_void) -> u64;
    /// torajs-str — replace/replaceAll glue, string-pattern lane.
    fn __torajs_str_any_replace(s: *const u8, needle: *const u8, repl: *const u8, all: i64) -> u64;
    /// torajs-str — replace/replaceAll glue, functional-replaceValue
    /// leg of the string-pattern lane (§22.1.3.19 step 5).
    fn __torajs_str_any_replace_fn(
        s: *const u8,
        needle: *const u8,
        closure: *mut c_void,
        all: i64,
    ) -> u64;
    /// torajs-str — replace/replaceAll glue, RegExp-pattern lane.
    fn __torajs_str_any_replace_regex(
        s: *const u8,
        re: *const c_void,
        repl: *const u8,
        all: i64,
    ) -> u64;
    /// torajs-str — annexB B.2.2 html-method glue (`mid` picks the
    /// CreateHTML form; NULL `val` rides as the JS `undefined`).
    fn __torajs_str_any_html(s: *const u8, mid: i64, val: *const u8) -> u64;
    /// torajs-str — padStart/padEnd glue (NULL pad = missing
    /// argument → the spec's single-space default; `end` picks
    /// padEnd).
    fn __torajs_str_any_pad(s: *const u8, target_len: i64, pad: *const u8, end: i64) -> u64;
    /// torajs-str — repeat glue (negative/Infinity n records a TLS
    /// pending RangeError).
    fn __torajs_str_any_repeat(s: *const u8, n: i64) -> u64;
    /// torajs-str — one concat fold step (fresh Str out).
    fn __torajs_str_any_concat2(a: *const u8, b: *const u8) -> u64;
    /// torajs-str — codePointAt glue (-1 = OOB, boxed as undefined
    /// here).
    fn __torajs_str_any_code_point_at(s: *const u8, i: i64) -> i64;
    /// torajs-str — localeCompare glue (-1/0/1).
    fn __torajs_str_any_locale_compare(s: *const u8, other: *const u8) -> i64;
    /// torajs-str — normalize glue (NULL form = the "NFC" default;
    /// invalid form records a TLS pending RangeError).
    fn __torajs_str_any_normalize(s: *const u8, form: *const u8) -> u64;
    /// torajs-str — toLocale{Upper,Lower}Case glue (NULL locale =
    /// host default; tr/az/lt select tailored SpecialCasing).
    fn __torajs_str_any_locale_case(s: *const u8, locale: *const u8, upper: i64) -> u64;
    /// torajs-str — array-locales variant (materializes a Substr
    /// receiver, walks CanonicalizeLocaleList on the anyvalue side).
    fn __torajs_str_any_locale_case_arr(s: *const u8, arr: *const c_void, upper: i64) -> u64;
    /// torajs-str — lastIndexOf glue (missing/NaN from = i64::MAX).
    fn __torajs_str_any_last_index_of(s: *const u8, needle: *const u8, from: i64) -> i64;
    /// torajs-str — search glue (match start or -1).
    fn __torajs_str_any_search(s: *const u8, re: *const c_void) -> i64;
    /// torajs-str — matchAll glue (array of exec-shape arrays;
    /// non-global regex records a TLS pending TypeError).
    fn __torajs_str_any_match_all(s: *const u8, re: *const c_void) -> u64;
    /// torajs-str — release a heap Str/Substr reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-str — allocate a fresh heap Str from bytes (rc=1).
    /// RFC 20260716 刀 8 uses it to mint the empty-flags Str for
    /// the string→RegExp coercion lane of match/search/matchAll.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-regex — compile a pattern (`Str *`, `Str *`) → RegExp
    /// cell pointer (rc=1). Release with `__torajs_regex_drop`.
    fn __torajs_regex_compile(pattern_str: *const c_void, flags_str: *const c_void) -> *mut c_void;
    /// torajs-regex — release a RegExp cell reference.
    fn __torajs_regex_drop(re: *mut c_void);
    /// torajs-throw — read the `active` flag; non-zero iff the last
    /// runtime call recorded a pending throw. RFC 20260716 刀 21
    /// uses it inside the REPLACE arm to short-circuit the
    /// replaceValue ToString so a searchValue user-toString throw is
    /// not clobbered by a second `__torajs_throw_set`.
    fn __torajs_throw_check() -> i64;
    /// torajs-arr — kind-aware `[start, end)` copy of an Any-elem
    /// array (end clamps to len). The split-limit truncation lane.
    fn __torajs_arr_any_slice(arr: *const u8, start: i64, end: i64) -> *mut u8;
    /// torajs-arr — fresh empty Any-elem array (the lim == 0 `[]`).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// Cross-tier — universal NaN-box-safe heap-value release (the
    /// full split product after truncation).
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// RFC 20260716 刀 8 — ES §22.1.3.{11,12,13} match/search/matchAll
/// coerce a non-RegExp argument via `RegExpCreate(ToString(arg),
/// flags)`. Owns the temporary RegExp cell; the caller is
/// expected to `__torajs_regex_drop` it after the underlying
/// kernel call returns. Match/search pass `""`; matchAll passes
/// `"g"` per spec step 4.c.
unsafe fn coerce_regexp(av: AnyValue, flag_bytes: &[u8]) -> Option<*mut c_void> {
    // SAFETY: caller invariant on `av` (valid AnyValue bit pattern)
    // is the same as `regexp_cell`'s.
    if let Some(p) = unsafe { regexp_cell(av) } {
        // Borrow: the raw RegExp cell already lives longer than the
        // method call (owned by the caller / cell slot). The `*const
        // c_void` cast to `*mut` is documented — the underlying
        // kernel does not mutate the cell, and the drop path checks
        // for identity (caller-owned vs coerced-owned) via a flag.
        return Some(p as *mut c_void);
    }
    // ToString(arg) + flags Str, both freshly owned rc=1.
    // SAFETY: `__torajs_anyv_to_str` returns a valid Str heap; the
    // flags alloc reads `flag_bytes` in-bounds via the passed len.
    let pat = unsafe { __torajs_anyv_to_str(av) as *const c_void };
    let flags = unsafe {
        __torajs_str_alloc(flag_bytes.as_ptr(), flag_bytes.len() as i64) as *const c_void
    };
    let re = unsafe { __torajs_regex_compile(pat, flags) };
    unsafe { __torajs_str_drop(pat as *mut c_void) };
    unsafe { __torajs_str_drop(flags as *mut c_void) };
    Some(re)
}

/// RFC 20260716 刀 8 — sibling drop that only releases when the
/// pointer was coerced (i.e. did not come from a caller-owned
/// RegExp cell). Returns `None` when [`regexp_cell`] would have
/// returned `Some`, matching the discriminator [`coerce_regexp`]
/// used to decide whether to mint or reuse.
unsafe fn regexp_drop_if_coerced(orig: AnyValue, re: *mut c_void) {
    // SAFETY: as above (regexp_cell contract propagates).
    if unsafe { regexp_cell(orig) }.is_none() {
        unsafe { __torajs_regex_drop(re) };
    }
}

/// `Tag::Str` arm — id-switch onto the torajs-str glue.
pub(crate) unsafe fn str_method(s: *mut u8, mid: i64, argv: *const u64, argc: i64) -> AnyValue {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_CHAR_AT => __torajs_str_any_char_at(s, to_index(arg_at(0), 0)),
            m if m == ANY_METHOD_TO_UPPER_CASE => __torajs_str_any_case(s, 1),
            m if m == ANY_METHOD_TO_LOWER_CASE => __torajs_str_any_case(s, 0),
            m if m == ANY_METHOD_INDEX_OF || m == ANY_METHOD_INCLUDES => {
                // ToString the needle (owned temp), scan, drop.
                // §22.1.3.8 steps 3-4 — each coercion aborts
                // (ReturnIfAbrupt, 刀 21 posture): without the checks
                // the position coercion's throw clobbers the
                // needle's (test262 S15.5.4.7_A4 family ordering).
                let needle = __torajs_anyv_to_str(arg_at(0));
                if __torajs_throw_check() != 0 {
                    __torajs_str_drop(needle);
                    return VALUE_UNDEFINED;
                }
                let from = to_index(arg_at(1), 0);
                if __torajs_throw_check() != 0 {
                    __torajs_str_drop(needle);
                    return VALUE_UNDEFINED;
                }
                let idx = __torajs_str_any_index_of(s, needle as *const u8, from);
                __torajs_str_drop(needle);
                if m == ANY_METHOD_INDEX_OF {
                    __torajs_anyv_box_i64(idx)
                } else {
                    __torajs_anyv_box_from_pair(1, (idx >= 0) as i64)
                }
            }
            m if m == ANY_METHOD_SLICE => {
                // §22.1.3.21 steps 4-5 — ?ToIntegerOrInfinity(start)
                // aborts before end's coercion runs (its throw would
                // clobber start's; test262 S15.5.4.13_A1_T12 expects
                // "instart").
                let start = to_index(arg_at(0), 0);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                let end = to_index(arg_at(1), i64::MAX);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                __torajs_str_any_slice(s, start, end)
            }
            m if m == ANY_METHOD_SUBSTRING => {
                // §22.1.3.24 steps 4-5 — same ReturnIfAbrupt pair as
                // slice (test262 S15.5.4.15_A1_T12).
                let start = to_index(arg_at(0), 0);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                let end = to_index(arg_at(1), i64::MAX);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                __torajs_str_any_substring(s, start, end)
            }
            m if m == ANY_METHOD_SUBSTR => {
                // Annex B §B.2.2.1 steps 3-4 — same ReturnIfAbrupt
                // pair as slice.
                let start = to_index(arg_at(0), 0);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                let length = to_index(arg_at(1), i64::MAX);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                __torajs_str_any_substr(s, start, length)
            }
            m if m == ANY_METHOD_AT => __torajs_str_any_at(s, to_index(arg_at(0), 0)),
            m if m == ANY_METHOD_CHAR_CODE_AT => {
                // OOB rides the -1 contract; spec answers NaN there.
                let code = __torajs_str_any_char_code_at(s, to_index(arg_at(0), 0));
                if code < 0 {
                    __torajs_anyv_box_from_pair(3, f64::NAN.to_bits() as i64)
                } else {
                    __torajs_anyv_box_i64(code)
                }
            }
            m if m == ANY_METHOD_STARTS_WITH || m == ANY_METHOD_ENDS_WITH => {
                // ToString the needle (owned temp), test, drop.
                // startsWith defaults pos to 0; endsWith's missing
                // end rides as i64::MAX (kernel clamps to length).
                // §22.1.3.23/.7 — ?ToString(search) then
                // ?ToIntegerOrInfinity(pos), each aborting
                // (ReturnIfAbrupt, 刀 21 posture).
                let needle = __torajs_anyv_to_str(arg_at(0));
                if __torajs_throw_check() != 0 {
                    __torajs_str_drop(needle);
                    return VALUE_UNDEFINED;
                }
                let pos = to_index(
                    arg_at(1),
                    if m == ANY_METHOD_STARTS_WITH {
                        0
                    } else {
                        i64::MAX
                    },
                );
                if __torajs_throw_check() != 0 {
                    __torajs_str_drop(needle);
                    return VALUE_UNDEFINED;
                }
                let hit = if m == ANY_METHOD_STARTS_WITH {
                    __torajs_str_any_starts_with(s, needle as *const u8, pos)
                } else {
                    __torajs_str_any_ends_with(s, needle as *const u8, pos)
                };
                __torajs_str_drop(needle);
                __torajs_anyv_box_from_pair(1, hit)
            }
            m if m == ANY_METHOD_SPLIT => {
                // §22.1.3.23 step 2 — a separator carrying a user
                // `@@split` dispatches with the limit passed RAW
                // (step 2 precedes step 4's ToUint32; a RegExp
                // separator misses this probe — its split rides the
                // dedicated kernel below — unless an own-dict
                // override shadows it, which is exactly step 2).
                if crate::str_match_custom::__torajs_any_str_symbol_probe(
                    arg_at(0),
                    crate::str_match_custom::WK_SPLIT,
                ) != 0
                {
                    return crate::str_match_custom::__torajs_any_str_symbol_invoke2(
                        s as *mut c_void,
                        arg_at(0),
                        arg_at(1),
                        crate::str_match_custom::WK_SPLIT,
                    );
                }
                // A present limit argument routes through the
                // ToUint32 + truncation lane; absent / undefined
                // keeps the zero-cost three-way dispatch.
                if is_undefined(arg_at(1)) {
                    split::split_with_any_sep(s, arg_at(0))
                } else {
                    split::split_any_with_limit(s, arg_at(0), arg_at(1))
                }
            }
            m if m == ANY_METHOD_MATCH => {
                // RFC 20260716 刀 8 — ES §22.1.3.11: non-RegExp arg
                // coerces via `RegExpCreate(ToString(arg), undefined)`.
                // A RegExp-cell arg passes through borrowed (owned by
                // caller); a primitive arg mints a fresh cell dropped
                // by the sibling `regexp_drop_if_coerced`.
                let arg = arg_at(0);
                let Some(re_ptr) = coerce_regexp(arg, b"") else {
                    return VALUE_UNDEFINED;
                };
                let out = __torajs_str_any_match(s, re_ptr);
                regexp_drop_if_coerced(arg, re_ptr);
                out
            }
            m if m == ANY_METHOD_REPLACE || m == ANY_METHOD_REPLACE_ALL => {
                // RC-2c — the pattern argument's cell tag picks the
                // lane; both replacement operands ToString through
                // owned temps this arm drops. A closure replacement
                // (the `_regex_fn` kernels) is a recorded follow-up.
                //
                // RFC 20260716 刀 21 — spec §22.1.3.15 step 4 (search
                // ToString) fires BEFORE step 6a (replace ToString).
                // The prior "repl first" ordering evaluated
                // `ToString(replaceValue)` before `ToString(searchValue)`
                // and, when both user-toString methods threw, clobbered
                // the earlier pending throw (test262
                // S15.5.4.11_A1_T12). Regex-cell pattern lane skips
                // the searchValue ToString path (§22.1.3.15 step 2
                // hands off to `@@replace` on the RegExp cell).
                let all = (m == ANY_METHOD_REPLACE_ALL) as i64;
                // §22.1.3.20 step 2.b precedes step 6.a — a
                // non-global RegExp searchValue disqualifies the call
                // before ToString(replaceValue) may run its user hook.
                if reject_non_global_regex_search(m, argv, argc) {
                    return VALUE_UNDEFINED;
                }
                if let Some(re_ptr) = regexp_cell(arg_at(0)) {
                    let repl = __torajs_anyv_to_str(arg_at(1));
                    let out = __torajs_str_any_replace_regex(s, re_ptr, repl as *const u8, all);
                    __torajs_str_drop(repl);
                    out
                } else {
                    let needle = __torajs_anyv_to_str(arg_at(0));
                    // A pending throw from ToString(searchValue) leaves
                    // `needle` as a placeholder — drop it and return
                    // early so ToString(replaceValue) does not stash a
                    // second pending throw that would clobber the
                    // first. Caller's emit_throw_check unwinds.
                    if __torajs_throw_check() != 0 {
                        __torajs_str_drop(needle);
                        return VALUE_UNDEFINED;
                    }
                    // §22.1.3.19 step 5 — a Closure-cell replaceValue
                    // is functionalReplace: invoke per match through
                    // the boxed-entry kernels (rotation 446, the
                    // static lane's runtime twin). ToString serves
                    // only the non-callable rest. The regex-pattern
                    // lane above keeps its recorded follow-up — its
                    // fn kernels want the pattern's capture count.
                    if let Some(cl) = closure_cell(arg_at(1)) {
                        let out = __torajs_str_any_replace_fn(s, needle as *const u8, cl, all);
                        __torajs_str_drop(needle);
                        return out;
                    }
                    let repl = __torajs_anyv_to_str(arg_at(1));
                    let out =
                        __torajs_str_any_replace(s, needle as *const u8, repl as *const u8, all);
                    __torajs_str_drop(needle);
                    __torajs_str_drop(repl);
                    out
                }
            }
            _ => ext::str_method_pad(s, mid, argv, argc),
        }
    }
}

mod any_pattern;
mod ext;
mod regex_gate;
mod split;

pub(crate) use regex_gate::reject_non_global_regex_search;

/// The argument's Closure cell pointer, or `None` for any
/// non-closure value — §22.1.3.19 step 5's IsCallable, spelled on
/// the cell tag exactly like [`regexp_cell`] below.
unsafe fn closure_cell(av: AnyValue) -> Option<*mut c_void> {
    if !is_cell(av) {
        return None;
    }
    let p = as_void_ptr(av);
    let tag = unsafe { (p.cast::<u8>().add(4) as *const u16).read() };
    if tag == Tag::Closure as u16 {
        Some(p)
    } else {
        None
    }
}

/// The argument's RegExp cell pointer, or `None` for any non-RegExp
/// value (primitives, other cell tags).
unsafe fn regexp_cell(av: AnyValue) -> Option<*const c_void> {
    if !is_cell(av) {
        return None;
    }
    let p = as_void_ptr(av);
    let tag = unsafe { (p.cast::<u8>().add(4) as *const u16).read() };
    if tag == Tag::RegExp as u16 {
        Some(p as *const c_void)
    } else {
        None
    }
}
