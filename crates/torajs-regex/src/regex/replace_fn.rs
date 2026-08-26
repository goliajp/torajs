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

use super::replace_fn_dispatch::invoke_replace_cb;
use super::{
    __torajs_str_drop, __torajs_str_undef, __torajs_throw_type_error, abort_unsupported, as_regex,
    byte_to_utf16_units, str_from_bytes, str_slice,
};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{Workspace, match_anchor, save_slot, search_from_with_ws};

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

/// Build N capture Strs from saves[]. Each cap slot reads
/// `saves[2*(i+1)] / saves[2*(i+1)+1]` (group 0 = whole match is
/// handled separately).
///
/// A group that did not participate is `undefined` per §22.2.6.11
/// step 14.g, and it reaches the callback as the immortal undefined
/// sentinel — the same cell the `match` / `exec` array lanes already
/// push for exactly this case. The empty Str this used to build made
/// `"xz".replace(/x(y)?(z)/, (m, p1) => "<" + p1 + ">")` answer
/// `<>` where every engine answers `<undefined>`, silently. (The
/// `$1` expansion in the string-replacement lane is NOT this case:
/// §22.2.6.11's GetSubstitution really does substitute "" for an
/// undefined capture.)
///
/// Caller owns the returned Strs and must drop them; the sentinel
/// carries `FLAG_STATIC_LITERAL`, so its drop is a no-op.
///
/// # Safety
///
/// `s` must outlive the returned pointers; `out_caps` is sized for
/// at least `n_caps` entries (max 9).
unsafe fn build_capture_strs(
    n_caps: i64,
    saves: &[i64],
    s: &[u8],
    out_caps: &mut [*mut c_void; 9],
) {
    for i in 0..(n_caps as usize) {
        let gs = save_slot(saves, 2 * (i + 1));
        let ge = save_slot(saves, 2 * (i + 1) + 1);
        let p = if gs < 0 || ge < 0 {
            unsafe { __torajs_str_undef() }
        } else {
            unsafe { str_from_bytes(&s[gs as usize..ge as usize]) }
        };
        out_caps[i] = p as *mut c_void;
    }
}

/// Shared match walk — find/anchor per the pattern's flags, hand
/// each match to `invoke`, splice its returned Str over the matched
/// span. `invoke` receives `(match_str, saves, s_bytes, offset_cu)`
/// borrowed (the walk drops `match_str` after the call) and answers
/// the freshly-owned replacement Str; `Err(())` means the callback
/// raised a pending throw — the walk aborts and hands back a
/// placeholder copy the caller's throw-check discards. The two
/// invoke strategies differ only in dispatch: the typed lanes
/// transmute to the callback's statically-declared arity, the boxed
/// lane rides the closure's boxed entry.
unsafe fn replace_fn_inner(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    global: bool,
    mut invoke: impl FnMut(*mut c_void, &[i64], &[u8], i64) -> Result<*mut c_void, ()>,
) -> *mut c_void {
    if re_ptr.is_null() {
        let s = unsafe { str_slice(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re = unsafe { as_regex(re_ptr) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    let s = unsafe { str_slice(str_ptr) };
    let slen = s.len() as i64;

    // Lazy-init Workspace — sticky branch uses match_anchor's own
    // Workspace; outer ws only needed in non-sticky branch.
    let mut ws: Option<Workspace> = None;
    // Phase C-3 — baked DFA view bound once. See match_all.rs.
    // Round 3 Phase B sub-batch 7.2 — AOT then runtime-baked fallback.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    let mut out: Vec<u8> = Vec::with_capacity(s.len() + 16);
    let mut pos: i64 = 0;
    let sticky = re.flags & RE_FLAG_Y != 0;
    while pos <= slen {
        let m = if sticky {
            match_anchor(&re.prog, &s, pos, re.flags)
        } else {
            // Round 3 Phase B attack #R-A1 — replace_fn currently
            // routes through `str_slice` (transcodes to owned bytes),
            // so the ASCII-view shortcut isn't on this path. Pass
            // `false`; semantics preserved. Round 5 attack #1 —
            // Workspace materialises lazily inside the vm.
            search_from_with_ws(&re.prog, &s, pos, re.flags, &mut ws, dfa_ref, false, true)
        };
        let Some(m) = m else { break };
        out.extend_from_slice(&s[pos as usize..m.start as usize]);
        let match_str = unsafe { str_from_bytes(&s[m.start as usize..m.end as usize]) };
        let ret = invoke(
            match_str as *mut c_void,
            m.saves(),
            &s,
            // ES §22.1.3.18 — the cb offset arg is in UTF-16
            // code units of the input, not transcoded bytes.
            byte_to_utf16_units(&s, m.start, false),
        );
        unsafe { __torajs_str_drop(match_str as *mut c_void) };
        let ret_str = match ret {
            Ok(p) => p,
            Err(()) => {
                // Pending throw inside the callback — abandon the
                // walk; the placeholder copy is discarded by the
                // caller's throw check.
                let s = unsafe { str_slice(str_ptr) };
                return unsafe { str_from_bytes(&s) as *mut c_void };
            }
        };
        if !ret_str.is_null() {
            let ret_bytes = unsafe { str_slice(ret_str) };
            out.extend_from_slice(&ret_bytes);
            unsafe { __torajs_str_drop(ret_str) };
        }
        if m.end == m.start {
            if m.start < slen {
                out.push(s[m.start as usize]);
            }
            pos = m.end + 1;
        } else {
            pos = m.end;
        }
        if !global {
            break;
        }
    }
    // Clamp pos to s.len() — pos can overshoot after an empty match
    // at end-of-string (pos = m.end + 1).
    let tail = (pos as usize).min(s.len());
    out.extend_from_slice(&s[tail..]);
    unsafe { str_from_bytes(&out) as *mut c_void }
}

/// The typed lanes' invoke strategy — build the callback's declared
/// `n_caps` capture Strs and transmute-dispatch through
/// [`invoke_replace_cb`]. Never `Err`: the typed callback's pending
/// throw rides the caller's own throw check after the whole walk
/// (pre-existing typed-lane contract, unchanged).
unsafe fn typed_invoke(
    closure_env: *mut c_void,
    n_caps: i64,
    has_off_input: bool,
    str_ptr: *const c_void,
    m: *mut c_void,
    saves: &[i64],
    s: &[u8],
    off: i64,
) -> Result<*mut c_void, ()> {
    // Load fn_addr from env+8 — same closure ABI as
    // promise_then_closure.
    let fn_ptr = unsafe { *((closure_env as *mut u8).add(8) as *mut *mut c_void) };
    let mut caps: [*mut c_void; 9] = [core::ptr::null_mut(); 9];
    unsafe { build_capture_strs(n_caps, saves, s, &mut caps) };
    let ret_str = unsafe {
        invoke_replace_cb(
            n_caps,
            has_off_input,
            closure_env,
            fn_ptr,
            m,
            &caps,
            off,
            str_ptr as *mut c_void,
        )
    };
    for cap in caps.iter().take(n_caps as usize) {
        unsafe { __torajs_str_drop(*cap) };
    }
    Ok(ret_str)
}

/// The boxed lane's invoke strategy — §22.2.6.11 step 14.j's full
/// «matched, p1..pn, position, string» argv over the closure's boxed
/// entry, which reads exactly its callee's declared-param count of
/// slots. `n_caps` here is the PATTERN's own capture count (the
/// caller read it off the RegExp cell), so position/string land on
/// their spec slots for every callback arity; a non-participating
/// group boxes the real `undefined`, not a Str sentinel. The
/// callback's return takes ToString; its pending throw aborts the
/// walk via `Err`.
unsafe fn boxed_invoke(
    closure: *mut c_void,
    n_caps: usize,
    m: *mut c_void,
    saves: &[i64],
    s: &[u8],
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
        let live = shift + 1 + n_caps + 2;
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
                let p = str_from_bytes(&s[gs as usize..ge as usize]) as *mut c_void;
                cap_strs.push(p);
                argv.push(__torajs_anyv_box_from_pair(TAG_HEAP, p as i64));
            }
        }
        argv.push(__torajs_anyv_box_from_pair(TAG_I64, off));
        argv.push(__torajs_anyv_box_from_pair(TAG_HEAP, input as i64));
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
        let s = unsafe { str_slice(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let global = if !re_ptr.is_null() {
        unsafe { as_regex(re_ptr) }.flags & RE_FLAG_G != 0
    } else {
        false
    };
    unsafe {
        replace_fn_inner(str_ptr, re_ptr, global, |m, saves, s, off| {
            typed_invoke(
                closure_env,
                n_caps,
                has_off_input != 0,
                str_ptr,
                m,
                saves,
                s,
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
                b"String.prototype.replaceAll called with a non-global RegExp argument\0".as_ptr(),
            );
            let s = str_slice(str_ptr);
            return str_from_bytes(&s) as *mut c_void;
        }
        if closure_env.is_null() {
            let s = str_slice(str_ptr);
            return str_from_bytes(&s) as *mut c_void;
        }
        replace_fn_inner(
            str_ptr,
            re_ptr,
            /* global */ true,
            |m, saves, s, off| {
                typed_invoke(
                    closure_env,
                    n_caps,
                    has_off_input != 0,
                    str_ptr,
                    m,
                    saves,
                    s,
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
            let s = str_slice(str_ptr);
            return str_from_bytes(&s) as *mut c_void;
        }
        let re = as_regex(re_ptr);
        if all != 0 && re.flags & RE_FLAG_G == 0 {
            __torajs_throw_type_error(
                b"String.prototype.replaceAll called with a non-global RegExp argument\0".as_ptr(),
            );
            let s = str_slice(str_ptr);
            return str_from_bytes(&s) as *mut c_void;
        }
        let global = all != 0 || re.flags & RE_FLAG_G != 0;
        let n_caps = re.n_captures.max(0) as usize;
        replace_fn_inner(str_ptr, re_ptr, global, |m, saves, s, off| {
            boxed_invoke(closure, n_caps, m, saves, s, off, str_ptr)
        })
    }
}
