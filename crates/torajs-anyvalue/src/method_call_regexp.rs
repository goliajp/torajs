//! `Tag::RegExp` arm of `__torajs_any_method_call`
//! (Any-method-call RFC 20260704 C4-3c) — `r.test(s)` / `r.exec(s)`
//! / `r.toString()` where the RegExp crossed into the `any` world.
//!
//! Mirrors the typed tier's regex method dispatch
//! (`ssa_lower_call_regex_methods.rs`) onto the `__torajs_regex_*`
//! kernels:
//!
//! - `test(s)` — ES §22.2.6.16; the haystack argument materializes
//!   through `anyv_to_str` as an owned temp this arm drops after
//!   the call (the kernel borrows), boxed bool result. `lastIndex`
//!   advance/reset for `g`/`y` regexes happens inside the kernel.
//! - `exec(s)` — ES §22.2.6.2; same owned-temp haystack. The kernel
//!   answers NULL on a miss — which boxes to JS `null` by
//!   construction — or a fresh +1 match array (with index / input /
//!   groups attached kernel-side) the box transfers out.
//! - `toString()` — ES §22.2.6.13 `/source/flags`, fresh +1 Str
//!   transferred out.
//!
//! Property reads (`r.source` / `r.lastIndex` / flag booleans) are
//! the member-read face, not method calls — they ride the
//! `emit_member_fallback` route (C4-3c-2).

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_COMPILE, ANY_METHOD_EXEC, ANY_METHOD_MATCH, ANY_METHOD_MATCH_ALL,
    ANY_METHOD_REPLACE, ANY_METHOD_SEARCH, ANY_METHOD_SPLIT, ANY_METHOD_TEST, ANY_METHOD_TO_STRING,
};

use crate::method_call::method_no_such;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::__torajs_anyv_to_str;

unsafe extern "C" {
    /// torajs-regex — anchored/global-aware test; advances/resets
    /// lastIndex for `g`/`y` regexes itself.
    fn __torajs_regex_test(re: *const c_void, s: *const c_void) -> i64;
    /// torajs-regex — exec; NULL on miss, fresh +1 match array on
    /// hit (index/input/groups attached kernel-side).
    fn __torajs_regex_exec(re: *const c_void, s: *const c_void) -> *mut c_void;
    /// torajs-regex — `/source/flags` rendering, fresh +1 Str.
    fn __torajs_regex_to_string(re: *const c_void) -> *mut c_void;
    /// torajs-regex — Annex B §B.2.4.1 in-place recompile; answers
    /// an OWNED share of the receiver, or null with a throw pending.
    fn __torajs_regex_compile_inplace(re: *mut c_void, pat: u64, flags: u64) -> *mut c_void;
    /// torajs-str — release a heap Str/Substr reference (the owned
    /// haystack temp).
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-arr — stamp the element-kind field on the typed array
    /// `exec` returns (its slots are Str heap pointers →
    /// `ARR_KIND_HEAP`) so kind-aware any-world reads stay correct
    /// (the C2 `split` stamping precedent).
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
}

/// `ARR_KIND_HEAP` mirror (torajs-rc) — the one kind `exec` match
/// slots ever hold.
const KIND_HEAP_CHAIN: u64 = 4;

/// `Tag::RegExp` arm — see module doc.
pub(crate) unsafe fn regexp_method(
    re: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_TEST => {
                let hay = __torajs_anyv_to_str(arg_at(0));
                let hit = __torajs_regex_test(re, hay as *const c_void);
                __torajs_str_drop(hay);
                __torajs_anyv_box_from_pair(1, hit)
            }
            m if m == ANY_METHOD_EXEC => {
                let hay = __torajs_anyv_to_str(arg_at(0));
                let arr = __torajs_regex_exec(re, hay as *const c_void);
                __torajs_str_drop(hay);
                if !arr.is_null() {
                    __torajs_arr_mark_kind(arr, KIND_HEAP_CHAIN);
                }
                // NULL (miss) boxes to JS null by construction; a hit
                // transfers the kernel's fresh +1 array out.
                __torajs_anyv_box_from_pair(4, arr as i64)
            }
            m if m == ANY_METHOD_TO_STRING => {
                __torajs_anyv_box_from_pair(4, __torajs_regex_to_string(re) as i64)
            }
            // Annex B §B.2.4.1 — RegExp.prototype.compile(pattern,
            // flags): in-place recompile of the receiver. The kernel
            // answers an owned share of the receiver (§B.2.4.1
            // step 5 returns O), or null with a throw pending — the
            // null box rides out under the pending throw and the
            // caller's throw check discards it.
            m if m == ANY_METHOD_COMPILE => {
                let fresh = __torajs_regex_compile_inplace(re, arg_at(0), arg_at(1));
                __torajs_anyv_box_from_pair(4, fresh as i64)
            }
            // §22.2.6.8/.11/.12/.13/.14 — the @@match / @@matchAll /
            // @@replace / @@search / @@split protocol methods (r289).
            // Each flips the operand order of its String.prototype
            // delegator (`re[@@match](s, …)` ≡ `s.match(re, …)`), so
            // the arm hands the call to the Str home with the
            // receiver boxed back as the pattern argument — coercion,
            // lastIndex advance and spec evaluation order all stay in
            // one place. The haystack materializes like every other
            // arm here (owned temp, dropped after); the pattern box
            // is a pure borrow encode the Str arms never take over
            // (`regexp_drop_if_coerced` only drops MINTED cells).
            m if m == ANY_METHOD_MATCH
                || m == ANY_METHOD_MATCH_ALL
                || m == ANY_METHOD_REPLACE
                || m == ANY_METHOD_SEARCH
                || m == ANY_METHOD_SPLIT =>
            {
                let hay = __torajs_anyv_to_str(arg_at(0));
                let re_box = __torajs_anyv_box_from_pair(4, re as i64);
                // Slot 1 carries @@split's limit / @@replace's
                // replacement; absent args pad undefined, which the
                // Str arms' own arg_at treats as absent.
                let argv2: [u64; 2] = [re_box, arg_at(1)];
                let out = crate::method_call_str::str_method(hay.cast(), m, argv2.as_ptr(), 2);
                __torajs_str_drop(hay);
                out
            }
            _ => method_no_such(),
        }
    }
}
