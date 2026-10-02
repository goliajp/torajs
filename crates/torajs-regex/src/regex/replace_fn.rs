//! `__torajs_str_replace_regex_fn` / `_all_regex_fn` callback form —
//! port of `runtime_regex.c` L2602-2854.
//!
//! Per match, runtime constructs a temp Str for the matched bytes
//! plus N temp Strs for capture groups and invokes the user cb
//! through [`super::replace_fn_dispatch::invoke_replace_cb`].
//! `has_off_input` switches between the basic `(env, m, g1..gN)`
//! and the spec-full `(env, m, g1..gN, offset_i64, input_str)` cb
//! arities (ES §22.1.3.18).
//!
//! [`__torajs_str_replace_regex_fn_boxed`] is the runtime-dispatch
//! twin for a replaceValue whose callable-ness was only discovered
//! at runtime (an `any` slot): same walk, but the invoke rides the
//! closure's boxed entry with the pattern's OWN capture count
//! shaping the argv, since no call site declared the arity.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::replace_fn_typed::typed_invoke;
use super::replace_fn_walk::replace_fn_inner;
use super::str_helpers::subject::Subject;
use super::{
    __torajs_str_drop, __torajs_throw_type_error, RegExp, as_regex, str_from_bytes, str_units,
};
use crate::parser::RE_FLAG_G;
use crate::vm::save_slot;

unsafe extern "C" {
    /// torajs-rc — the one boxed-entry reader (link-judged; see
    /// `torajs_rc::closure_entry`).
    fn __torajs_closure_boxed_entry(cell: *const u8) -> u64;
    /// torajs-anyvalue — NaN-box a (tag, value) pair.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// torajs-anyvalue — the boxed value's slot tag.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    /// torajs-anyvalue — the boxed value's payload word.
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-anyvalue — ToString over a boxed value (fresh Str).
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    /// torajs-throw — non-zero iff a pending throw is recorded.
    fn __torajs_throw_check() -> i64;
}

/// Boxed closure entry ABI — mirror of the `__boxed_<fn>` wrappers
/// ssa_lower synthesizes at closure+32; must move in lockstep with
/// `torajs-str/src/transform/replace_fn.rs`'s copy.
type BoxedEntry = unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64;

/// Closure header flags word (cell +6) and the receiver-first bit —
/// mirror of `torajs_rc::FLAG_CLOSURE_RECV_FIRST` (bit 12). A
/// promoted callback declares `__this` first; §22.2.6.11 step 14.j
/// runs the replacer with an undefined receiver, so the boxed walk
/// seeds argv[0] undefined and shifts the spec args up by one.
const CLOSURE_FLAGS_OFF: usize = 6;
const FLAG_CLOSURE_RECV_FIRST: u16 = 1 << 12;

/// Minimum argv slot count handed to the boxed entry — the boxed
/// wrapper reads exactly its callee's declared-param count of slots,
/// so a callback declaring more params than the spec args must read
/// the spec-correct `undefined`, not out-of-bounds garbage (same
/// constant as the literal-needle lane's `ARGV_SLOTS`).
const BOXED_ARGV_MIN: usize = 16;

/// AnySlotTag values mirrored from `__torajs_anyv_box_from_pair`'s
/// contract (torajs-anyvalue/nanbox_encode.rs).
const TAG_I64: i64 = 2;
const TAG_HEAP: i64 = 4;
const TAG_UNDEF: i64 = 5;

/// The boxed lane's invoke strategy — §22.2.6.11 step 14.j's full
/// «matched, p1..pn, position, string, groups» argv over the closure's
/// boxed entry, which reads exactly its callee's declared-param count
/// of slots. The capture count is the PATTERN's own, read off the
/// RegExp cell, so position/string land on their spec slots for every
/// callback arity; a non-participating group boxes the real
/// `undefined`, not a Str sentinel, and the groups object is there
/// only for a pattern that names a capture. The callback's return
/// takes ToString; its pending throw aborts the walk via `Err`.
unsafe fn boxed_invoke(
    closure: *mut c_void,
    re: &RegExp,
    m: *mut c_void,
    saves: &[i64],
    subject: &Subject,
    off: i64,
    input: *const c_void,
) -> Result<*mut c_void, ()> {
    unsafe {
        let entry: BoxedEntry =
            core::mem::transmute(__torajs_closure_boxed_entry(closure as *const u8) as usize);
        let shift = usize::from(
            ((closure as *const u8).add(CLOSURE_FLAGS_OFF) as *const u16).read()
                & FLAG_CLOSURE_RECV_FIRST
                != 0,
        );
        let undef = __torajs_anyv_box_from_pair(TAG_UNDEF, 0);
        let n_caps = re.n_captures.max(0) as usize;
        let groups = super::groups::build_groups(re, subject, saves);
        let live = shift + 1 + n_caps + 2 + usize::from(groups.is_some());
        let mut argv: Vec<u64> = Vec::with_capacity(live.max(BOXED_ARGV_MIN));
        // Receiver-first shift — argv[0] stays the undefined
        // receiver, the spec args move up one.
        if shift != 0 {
            argv.push(undef);
        }
        argv.push(__torajs_anyv_box_from_pair(TAG_HEAP, m as i64));
        let mut cap_strs: Vec<*mut c_void> = Vec::with_capacity(n_caps);
        for i in 0..n_caps {
            let gs = save_slot(saves, 2 * (i + 1));
            let ge = save_slot(saves, 2 * (i + 1) + 1);
            if gs < 0 || ge < 0 {
                argv.push(undef);
            } else {
                let p = subject.span(gs, ge) as *mut c_void;
                cap_strs.push(p);
                argv.push(__torajs_anyv_box_from_pair(TAG_HEAP, p as i64));
            }
        }
        argv.push(__torajs_anyv_box_from_pair(TAG_I64, off));
        argv.push(__torajs_anyv_box_from_pair(TAG_HEAP, input as i64));
        if let Some(g) = groups {
            argv.push(__torajs_anyv_box_from_pair(TAG_HEAP, g as i64));
        }
        // Undefined pad — a callback declaring more params than the
        // spec args reads the spec-correct `undefined`, not
        // out-of-bounds garbage.
        while argv.len() < BOXED_ARGV_MIN {
            argv.push(undef);
        }
        let ret = entry(closure, argv.as_ptr(), live as i64);
        for p in cap_strs {
            __torajs_str_drop(p);
        }
        if let Some(g) = groups {
            super::__torajs_value_drop_heap(g);
        }
        if __torajs_throw_check() != 0 {
            return Err(());
        }
        let r = __torajs_anyv_to_str(ret);
        // Release the callback's returned stake (the boxed wrapper
        // hands back an owned cell for heap-shaped returns).
        if __torajs_anyv_unbox_tag(ret) == TAG_HEAP {
            let p = __torajs_anyv_unbox_value(ret) as *mut c_void;
            if !p.is_null() {
                super::__torajs_value_drop_heap(p);
            }
        }
        Ok(r)
    }
}

/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `closure_env` is null or
/// a live closure heap block (env+8 holds the cb fn pointer).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_regex_fn(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    closure_env: *mut c_void,
    n_caps: i64,
    has_off_input: i64,
) -> *mut c_void {
    if closure_env.is_null() {
        let s = unsafe { str_units(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let global = if !re_ptr.is_null() {
        unsafe { as_regex(re_ptr) }.flags & RE_FLAG_G != 0
    } else {
        false
    };
    unsafe {
        replace_fn_inner(str_ptr, re_ptr, global, |m, saves, subject, off| {
            typed_invoke(
                closure_env,
                n_caps,
                has_off_input != 0,
                str_ptr,
                m,
                saves,
                subject,
                off,
            )
        })
    }
}

/// # Safety
///
/// Same constraints as
/// [`__torajs_str_replace_regex_fn`](self::__torajs_str_replace_regex_fn).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_all_regex_fn(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    closure_env: *mut c_void,
    n_caps: i64,
    has_off_input: i64,
) -> *mut c_void {
    unsafe {
        // §22.1.5 — replaceAll throws a TypeError on a non-global RegExp
        // (same as the Str-replacement kernel). Record the pending throw
        // and answer the original string for the caller's throw check to
        // discard; the closure env is released by the caller's temp drop.
        if !re_ptr.is_null() && as_regex(re_ptr).flags & RE_FLAG_G == 0 {
            __torajs_throw_type_error(
                c"String.prototype.replaceAll called with a non-global RegExp argument"
                    .as_ptr()
                    .cast(),
            );
            let s = str_units(str_ptr);
            return str_from_bytes(&s) as *mut c_void;
        }
        if closure_env.is_null() {
            let s = str_units(str_ptr);
            return str_from_bytes(&s) as *mut c_void;
        }
        replace_fn_inner(
            str_ptr,
            re_ptr,
            /* global */ true,
            |m, saves, subject, off| {
                typed_invoke(
                    closure_env,
                    n_caps,
                    has_off_input != 0,
                    str_ptr,
                    m,
                    saves,
                    subject,
                    off,
                )
            },
        )
    }
}

/// The runtime-dispatch twin of the two typed entries above — the
/// callback arity is UNKNOWN at the call site (the replaceValue
/// arrived in an `any` slot and only its cell tag said Closure), so
/// the walk rides the closure's boxed entry with the pattern's own
/// capture count deciding the argv shape. `all` picks the replaceAll
/// step-2.b non-global rejection; plain replace still walks every
/// match when the pattern carries `g` (§22.2.6.11 step 10).
///
/// # Safety
///
/// `str_ptr` is a live owned Str (Substr views are materialized by
/// the torajs-str glue); `re_ptr` is null or a live `*RegExp`;
/// `closure` is null or a live closure heap block whose +32 slot
/// holds the boxed entry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_regex_fn_boxed(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    closure: *mut c_void,
    all: i64,
) -> *mut c_void {
    unsafe {
        if re_ptr.is_null() || closure.is_null() {
            let s = str_units(str_ptr);
            return str_from_bytes(&s) as *mut c_void;
        }
        let re = as_regex(re_ptr);
        if all != 0 && re.flags & RE_FLAG_G == 0 {
            __torajs_throw_type_error(
                c"String.prototype.replaceAll called with a non-global RegExp argument"
                    .as_ptr()
                    .cast(),
            );
            let s = str_units(str_ptr);
            return str_from_bytes(&s) as *mut c_void;
        }
        let global = all != 0 || re.flags & RE_FLAG_G != 0;
        replace_fn_inner(str_ptr, re_ptr, global, |m, saves, subject, off| {
            boxed_invoke(closure, re, m, saves, subject, off, str_ptr)
        })
    }
}
