//! `__torajs_str_replace_regex_fn` / `_all_regex_fn` callback form —
//! port of `runtime_regex.c` L2602-2854.
//!
//! Per match, runtime constructs a temp Str for the matched bytes
//! plus N temp Strs for capture groups and invokes the user cb
//! through [`super::replace_fn_dispatch::invoke_replace_cb`].
//! `has_off_input` switches between the basic `(env, m, g1..gN)`
//! and the spec-full `(env, m, g1..gN, offset_i64, input_str)` cb
//! arities (ES §22.1.3.18).

use alloc::vec::Vec;
use core::ffi::c_void;

use super::replace_fn_dispatch::invoke_replace_cb;
use super::{
    __torajs_str_drop, __torajs_str_undef, __torajs_throw_type_error, abort_unsupported, as_regex,
    byte_to_utf16_units, str_from_bytes, str_slice,
};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{Workspace, match_anchor, save_slot, search_from_with_ws};

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

unsafe fn replace_fn_inner(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    closure_env: *mut c_void,
    n_caps: i64,
    has_off_input: bool,
    global: bool,
) -> *mut c_void {
    if re_ptr.is_null() || closure_env.is_null() {
        let s = unsafe { str_slice(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re = unsafe { as_regex(re_ptr) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    let s = unsafe { str_slice(str_ptr) };
    let slen = s.len() as i64;
    // Load fn_addr from env+8 — same closure ABI as
    // promise_then_closure.
    let fn_ptr = unsafe { *((closure_env as *mut u8).add(8) as *mut *mut c_void) };

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
        let mut caps: [*mut c_void; 9] = [core::ptr::null_mut(); 9];
        unsafe { build_capture_strs(n_caps, m.saves(), &s, &mut caps) };
        let ret_str = unsafe {
            invoke_replace_cb(
                n_caps,
                has_off_input,
                closure_env,
                fn_ptr,
                match_str as *mut c_void,
                &caps,
                // ES §22.1.3.18 — the cb offset arg is in UTF-16
                // code units of the input, not transcoded bytes.
                byte_to_utf16_units(&s, m.start, false),
                str_ptr as *mut c_void,
            )
        };
        unsafe { __torajs_str_drop(match_str as *mut c_void) };
        for cap in caps.iter().take(n_caps as usize) {
            unsafe { __torajs_str_drop(*cap) };
        }
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
    let global = if !re_ptr.is_null() {
        unsafe { as_regex(re_ptr) }.flags & RE_FLAG_G != 0
    } else {
        false
    };
    unsafe {
        replace_fn_inner(
            str_ptr,
            re_ptr,
            closure_env,
            n_caps,
            has_off_input != 0,
            global,
        )
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
        replace_fn_inner(
            str_ptr,
            re_ptr,
            closure_env,
            n_caps,
            has_off_input != 0,
            /* global */ true,
        )
    }
}
