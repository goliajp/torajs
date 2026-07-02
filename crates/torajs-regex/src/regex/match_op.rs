//! `__torajs_str_match_regex` + `__torajs_regex_exec` +
//! `attach_groups` — port of `runtime_regex.c` L2257-2387, L2929-2988.

use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use super::{
    __torajs_arr_alloc, __torajs_arr_push, __torajs_arrprops_set, __torajs_dynobj_alloc,
    __torajs_dynobj_mark_null_proto, __torajs_dynobj_set, __torajs_rc_inc, __torajs_str_drop,
    ANY_HEAP, ANY_I64, ANY_UNDEF, RegExp, abort_unsupported, as_regex_mut, str_from_bytes,
    str_slice, str_slice_ascii_view,
};
use crate::node::{REGEX_MAX_CAPTURES, REGEX_SAVE_SLOTS};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{Workspace, match_anchor, search_from, search_from_with_ws};

/// `torajs_rc::FLAG_STATIC_LITERAL` — `1 << 2 = 4`. Mirrored as a
/// local const so torajs-regex doesn't need a torajs-rc cargo dep
/// for one flag bit. When set on a `HeapHeader.flags` field (offset
/// 6 on the universal layout), `__torajs_rc_inc` / `__torajs_rc_dec`
/// / `__torajs_str_drop` all no-op — the heap block is immortal
/// for the program's lifetime. Unit test
/// `torajs_rc::tests::flag_static_literal_value_locked` (lib.rs:657)
/// asserts the value stays `4`.
const FLAG_STATIC_LITERAL: u16 = 1 << 2;

/// Round 4 wire-back Phase B chunk 1 attacks #R-K1 + #R-K3 — cached
/// `"index"` / `"input"` / `"groups"` key Str slots. First call into
/// each `attach_*` allocates the Str via `str_from_bytes`, stamps
/// `FLAG_STATIC_LITERAL` on the header, and CASes the result into
/// the static slot; subsequent calls fast-path the atomic-load
/// (~1 ns vs ~32 ns alloc). `__torajs_str_drop` after `arrprops_set`
/// also no-ops on the flag, saving an additional ~3 ns per key.
/// Per-call gain: ~35 ns/key × 3 keys = ~105 ns/iter on the
/// `regex-wireback-minlit-100k` fixture; aligns the decomp
/// (`.claude/rfcs/20260625-perf-wire-back-decomp/decomposition.md`)
/// Top-N estimate.
///
/// **Race semantics**: under v0.2 single-mutator the CAS always
/// succeeds first time; once the multi-threaded substrate lands
/// (v1.0 biased-ARC, design-principles.md §6.2), a losing-CAS
/// thread cleans up its fresh alloc and adopts the winner. The
/// only observable cost of a race loss is the cleanup `str_drop`
/// path (~30 ns); bounded by thread count, occurs once per slot.
static K_INDEX: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static K_INPUT: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static K_GROUPS: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

/// Returns the cached immortal Str for `bytes`, allocating on first
/// call. Subsequent calls hit the atomic-load fast path. See
/// `K_INDEX` doc for race semantics + perf framing.
///
/// # Safety
///
/// `bytes` must be a stable literal (the first call's content seeds
/// the cache forever); all wireback call sites pass `b"..."` byte
/// literals.
unsafe fn cached_static_key(slot: &AtomicPtr<c_void>, bytes: &[u8]) -> *const c_void {
    let cached = slot.load(Ordering::Relaxed);
    if !cached.is_null() {
        return cached as *const c_void;
    }
    let fresh = unsafe { str_from_bytes(bytes) } as *mut c_void;
    // Stamp FLAG_STATIC_LITERAL so rc_inc / rc_dec / str_drop all
    // no-op on this header — the slot becomes immortal. HeapHeader
    // layout: refcount u32 @0, type_tag u16 @4, flags u16 @6
    // (see torajs_rc::lib.rs `pub struct HeapHeader`).
    unsafe {
        let flags_ptr = (fresh as *mut u8).add(6) as *mut u16;
        *flags_ptr |= FLAG_STATIC_LITERAL;
    }
    match slot.compare_exchange(
        core::ptr::null_mut(),
        fresh,
        Ordering::AcqRel,
        Ordering::Relaxed,
    ) {
        Ok(_) => fresh as *const c_void,
        Err(other) => {
            // CAS race lost — clear our immortal flag so str_drop
            // can genuinely free the fresh alloc (otherwise the
            // STATIC_LITERAL gate would short-circuit drop and leak
            // ~16 bytes per losing race).
            unsafe {
                let flags_ptr = (fresh as *mut u8).add(6) as *mut u16;
                *flags_ptr &= !FLAG_STATIC_LITERAL;
                __torajs_str_drop(fresh as *mut c_void);
            }
            other as *const c_void
        }
    }
}

/// Attach the spec §22.2.7.8 match-result properties `index` (match
/// start; UTF-8 byte domain — same domain note as lastIndex) and
/// `input` (the original subject Str, rc-shared, zero copy + zero
/// encoding drift) to `arr` via the arrprops side table. Insertion
/// order matters for the print face: index → input (→ groups, which
/// [`attach_groups`] appends right after).
///
/// Round 4 wire-back Phase B chunk 1 — `"index"` / `"input"` keys
/// route through `cached_static_key` (immortal static slot,
/// `FLAG_STATIC_LITERAL` stamped). Drops elided since the flag
/// makes them no-ops; `__torajs_rc_inc` on the cached key inside
/// `dynobj_set`'s fresh-insert branch is likewise a no-op.
///
/// # Safety
///
/// `arr` is a live tora Array; `str_ptr` a live Str outliving it.
pub unsafe fn attach_exec_props(arr: *mut c_void, str_ptr: *const c_void, index: i64) {
    unsafe {
        let k_index = cached_static_key(&K_INDEX, b"index");
        __torajs_arrprops_set(arr, k_index as *mut c_void, ANY_I64 as i64, index);
        let k_input = cached_static_key(&K_INPUT, b"input");
        __torajs_rc_inc(str_ptr as *mut c_void);
        __torajs_arrprops_set(arr, k_input as *mut c_void, ANY_HEAP as i64, str_ptr as i64);
    }
}

/// Build `.groups` dynobj from the named captures recorded on `re`
/// and the just-finished match's saves[]. Attaches the dict to
/// `arr` via the arrprops side table (so `arr.groups` resolves via
/// the standard Array.<unknown-prop> path). Without named captures
/// the property is still attached, as `undefined` (spec §22.2.7.8
/// step 24; bun prints `groups: undefined`).
///
/// # Safety
///
/// Calls cross-tier extern allocators; `arr` must be a live tora
/// Array. `re` and `s` must outlive the call.
pub unsafe fn attach_groups(
    arr: *mut c_void,
    re: &RegExp,
    s: &[u8],
    saves: &[i64; REGEX_SAVE_SLOTS],
) {
    if re.n_named_captures == 0 || re.capture_names.is_empty() {
        // Round 4 wire-back Phase B chunk 1 — cached static "groups" key.
        let outer_key = unsafe { cached_static_key(&K_GROUPS, b"groups") };
        unsafe {
            __torajs_arrprops_set(arr, outer_key as *mut c_void, ANY_UNDEF as i64, 0);
        }
        return;
    }
    // Spec: the groups object is created with a null prototype
    // (OrdinaryObjectCreate(null)); the flag drives the
    // `[Object: null prototype] ` print prefix.
    let mut groups = unsafe { __torajs_dynobj_alloc() };
    unsafe { __torajs_dynobj_mark_null_proto(groups) };
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    for i in 1..=n_cap_lim {
        let name = match re.capture_names.get(i) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let name_key = unsafe { str_from_bytes(name) };
        let gs = saves[2 * i];
        let ge = saves[2 * i + 1];
        if gs < 0 || ge < 0 {
            // Non-participating named group → undefined.
            unsafe {
                __torajs_dynobj_set(&mut groups, name_key as *mut c_void, ANY_UNDEF, 0);
            }
        } else {
            let val_str = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
            unsafe {
                __torajs_dynobj_set(
                    &mut groups,
                    name_key as *mut c_void,
                    ANY_HEAP,
                    val_str as u64,
                );
            }
        }
        unsafe { __torajs_str_drop(name_key as *mut c_void) };
    }
    // Round 4 wire-back Phase B chunk 1 — same cached static "groups" key.
    let outer_key = unsafe { cached_static_key(&K_GROUPS, b"groups") };
    unsafe {
        __torajs_arrprops_set(
            arr,
            outer_key as *mut c_void,
            ANY_HEAP as i64,
            groups as i64,
        );
    }
}

/// `s.match(re)` — Phase 1c shape: Array<Str>.
/// - Without `g`: `[match, group1, group2, ...]` + `.groups` for
///   named captures.
/// - With `g`: array of all non-overlapping match substrings (per
///   ES spec drops capture info).
/// - Empty matches bump pos by 1 to avoid infinite loops.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_match_regex(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
) -> *mut c_void {
    // Spec §22.2.7.5/.8: no match → null (both global and non-global).
    // The array is allocated lazily on first hit so the null path
    // never owns a stray +1 block.
    if re_ptr.is_null() || str_ptr.is_null() {
        return core::ptr::null_mut();
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    // chunk 7.7 v2 step 12 C2 Phase B-1 attack #A — zero-copy
    // ASCII Latin-1 view; transcode fallback for non-ASCII Latin-1
    // / UTF-16 payloads. `_s_owned` is alive for the whole fn body
    // so the view's `'_` lifetime is safely bounded by `str_ptr`'s
    // caller-held reference.
    // Round 3 Phase B attack #R-A1 — capture the ASCII-view discrimination
    // so the hot-loop call can pass it down to `search_from_with_ws`,
    // letting the u-flag continuation-byte gate short-circuit on
    // ASCII-only haystacks.
    let _s_owned: Vec<u8>;
    let (s, haystack_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(view) => (view, true),
        None => {
            _s_owned = unsafe { str_slice(str_ptr) };
            (&_s_owned, false)
        }
    };
    let slen = s.len() as i64;

    let global = re.flags & RE_FLAG_G != 0;
    let sticky = re.flags & RE_FLAG_Y != 0;

    // Lazy-init Workspace — the `!global && sticky` branch below uses
    // match_anchor which builds its own Workspace, so the outer
    // search_from_with_ws's Workspace is only needed in the `else`
    // branch. Sticky-only callers (typical `r.exec` / `str.match(r)`
    // with /y/) skip this ~50KB allocation entirely.
    let mut ws: Option<Workspace> = None;
    // Phase C-3 — bind the AOT-baked DFA view once outside the loop;
    // see match_all.rs for the rationale.
    // Round 3 Phase B sub-batch 7.2 — runtime-baked DFA fallback.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    let mut out: *mut c_void = core::ptr::null_mut();
    let mut pos: i64 = 0;
    while pos <= slen {
        let hit = if !global && sticky {
            let start = re.last_index.max(0);
            let h = if start > slen {
                None
            } else {
                match_anchor(&re.prog, &s, start, re.flags)
            };
            re.last_index = h.as_ref().map(|m| m.end).unwrap_or(0);
            h
        } else {
            // Round 5 attack #1 — Workspace materialisation is sunk
            // into the vm's `vm_match_at` call sites: DFA-resident +
            // no-save programs never touch it, so the whole 5-Vec
            // alloc/free cycle disappears from this hot loop.
            search_from_with_ws(
                &re.prog,
                &s,
                pos,
                re.flags,
                &mut ws,
                dfa_ref,
                haystack_is_ascii,
                true,
            )
        };
        let Some(m) = hit else { break };
        if out.is_null() {
            // Round 5 attack #6 — pre-size to the exec shape
            // (m[0] + capture groups) so the first push never takes
            // the cap0→4 grow (pool pop + 32B header memcpy + pool
            // push) that used to fire on every fresh result array.
            // The global branch pushes one segment per hit and grows
            // normally past the pre-size; still strictly better than
            // starting at 0.
            let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
            out = unsafe { __torajs_arr_alloc(1 + n_cap_lim as u64) };
        }
        let seg = unsafe { str_from_bytes(&s[m.start as usize..m.end as usize]) };
        out = unsafe { __torajs_arr_push(out, seg as i64) };
        if !global {
            // Append captures.
            let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
            for i in 1..=n_cap_lim {
                let gs = m.saves()[2 * i];
                let ge = m.saves()[2 * i + 1];
                if gs < 0 || ge < 0 {
                    out = unsafe { __torajs_arr_push(out, 0) };
                } else {
                    let grp = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
                    out = unsafe { __torajs_arr_push(out, grp as i64) };
                }
            }
            // Non-global match = exec shape (spec §22.2.7.8):
            // index / input / groups attach in print order.
            unsafe {
                attach_exec_props(out, str_ptr, m.start);
                attach_groups(out, re, &s, m.saves());
            }
            break;
        }
        // Empty match — bump pos by 1.
        pos = if m.end == m.start { m.end + 1 } else { m.end };
    }
    out
}

/// `re.exec(s)` — Phase 1c.1 spec-shape result `[match, g1, g2,
/// ...]` with named-capture `.groups` attached. Sticky / global
/// lastIndex bookkeeping matches spec §22.2.5.2.2.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_exec(
    re_ptr: *const c_void,
    str_ptr: *const c_void,
) -> *mut c_void {
    // Spec §22.2.7.2 step 9.a: no match → null. The array is
    // allocated only after a hit so the null path owns nothing.
    if re_ptr.is_null() || str_ptr.is_null() {
        return core::ptr::null_mut();
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    // chunk 7.7 v2 step 12 C2 Phase B-1 attack #A — zero-copy ASCII
    // Latin-1 view; mirrors `__torajs_str_match_regex` rationale.
    let _s_owned: Vec<u8>;
    let s: &[u8] = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(view) => view,
        None => {
            _s_owned = unsafe { str_slice(str_ptr) };
            &_s_owned
        }
    };
    let slen = s.len() as i64;

    let sticky = re.flags & RE_FLAG_Y != 0;
    let global = re.flags & RE_FLAG_G != 0;
    let track = sticky || global;
    let start = if track { re.last_index.max(0) } else { 0 };

    // Phase C-3 — single-shot exec hits the same baked-DFA short-circuit.
    // Round 3 Phase B sub-batch 7.2 — runtime-baked DFA fallback.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    let m = if track && start > slen {
        None
    } else if sticky {
        match_anchor(&re.prog, &s, start, re.flags)
    } else {
        search_from(&re.prog, &s, start, re.flags, dfa_ref)
    };
    let Some(m) = m else {
        if track {
            re.last_index = 0;
        }
        return core::ptr::null_mut();
    };
    if track {
        re.last_index = m.end;
    }
    // Round 5 attack #6 — pre-size to the exec shape (see match loop
    // above); the pushes below fill exactly 1 + n_cap_lim slots.
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    let mut out = unsafe { __torajs_arr_alloc(1 + n_cap_lim as u64) };
    let whole = unsafe { str_from_bytes(&s[m.start as usize..m.end as usize]) };
    out = unsafe { __torajs_arr_push(out, whole as i64) };
    for i in 1..=n_cap_lim {
        let gs = m.saves()[2 * i];
        let ge = m.saves()[2 * i + 1];
        if gs < 0 || ge < 0 {
            out = unsafe { __torajs_arr_push(out, 0) };
        } else {
            let grp = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
            out = unsafe { __torajs_arr_push(out, grp as i64) };
        }
    }
    unsafe {
        attach_exec_props(out, str_ptr, m.start);
        attach_groups(out, re, &s, m.saves());
    }
    out
}
