//! Public extern "C" surface — port of the heavy machinery in
//! `runtime_regex.c` L1352-3059 (P6.2-e mega-cutover, 2026-05-24).
//!
//! Submodules:
//!
//! - [`mod@self`] — RegExp / HeapHeader struct + Str/Arr/dynobj ABI
//!   constants + cross-tier extern declarations + shared helpers
//!   (`str_from_bytes`, `abort_unsupported`, `str_slice`).
//! - [`compile`] — `__torajs_regex_compile` driving parser → resolve →
//!   compile.
//! - [`lifecycle`] — `__torajs_regex_drop` / `get_source` /
//!   `get_last_index` / `set_last_index`.
//! - [`test_find`] — `__torajs_regex_test` / `__torajs_regex_find`.
//! - [`match_op`] — `__torajs_str_match_regex` + `attach_groups` +
//!   `__torajs_regex_exec`.
//! - [`match_all`] — `__torajs_str_match_all_regex`.
//! - [`replace`] — `expand_repl` + `__torajs_str_replace_regex` +
//!   `__torajs_str_replace_all_regex`.
//! - [`replace_fn`] — `invoke_replace_cb` + `build_capture_strs` +
//!   `__torajs_str_replace_regex_fn` + `__torajs_str_replace_all_regex_fn`.
//! - [`split`] — `__torajs_str_split_regex`.

pub mod compile;
pub mod lifecycle;
pub mod match_all;
pub mod match_op;
pub mod print;
pub mod replace;
pub mod replace_fn;
pub mod replace_fn_dispatch;
pub mod split;
pub mod test_find;

use core::ffi::c_void;

use alloc::vec::Vec;

use crate::program::Program;

/// Universal heap header (offset 0 of every refcounted heap object).
/// Mirrors `__torajs_heap_header_t` in runtime_str.c; `#[repr(C)]`
/// keeps `refcount` at offset 0 so [`extern_rc_dec`] (and the
/// runtime's tag-dispatch in `value_drop_heap`) reads the right
/// field regardless of which crate allocated the block.
#[repr(C)]
pub struct HeapHeader {
    pub refcount: u32,
    pub type_tag: u16,
    pub flags: u16,
}

/// `__TORAJS_TAG_REGEX` from runtime_regex.c L66.
pub const TAG_REGEX: u16 = 4;

/// Str heap layout — must match runtime_str.c.
pub const STR_HDR_SIZE: usize = 16;

/// `ANY` enum-tags used when storing a heap-shaped value into a
/// dynobj / arrprops bucket. Must match runtime_str.c's
/// `__TORAJS_ANY_HEAP` / `__TORAJS_ANY_UNDEF` (see runtime_regex.c
/// L2212-2213 where they're redeclared locally).
pub const ANY_I64: u64 = 2;
pub const ANY_HEAP: u64 = 4;
pub const ANY_UNDEF: u64 = 5;

/// In-memory RegExp object. The C VM is gone (P6.2-d ported it to
/// Rust), so layout below `header` is opaque to C — only the header
/// matters for type-tag dispatch + refcount.
#[repr(C)]
pub struct RegExp {
    pub header: HeapHeader,
    pub flags: u8,
    /// Set when the parser couldn't accept the pattern. test/find
    /// silently return miss; the heavier surface (exec / match /
    /// replace*  / split / matchAll) aborts via
    /// [`abort_unsupported`] to land in the test262 runner's
    /// "incompatible" bucket rather than producing wrong matches.
    pub rejected: u8,
    pub _pad: [u8; 2],
    pub n_captures: i32,
    pub prog: Program,
    /// Original pattern bytes — `re.source` returns these wrapped
    /// in a fresh `Str` via `get_source`.
    pub src_bytes: Vec<u8>,
    /// `(?<name>...)` capture name table. Index 0 unused; 1..=N is
    /// the capture index. Empty `Vec` for unnamed positional groups.
    pub capture_names: Vec<Vec<u8>>,
    /// Count of non-empty `capture_names` entries. Drives whether
    /// `attach_groups` runs at all (skip the dynobj alloc when 0).
    pub n_named_captures: i32,
    /// `RegExp.prototype.lastIndex` per ES spec §22.2.6.9. Mutated
    /// by exec / test / match / replace under sticky / global. Init
    /// 0 in `compile`.
    pub last_index: i64,
    /// V0.2 P14-S8 — per-RegExp Pike VM workspace cache. The
    /// pre-S8 `__torajs_str_replace_regex` (and matchAll / split-
    /// regex / etc) called `Workspace::for_program(&prog)` on
    /// every entry — 4 Vec allocations per call (2 ThreadList
    /// `Vec::with_capacity` + 2 VisitedTable `vec![0u32; n]` with
    /// zero-init). With LICM (P14-S1) hoisting the RegExp object
    /// to the enclosing fn's entry block, a `for i in 0..100_000`
    /// loop over `s.replace(re, ...)` shares one RegExp instance
    /// — so the workspace can be allocated once at first use
    /// (lazy) and reused for every subsequent search. The
    /// `step_id` counter that gates the visited-table dedup is
    /// monotonically bumped per `vm_match_at` call, so stale
    /// `visited[]` entries from the previous run auto-invalidate
    /// without an explicit clear pass.
    ///
    /// Single-threaded by construction. v0.2's no-multi-thread
    /// substrate (§6.2 of the design principles) guarantees no
    /// concurrent share. When the biased-ARC multi-thread
    /// transition lands (v1.0+), this becomes thread-local-indexed
    /// by `owner_thread_id` and the shared-RegExp transition
    /// re-allocates the cache through the cross-thread atomic
    /// path.
    pub workspace_cache: core::cell::UnsafeCell<Option<crate::vm::Workspace>>,
    /// V0.2 P14 chunk 7.7 — per-RegExp lazy DFA cache. Mirrors the
    /// `workspace_cache` UnsafeCell pattern at the same struct level,
    /// hosting the cache on the owning heap allocation rather than on
    /// the nested `prog: Program` field. Chunks 6+7 hosted this on
    /// `Program::dfa_cache` and got 30%-flaky SIGBUS in
    /// `regex-021-test-lastindex` from the cross-call `&DfaProgram`
    /// borrow; chunk 7.5's `OnceCell` swap kept the 2/10 SIGBUS, so
    /// chunk 7 v3 shipped per-call inline `build_dfa(prog, flags)` and
    /// the cache was deleted as dead substrate. Chunk 7.6 audit
    /// (`.claude/rfcs/20260623-chunk-7.6-cache-ub-audit/diagnosis.md`)
    /// ruled the workspace-cache layout safe and Option A (move cache
    /// to RegExp) the fix direction; chunk 7.7 implements that move.
    ///
    /// Built lazily by [`RegExp::get_or_build_dfa`]; consumed by
    /// `vm::search_from_with_ws` via the `dfa_cached` parameter when
    /// the caller passes the cached ref through.
    pub dfa_cache: core::cell::UnsafeCell<Option<crate::dfa::DfaProgram>>,
}

// ---- Cross-tier extern declarations ----
// Resolved at `tr build` link time against:
//   - libtorajs_rc.a          (rc_dec)
//   - libtorajs_str.a         (str_alloc_pooled, str_drop)
//   - libtorajs_arr.a         (arr_alloc, arr_push, arrprops_set)
//   - libtorajs_dynobj.a      (dynobj_alloc, dynobj_set)
//   - libtorajs_throw.a       (throw_type_error)
// During `cargo test` these are stubbed (see lib.rs test stubs at
// the crate root).

unsafe extern "C" {
    pub fn __torajs_rc_inc(p: *mut c_void);
    pub fn __torajs_rc_dec(p: *mut c_void) -> i32;
    pub fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    /// P11.1-S2.1 canonical-encoding alloc — scans the input UTF-8
    /// byte stream and picks Latin-1 / UTF-16 LE storage so the
    /// resulting Str round-trips correctly through downstream
    /// print / concat / search.
    pub fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    pub fn __torajs_str_drop(s: *mut c_void);
    pub fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    pub fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    pub fn __torajs_dynobj_alloc() -> *mut c_void;
    pub fn __torajs_dynobj_mark_null_proto(obj: *mut c_void);
    pub fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    pub fn __torajs_arrprops_set(arr_ptr: *mut c_void, key: *mut c_void, tag: i64, value: i64);
    pub fn __torajs_throw_type_error(msg: *const u8);
}

// ---- Shared helpers ----

/// View a tora `Str *` as a `&[u8]` of its payload. Safety: `p`
/// must point at a live Str whose header is well-formed and whose
/// payload remains valid for the borrow's lifetime.
///
/// # Safety
///
/// Caller guarantees that `p` is non-null, well-aligned, and
/// references a tora-Str-layout block whose bytes outlive `'a`.
pub unsafe fn str_slice(p: *const c_void) -> Vec<u8> {
    // P11.1-S2.1 — Str payload is encoded (Latin-1 or UTF-16 LE)
    // rather than raw UTF-8 bytes. The regex engine operates on
    // UTF-8 byte streams, so haystacks / patterns transcode here
    // before reaching the matching code. Returns an owned Vec so
    // call sites uniformly hold the buffer for the duration of
    // the match; ASCII-only Latin-1 payloads still allocate +
    // `to_vec` once each match, but the regex hot loops dominate
    // that cost so the simplicity wins. (A `Cow` variant was
    // explored but every downstream consumer ends up owning the
    // buffer anyway via VM iteration / replace builders.)
    let s = p as *const u8;
    let length = unsafe { *(s.add(8) as *const u32) };
    let flags = unsafe { *(s.add(6) as *const u16) };
    let is_latin1 = (flags & 0x0002) != 0;
    let byte_cnt = if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    };
    let payload = unsafe { core::slice::from_raw_parts(s.add(STR_HDR_SIZE), byte_cnt) };
    if is_latin1 && payload.iter().all(|&b| b <= 0x7F) {
        return payload.to_vec();
    }
    if is_latin1 {
        let mut out = Vec::with_capacity(payload.len() * 2);
        for &b in payload {
            if b <= 0x7F {
                out.push(b);
            } else {
                out.push(0xC0 | (b >> 6));
                out.push(0x80 | (b & 0x3F));
            }
        }
        return out;
    }
    let mut out = Vec::with_capacity((length as usize) * 3);
    let mut i = 0usize;
    while i + 1 < payload.len() {
        let cu = u16::from_le_bytes([payload[i], payload[i + 1]]) as u32;
        let cp = if (0xD800..=0xDBFF).contains(&cu) && i + 3 < payload.len() {
            let lo = u16::from_le_bytes([payload[i + 2], payload[i + 3]]) as u32;
            if (0xDC00..=0xDFFF).contains(&lo) {
                i += 4;
                0x10000 + ((cu - 0xD800) << 10) + (lo - 0xDC00)
            } else {
                i += 2;
                cu
            }
        } else {
            i += 2;
            cu
        };
        if cp <= 0x7F {
            out.push(cp as u8);
        } else if cp <= 0x7FF {
            out.push((0xC0 | (cp >> 6)) as u8);
            out.push((0x80 | (cp & 0x3F)) as u8);
        } else if cp <= 0xFFFF {
            out.push((0xE0 | (cp >> 12)) as u8);
            out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
            out.push((0x80 | (cp & 0x3F)) as u8);
        } else {
            out.push((0xF0 | (cp >> 18)) as u8);
            out.push((0x80 | ((cp >> 12) & 0x3F)) as u8);
            out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
            out.push((0x80 | (cp & 0x3F)) as u8);
        }
    }
    out
}

/// Allocate a fresh refcounted `Str` of `data.len()` bytes via the
/// small-Str pool path; copy `data` into the payload. Returns the
/// pool-aligned Str pointer (rc=1).
///
/// # Safety
///
/// Calls into the C `__torajs_str_alloc_pooled` allocator (link-
/// time). The returned pointer must be released via
/// `__torajs_str_drop`.
pub unsafe fn str_from_bytes(data: &[u8]) -> *mut u8 {
    // P11.1-S2.1 — route through the canonical-encoding alloc so
    // returned match-fragment Strs carry the correct encoding flag
    // and downstream print / concat see them with consistent
    // semantics. Input `data` is a UTF-8 byte slice (either the
    // already-transcoded haystack returned by `str_slice`, or a
    // replacement-builder buffer that the regex engine assembled
    // codepoint-by-codepoint).
    unsafe { __torajs_str_alloc(data.as_ptr(), data.len() as i64) }
}

/// Abort with "not yet supported:" for a rejected regex. The
/// test262 runner classifies stderr starting with this prefix as
/// `incompatible` (subset boundary) — preserves tr-accepted parity
/// by keeping these cases out of the bug bucket.
pub fn abort_unsupported(re: &RegExp) {
    crate::write_stderr(
        "not yet supported: regex feature not yet implemented in v0.2 #1.c — pattern: /".as_bytes(),
    );
    if !re.src_bytes.is_empty() {
        // stderr is a byte stream — write the raw source bytes directly,
        // no utf8 re-encode (drops the String::from_utf8_lossy alloc).
        crate::write_stderr(&re.src_bytes);
    }
    crate::write_stderr(b"/\n");
    torajs_syscall::exit(1);
}

/// Lift a `*const c_void` RegExp pointer to a `&RegExp`. Safety:
/// pointer must be non-null + must originate from
/// [`__torajs_regex_compile`](compile::__torajs_regex_compile).
///
/// # Safety
///
/// Caller guarantees `p` is non-null and produced by
/// `__torajs_regex_compile`; the borrow must not outlive the
/// regex's refcount.
pub unsafe fn as_regex<'a>(p: *const c_void) -> &'a RegExp {
    unsafe { &*(p as *const RegExp) }
}

/// Lift a `*mut c_void` RegExp pointer to a `&mut RegExp` (for
/// `last_index` mutation under sticky / global).
///
/// # Safety
///
/// Caller guarantees `p` is non-null and produced by
/// `__torajs_regex_compile`; the borrow must not outlive the
/// regex's refcount + nothing else holds a `&RegExp` alias.
pub unsafe fn as_regex_mut<'a>(p: *mut c_void) -> &'a mut RegExp {
    unsafe { &mut *(p as *mut RegExp) }
}

impl RegExp {
    /// V0.2 P14 chunk 7.7 — lazy-build the per-RegExp DFA. Returns
    /// `Some(&DfaProgram)` when the program is DFA-eligible (no
    /// backref / lookaround) and the underlying NFA opcodes are safe
    /// (`crate::dfa::prog_ops_dfa_safe`); `None` otherwise. Eligibility
    /// is recomputed on each call (cheap O(insts.len()) scan); the DFA
    /// itself is built once on the first eligible call and cached.
    ///
    /// Mirrors [`RegExp::workspace_cache`] (`UnsafeCell<Option<T>>`
    /// lazy slot) at the same struct level. Chunk 6+7 hosted the
    /// cache on `Program::dfa_cache` and got 30%-flaky SIGBUS in
    /// `regex-021-test-lastindex` — chunk 7.6 audit RFC ruled the
    /// workspace_cache layout safe by comparison (`workspace_cache` 0
    /// regression; same `UnsafeCell<Option<T>>` pattern), so the move
    /// to `RegExp::dfa_cache` here is Option A from the audit
    /// (`.claude/rfcs/20260623-chunk-7.6-cache-ub-audit/diagnosis.md`).
    ///
    /// Build cost = BFS subset construction over 256 input bytes per
    /// state; amortised across the RegExp's lifetime via the cached
    /// slot. Multi-thread (v1.0+) reroutes through the biased-ARC
    /// share-transition path; the field doc on
    /// [`RegExp::workspace_cache`] explains the convention this
    /// follows.
    pub fn get_or_build_dfa(&self) -> Option<&crate::dfa::DfaProgram> {
        if !self.prog.can_dfa {
            return None;
        }
        if !crate::dfa::prog_ops_dfa_safe(&self.prog) {
            return None;
        }
        // SAFETY: v0.2 single-mutator substrate (§6.2 of the design
        // principles) guarantees no concurrent access; mirrors the
        // workspace_cache UnsafeCell access pattern in
        // `regex/replace.rs::replace_inner`.
        let cell_ptr = self.dfa_cache.get();
        let opt: &mut Option<crate::dfa::DfaProgram> = unsafe { &mut *cell_ptr };
        if opt.is_none() {
            *opt = Some(crate::dfa::build_dfa(&self.prog, self.flags));
        }
        opt.as_ref()
    }
}
