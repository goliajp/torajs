//! `__torajs_str_replace_regex` / `_all_regex` — port of
//! `runtime_regex.c` L2401-2600. The replacement text is expanded by
//! [`super::replace_subst::expand_repl`].

use alloc::vec::Vec;
use core::ffi::c_void;

use super::replace_subst::{ReplMatch, expand_repl};
use super::{
    __torajs_throw_type_error, RegExp, abort_unsupported, as_regex_mut, haystack, str_from_bytes,
    str_from_bytes_ascii, str_slice_ascii_view, str_units,
};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{match_anchor, search_from_with_ws};

fn replace_inner<'a>(
    re: &'a RegExp,
    s: &[u8],
    repl: &[u8],
    global: bool,
    haystack_is_ascii: bool,
) -> &'a [u8] {
    let slen = s.len() as i64;
    // V0.2 P14-S8 — reuse the per-RegExp cached Pike VM
    // workspace. The Pike VM's `step_id` counter increments
    // monotonically on every `vm_match_at` call, so stale
    // `visited[]` entries from prior runs auto-invalidate
    // (no clear pass needed). Only the cur/nxt thread lists
    // need explicit reset between invocations.
    // Round 5 attack #1 — pass the cache's `Option<Workspace>` down
    // as-is; the vm materialises it lazily at its `vm_match_at` call
    // sites (DFA-resident + no-save programs never touch it). No
    // explicit cur/nxt reset needed: `vm_match_at` clears `cur` and
    // resets the arena on entry, `nxt` per step.
    let ws_cell = re.workspace_cache.get();
    let ws = unsafe { &mut *ws_cell };
    // Round 5 attack str-replace #3 — reuse the per-RegExp output
    // buffer (alloc/free once per RegExp instead of per call). Same
    // single-threaded interior-mutability contract as
    // `workspace_cache` above; the borrow ends when the returned
    // slice is consumed by `str_from_bytes` in the extern wrapper.
    let out: &mut Vec<u8> = unsafe { &mut *re.replace_out_cache.get() };
    out.clear();
    out.reserve(s.len() + 16);
    let mut pos: i64 = 0;
    let sticky = re.flags & RE_FLAG_Y != 0;
    // Round 5 attack str-replace #1 (JSC `substituteBackreferences`
    // dollarless parity) — a replacement with no `$` provably never
    // reads captures, so the search can skip the 512-byte saves init
    // + the second-pass Pike VM capture extraction per hit.
    // It also decides how the replacement itself is written: with no
    // `$` it is a literal, so each hit appends it whole rather than
    // through `expand_repl`'s per-byte walk.
    let want_saves = repl.contains(&b'$');
    // Phase C-3 — bind the AOT-baked DFA view once outside the loop.
    // See match_all.rs for the rationale.
    // Round 3 Phase B sub-batch 7.2 — prefer the AOT-baked view, fall
    // back to the runtime-baked `RegExp.dfa_runtime` (eager-built at
    // `__torajs_regex_compile` ctor for DFA-eligible runtime literals).
    // Either way `dfa_ref: Option<&DfaProgram>` is the wire shape into
    // `vm::search_from_with_ws`; the vm's per-call `dfa_built_local`
    // path becomes dead in sub-batch 7.3.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    while pos <= slen {
        let m = if sticky {
            match_anchor(&re.prog, s, pos, re.flags)
        } else {
            search_from_with_ws(
                &re.prog,
                s,
                pos,
                re.flags,
                ws,
                dfa_ref,
                haystack_is_ascii,
                want_saves,
            )
        };
        let Some(m) = m else { break };
        out.extend_from_slice(&s[pos as usize..m.start as usize]);
        if want_saves {
            expand_repl(
                repl,
                ReplMatch {
                    s,
                    st: m.start,
                    en: m.end,
                    saves: m.saves(),
                    n_captures: re.n_captures,
                    capture_names: &re.capture_names,
                    has_named: re.n_named_captures > 0,
                },
                out,
            );
        } else {
            // Rotation 470 — `want_saves` IS `repl.contains(&b'$')`,
            // and with no `$` anywhere `expand_repl` walks the
            // replacement one `Vec::push` per byte and copies it
            // verbatim. The fact is already established above; act on
            // it and copy in one go. 23% of `str-replace-100k`'s
            // samples were that walk.
            out.extend_from_slice(repl);
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
    // pos may overshoot slen after an empty match at end-of-string
    // (pos = m.end + 1). Clamp before slicing — matches C's
    // `emit_bytes(s + pos, slen - pos)` which is a no-op when
    // slen - pos < 0 (n_bytes guard inside emit_bytes).
    let tail = (pos as usize).min(s.len());
    out.extend_from_slice(&s[tail..]);
    out
}

/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` / `repl_ptr`
/// are live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_regex(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    repl_ptr: *const c_void,
) -> *mut c_void {
    if re_ptr.is_null() {
        let s = unsafe { str_units(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    // §22.2.6.11 step 10 — see `replace_fn_inner`. A sticky-only
    // replace stores lastIndex through its one exec (§22.2.7.2 step
    // 12.a / 15), which a frozen lastIndex refuses the same way.
    let global = re.flags & RE_FLAG_G != 0;
    let sticky_only = !global && re.flags & RE_FLAG_Y != 0;
    if (global && !unsafe { super::lifecycle::global_replace_start(re) })
        || (sticky_only && unsafe { super::lifecycle::tracked_store_refuses(re) })
    {
        let s = unsafe { str_units(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re: &RegExp = re;
    // Round 5 attacks str-replace #2/#4 — borrow the Str payload
    // directly when it's pure-ASCII Latin-1 (no transcode, no owned
    // Vec alloc/free per call); fall back to the owned transcode for
    // non-ASCII. The borrow is bounded to this call frame, same
    // contract as the match/exec paths using `str_slice_ascii_view`.
    let s_owned;
    let (s, haystack_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(v) => (v, true),
        None => {
            s_owned = unsafe { haystack(re, str_ptr) };
            (&s_owned, false)
        }
    };
    let repl_owned;
    let (repl, repl_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(repl_ptr) } {
        Some(v) => (v, true),
        None => {
            repl_owned = unsafe { haystack(re, repl_ptr) };
            (&repl_owned, false)
        }
    };
    if haystack_is_ascii
        && repl_is_ascii
        && re.flags & RE_FLAG_Y == 0
        && !repl.contains(&b'$')
        && let Some(out) =
            unsafe { super::replace_splice::replace_ascii_splice(re, s, repl, global) }
    {
        return out;
    }
    let out = replace_inner(re, s, repl, global, haystack_is_ascii);
    // Round 5 attack str-replace #5 — the output is gap-copies of `s`
    // plus expansions of `repl`; when both are ASCII the result is
    // provably ASCII, so skip the encoding re-scan in the Str alloc.
    if haystack_is_ascii && repl_is_ascii {
        return unsafe { str_from_bytes_ascii(out) as *mut c_void };
    }
    unsafe { str_from_bytes(out) as *mut c_void }
}

/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` / `repl_ptr`
/// are live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_all_regex(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    repl_ptr: *const c_void,
) -> *mut c_void {
    if re_ptr.is_null() {
        let s = unsafe { str_units(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    // §22.1.5 — String.prototype.replaceAll throws a TypeError when the
    // RegExp searchValue lacks the `g` flag (mirrors matchAll §22.1.3.13).
    // The kernel records the pending throw; the caller's post-call
    // emit_throw_check propagates it and discards this returned string.
    if re.flags & RE_FLAG_G == 0 {
        unsafe {
            __torajs_throw_type_error(
                c"String.prototype.replaceAll called with a non-global RegExp argument"
                    .as_ptr()
                    .cast(),
            );
        }
        let s = unsafe { str_units(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    // §22.2.6.11 step 10 — the RegExp @@replace a replaceAll reaches.
    if !unsafe { super::lifecycle::global_replace_start(re) } {
        let s = unsafe { str_units(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re: &RegExp = re;
    // Round 5 attacks str-replace #2/#4 — same ASCII borrow shape as
    // `__torajs_str_replace_regex` above.
    let s_owned;
    let (s, haystack_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(v) => (v, true),
        None => {
            s_owned = unsafe { haystack(re, str_ptr) };
            (&s_owned, false)
        }
    };
    let repl_owned;
    let (repl, repl_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(repl_ptr) } {
        Some(v) => (v, true),
        None => {
            repl_owned = unsafe { haystack(re, repl_ptr) };
            (&repl_owned, false)
        }
    };
    // replace_all == replace with implicit `g` (ignore the regex's
    // own g flag — JS spec actually throws TypeError if no g, but
    // tr deferred that to v0.2 #1.c per the C port comment).
    let out = replace_inner(re, s, repl, /* global */ true, haystack_is_ascii);
    if haystack_is_ascii && repl_is_ascii {
        return unsafe { str_from_bytes_ascii(out) as *mut c_void };
    }
    unsafe { str_from_bytes(out) as *mut c_void }
}
