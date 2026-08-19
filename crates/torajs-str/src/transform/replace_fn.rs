//! `__torajs_str_replace_fn` / `__torajs_str_replace_all_fn` — the
//! functional-replaceValue lane for the STRING-pattern replace family
//! (ES §22.1.3.18 / §22.1.3.19). The regex-pattern lane lives at
//! `torajs-regex/src/regex/replace_fn.rs`.
//!
//! The callback runs through the closure's BOXED entry (closure+32,
//! `fn(env, argv, argc) -> AnyValue`) instead of the regex lane's
//! typed transmute dispatch: an untyped callback's params are
//! `Type::Any` at the ABI, and the boxed wrapper reads exactly its
//! callee's declared-param count of argv slots. A string pattern has
//! no capture groups, so argv is always
//! `[matched: Str, position: I64, whole: Str]` (ES §22.1.3.18 step
//! 10) padded to [`ARGV_SLOTS`] with `undefined` — a callback
//! declaring more params reads the spec-correct `undefined` instead
//! of out-of-bounds garbage.
//!
//! Position is reported in code units (the Str payload IS code
//! units: Latin-1 stride 1 / UTF-16 stride 2), matching
//! `String.prototype.indexOf`. The callback's return value takes
//! `ToString` (`__torajs_anyv_to_str`); a pending throw raised
//! inside the callback aborts the walk and the caller's throw-check
//! discards the placeholder result.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::replace::{
    PayloadBuf, alloc_str_copy, build_result_with, coerce_payload, find_first_with_stride,
    splice_into, str_view,
};

unsafe extern "C" {
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_str_drop(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    /// torajs-regex — the boxed-entry callback replacer over a
    /// RegExp pattern (n_caps read off the cell's own capture
    /// count; `all` picks replaceAll's non-global rejection).
    fn __torajs_str_replace_regex_fn_boxed(
        s: *const c_void,
        re: *const c_void,
        closure: *mut c_void,
        all: i64,
    ) -> *mut c_void;
}

/// Boxed closure entry ABI — mirrors the `__boxed_<fn>` wrappers
/// ssa_lower synthesizes at `CLOSURE_BOXED_ENTRY_OFF` (+32).
type BoxedEntry = unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64;

const CLOSURE_BOXED_ENTRY_OFF: usize = 32;

/// Closure header flags word (cell +6) and the receiver-first bit —
/// mirror of `torajs_rc::FLAG_CLOSURE_RECV_FIRST` (bit 12), must move
/// in lockstep. A promoted callback (`function () { …this… }`)
/// declares `__this` as its first param; §22.1.3.18 step 10 runs the
/// replacer as Call(replaceValue, undefined, «matched, position,
/// string»), so the kernel seeds argv[0] `undefined` and shifts the
/// user triple up by one (the sloppy prologue then answers
/// globalThis). A plain callback keeps the unshifted argv.
const CLOSURE_FLAGS_OFF: usize = 6;
const FLAG_CLOSURE_RECV_FIRST: u16 = 1 << 12;

/// argv slot count handed to the boxed entry. 3 live slots
/// (matched / position / whole) + undefined pad so a callback
/// declaring extra params stays in bounds.
const ARGV_SLOTS: usize = 16;

/// AnySlotTag values mirrored from `__torajs_anyv_box_from_pair`'s
/// contract (torajs-anyvalue/nanbox_encode.rs).
const TAG_I64: i64 = 2;
const TAG_HEAP: i64 = 4;
const TAG_UNDEF: i64 = 5;

/// Invoke the callback for one match. Returns the freshly-owned
/// `*mut Str` of `ToString(cb(matched, position, whole))`, or null
/// when the callback raised (pending throw is set; caller unwinds).
///
/// `matched` is consumed (dropped here after the call); `whole_str`
/// is borrowed (+0 — the callee never drops its params).
unsafe fn invoke_cb(
    closure: *mut c_void,
    matched: *mut c_void,
    position_cu: i64,
    whole_str: *const c_void,
) -> *mut c_void {
    let boxed_entry = unsafe {
        let slot = (closure as *const u8).add(CLOSURE_BOXED_ENTRY_OFF) as *const BoxedEntry;
        slot.read()
    };
    let undef = unsafe { __torajs_anyv_box_from_pair(TAG_UNDEF, 0) };
    let mut argv = [undef; ARGV_SLOTS];
    // Receiver-first shift (doc on FLAG_CLOSURE_RECV_FIRST above):
    // argv[0] stays the undefined receiver, user triple moves up one.
    let shift = usize::from(
        unsafe { ((closure as *const u8).add(CLOSURE_FLAGS_OFF) as *const u16).read() }
            & FLAG_CLOSURE_RECV_FIRST
            != 0,
    );
    argv[shift] = unsafe { __torajs_anyv_box_from_pair(TAG_HEAP, matched as i64) };
    argv[shift + 1] = unsafe { __torajs_anyv_box_from_pair(TAG_I64, position_cu) };
    argv[shift + 2] = unsafe { __torajs_anyv_box_from_pair(TAG_HEAP, whole_str as i64) };
    let ret = unsafe { boxed_entry(closure, argv.as_ptr(), 3 + shift as i64) };
    unsafe { __torajs_str_drop(matched) };
    if unsafe { __torajs_throw_check() } != 0 {
        return core::ptr::null_mut();
    }
    let r = unsafe { __torajs_anyv_to_str(ret) };
    // Release the callback's returned stake (the boxed wrapper hands
    // back an owned cell for heap-shaped returns; to_str took its
    // own fresh copy / inc above).
    if unsafe { __torajs_anyv_unbox_tag(ret) } == TAG_HEAP {
        let p = unsafe { __torajs_anyv_unbox_value(ret) } as *mut c_void;
        if !p.is_null() {
            unsafe { __torajs_value_drop_heap(p) };
        }
    }
    r
}

/// Shared walk: find matches of `needle` in `s`, invoke the cb per
/// match, splice `ToString(ret)` in place of the matched span.
/// `global=false` stops after the first match (replace);
/// `global=true` walks every non-overlapping match (replaceAll,
/// empty-needle inserts at every position per §22.1.3.19).
unsafe fn replace_fn_inner(
    s: *const c_void,
    needle: *const c_void,
    closure: *mut c_void,
    global: bool,
) -> *mut c_void {
    let (s_payload, _, s_latin1) = unsafe { str_view(s as *const u8) };
    let (n_payload, n_len_cu, n_latin1) = unsafe { str_view(needle as *const u8) };

    // A UTF-16 needle can't occur inside a Latin-1 haystack (except
    // the empty-needle insert case) — mirror __torajs_str_replace.
    if s_latin1 && !n_latin1 && n_len_cu != 0 {
        return alloc_str_copy(s_payload, s_latin1) as *mut c_void;
    }

    // Normalize haystack + needle to a common search encoding.
    let search_latin1 = s_latin1 && n_latin1;
    let stride: usize = if search_latin1 { 1 } else { 2 };
    let s_buf = coerce_payload(s_payload, s_latin1, search_latin1);
    let n_buf = coerce_payload(n_payload, n_latin1, search_latin1);
    let s_bytes = s_buf.as_ref();
    let n_bytes = n_buf.as_ref();
    let n_len = n_bytes.len();

    // Collect (byte_start, repl Str) pairs in search encoding.
    let mut hits: Vec<(usize, *mut c_void)> = Vec::new();
    let mut all_repl_latin1 = true;
    let mut pos = 0usize;
    while pos <= s_bytes.len() {
        let Some(rel) = find_first_with_stride(&s_bytes[pos..], n_bytes, stride) else {
            break;
        };
        let found = pos + rel;
        let matched = alloc_str_copy(&s_bytes[found..found + n_len], search_latin1);
        let r = unsafe { invoke_cb(closure, matched as *mut c_void, (found / stride) as i64, s) };
        if r.is_null() {
            // Pending throw — drop collected repls, hand back a
            // placeholder copy the caller's throw-check discards.
            for (_, rp) in &hits {
                unsafe { __torajs_str_drop(*rp) };
            }
            return alloc_str_copy(s_payload, s_latin1) as *mut c_void;
        }
        let (_, _, r_latin1) = unsafe { str_view(r as *const u8) };
        all_repl_latin1 &= r_latin1;
        hits.push((found, r));
        if !global {
            break;
        }
        // Advance past the match; an empty needle steps one code
        // unit so every position (incl. end-of-string) matches once.
        pos = found + if n_len == 0 { stride } else { n_len };
    }
    if hits.is_empty() {
        return alloc_str_copy(s_payload, s_latin1) as *mut c_void;
    }

    // Result encoding = widest of haystack + every replacement.
    let out_latin1 = search_latin1 && all_repl_latin1;
    let out_stride: usize = if out_latin1 { 1 } else { 2 };
    let widen = |b: usize| {
        if search_latin1 == out_latin1 {
            b
        } else {
            b * 2
        }
    };
    let s_out = coerce_payload(s_bytes, search_latin1, out_latin1);
    let s_out = s_out.as_ref();

    let mut total = 0usize;
    let mut r_bufs: Vec<PayloadBuf<'_>> = Vec::with_capacity(hits.len());
    for (_, r) in &hits {
        let (r_payload, _, r_latin1) = unsafe { str_view(*r as *const u8) };
        let rb = coerce_payload(r_payload, r_latin1, out_latin1);
        total += rb.as_ref().len();
        r_bufs.push(rb);
    }
    total += s_out.len() - hits.len() * widen(n_len);

    let result = build_result_with(
        (total / out_stride) as u32,
        total as u32,
        out_latin1,
        |dst| {
            let mut src_i = 0usize;
            let mut dst_i = 0usize;
            for ((found, _), rb) in hits.iter().zip(r_bufs.iter()) {
                let f = widen(*found);
                let pre = &s_out[src_i..f];
                let rb = rb.as_ref();
                splice_into(&mut dst[dst_i..dst_i + pre.len() + rb.len()], pre, rb, &[]);
                dst_i += pre.len() + rb.len();
                src_i = f + widen(n_len);
            }
            dst[dst_i..].copy_from_slice(&s_out[src_i..]);
        },
    );
    for (_, r) in &hits {
        unsafe { __torajs_str_drop(*r) };
    }
    result as *mut c_void
}

/// `s.replace(needle, cb)` — first occurrence only.
///
/// # Safety
///
/// `s` / `needle` are live Str blocks; `closure` is a live closure
/// heap block whose +32 slot holds the boxed entry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_fn(
    s: *const c_void,
    needle: *const c_void,
    closure: *mut c_void,
) -> *mut c_void {
    unsafe { replace_fn_inner(s, needle, closure, false) }
}

/// `s.replaceAll(needle, cb)` — every non-overlapping occurrence.
///
/// # Safety
///
/// Same constraints as [`__torajs_str_replace_fn`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_all_fn(
    s: *const c_void,
    needle: *const c_void,
    closure: *mut c_void,
) -> *mut c_void {
    unsafe { replace_fn_inner(s, needle, closure, true) }
}

/// `s.replace(needle, cb)` / `replaceAll` glue for a FUNCTIONAL
/// replaceValue that arrived through the any method dispatch —
/// §22.1.3.19 step 5's functionalReplace leg the REPLACE arm's
/// ToString twin (`__torajs_str_any_replace`) cannot serve. Operands
/// materialize through [`crate::method_any::owned_src`] exactly like
/// that twin (a Substr-view receiver would otherwise be misread as
/// an owned Str header); the closure invokes through its boxed
/// entry, whose pending-throw path hands back a placeholder the
/// caller's throw-check discards — never null, so the cell pointer
/// is the boxed return as-is.
///
/// # Safety
/// `s` / `needle` are live Str/Substr cells; `closure` is a live
/// closure heap block whose +32 slot holds the boxed entry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_replace_fn(
    s: *const u8,
    needle: *const u8,
    closure: *mut c_void,
    all: i64,
) -> u64 {
    unsafe {
        let (src, t0) = crate::method_any::owned_src(s);
        let (nd, t1) = crate::method_any::owned_src(needle);
        let out = replace_fn_inner(src.cast(), nd.cast(), closure, all != 0);
        crate::method_any::drop_tmp(t1);
        crate::method_any::drop_tmp(t0);
        out as u64
    }
}

/// The RegExp-pattern twin of [`__torajs_str_any_replace_fn`] —
/// §22.1.3.19 step 5's functionalReplace leg when the pattern is a
/// live RegExp cell AND the replaceValue's callable-ness was only
/// discoverable at runtime. The receiver materializes through
/// [`crate::method_any::owned_src`] (a Substr view would be misread
/// as an owned Str header by the regex kernel's `str_slice`); the
/// kernel reads the capture count off the cell itself and drives the
/// closure's boxed entry, so any callback arity lands on its spec
/// slots.
///
/// # Safety
/// `s` is a live Str/Substr cell; `re` is a live RegExp cell;
/// `closure` is a live closure heap block whose +32 slot holds the
/// boxed entry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_replace_regex_fn(
    s: *const u8,
    re: *const c_void,
    closure: *mut c_void,
    all: i64,
) -> u64 {
    unsafe {
        let (src, t0) = crate::method_any::owned_src(s);
        let out = __torajs_str_replace_regex_fn_boxed(src.cast(), re, closure, all);
        crate::method_any::drop_tmp(t0);
        out as u64
    }
}
