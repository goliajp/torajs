//! Public extern "C" surface — port of the heavy machinery in
//! `runtime_regex.c` L1352-3059 (P6.2-e mega-cutover, 2026-05-24).
//!
//! Submodules:
//!
//! - [`mod@self`] — RegExp / HeapHeader struct + Str/Arr/dynobj ABI
//!   constants + cross-tier extern declarations + [`as_regex`] /
//!   [`as_regex_mut`] lifting + [`abort_unsupported`].
//! - [`str_helpers`] — Str-payload transcoding + fresh-Str allocation
//!   (`str_slice_ascii_view` / `str_slice` / `str_from_bytes` /
//!   `str_from_bytes_ascii`). Re-exported at [`mod@self`] so
//!   sibling callers keep `use super::str_slice;` unchanged.
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

mod compile;
pub mod compile_any;
pub mod compile_aot;
pub mod escape;
pub mod lifecycle;
pub mod match_all;
pub mod match_indices;
pub mod match_op;
pub mod offset_map;
pub mod print;
pub mod replace;
pub mod replace_fn;
pub mod replace_fn_dispatch;
pub mod split;
pub mod static_keys;
pub mod str_helpers;
pub mod subclass;
pub mod test_find;

use core::ffi::c_void;

use alloc::vec::Vec;

use crate::program::Program;

pub(crate) use offset_map::{byte_to_utf16_units, utf16_units_to_byte};
pub use str_helpers::{str_from_bytes, str_from_bytes_ascii, str_slice, str_slice_ascii_view};

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

/// Byte offset of [`RegExp::props`] — mirrored by torajs-anyvalue
/// (`member_get_layout::REGEX_PROPS_OFF`) and torajs-meta, the same
/// narrow-ABI constant replication [`STR_HDR_SIZE`] uses.
pub const REGEX_PROPS_OFF: usize = 8;

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
    /// Lazy own-property bag — a RegExp instance is an ordinary
    /// object (§22.2.6) whose compiled program is INTERNAL state,
    /// so `re.zz = 1` is an ordinary own property and must land
    /// somewhere. NULL until the first non-`lastIndex` write; the
    /// same lazily-allocated DynObj shape Promise / wrapper /
    /// buffer cells carry. Sits directly after the header so the
    /// offset is a stable [`REGEX_PROPS_OFF`] the anyvalue tier can
    /// mirror without knowing anything about the rest.
    pub props: *mut c_void,
    pub flags: u8,
    /// Set when the parser couldn't accept the pattern. test/find
    /// silently return miss; the heavier surface (exec / match /
    /// replace*  / split / matchAll) aborts via
    /// [`abort_unsupported`] to land in the test262 runner's
    /// "incompatible" bucket rather than producing wrong matches.
    pub rejected: u8,
    /// Set once `Object.defineProperty(re, "lastIndex", {writable:
    /// false})` has run. §22.2.4.1 mints `lastIndex` as `{writable:
    /// true, enumerable: false, configurable: false}`, and a
    /// non-configurable data property may still be made read-only
    /// once — after which its value is frozen too (§10.1.6.3 step
    /// 4). One byte out of the padding that was already here; the
    /// cell's size and every other offset are unchanged.
    pub last_index_frozen: u8,
    pub _pad: [u8; 1],
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
    /// 0 in `compile`. An f64 because lastIndex is an ordinary data
    /// property — assignment stores the value uncoerced (`r.lastIndex
    /// = 2.9` reads back 2.9); ToLength happens at the consumption
    /// sites (`.max(0.0) as i64`, NaN → 0 via max). The numeric fast
    /// slot of the pair — valid only while [`Self::last_index_boxed`]
    /// is 0.
    pub last_index: f64,
    /// Non-numeric `lastIndex` overflow slot (RFC 20260722 L3b
    /// any-slot 刀) — §22.2.4.1 makes lastIndex an ordinary
    /// `{writable: true}` data property, so `re.lastIndex = "abc"`
    /// must store the string UNCOERCED and read it back identically
    /// (ToLength happens only at the §22.2.7.2 exec entry). 0 =
    /// numeric form (read [`Self::last_index`]); non-0 = a NaN-box
    /// AnyValue holding the assigned value verbatim (one owned rc
    /// for a heap cell — released by the numeric-write reset, the
    /// boxed re-store, and the RegExp drop). Every internal
    /// consumption site reads through [`Self::last_index_i64`] and
    /// every internal advance/reset writes through
    /// [`Self::set_last_index_num`], so the numeric-only fast path
    /// never touches the box externs (cargo-test stubs stay cold).
    pub last_index_boxed: u64,
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
    /// Round 5 attack str-replace #3 (2026-07-03) — reusable output
    /// buffer for the `replace` builders. Same single-threaded
    /// interior-mutability contract as `workspace_cache` above: the
    /// borrow is scoped to one `replace_inner` call, cleared (not
    /// freed) between calls so a hot `s.replace(re, r)` loop pays
    /// the Vec alloc/free once instead of per iteration.
    pub replace_out_cache: core::cell::UnsafeCell<alloc::vec::Vec<u8>>,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-2 (2026-06-24) —
    /// optional AOT-baked DFA metadata pointer. `None` when this
    /// `RegExp` came from `__torajs_regex_compile` (runtime literal /
    /// `new RegExp(...)` path); `Some(meta)` when it came from
    /// `__torajs_regex_compile_from_static_dfa`, in which case
    /// `meta.states_ptr` points at a `.rodata`-baked `[DfaState; N]`
    /// owned by the user binary and the four start indices are
    /// pre-computed too.
    ///
    /// `NonNull<BakedDfaMeta>` is the right shape here — the pointee
    /// lives in `.rodata` for the program's lifetime, so the slot
    /// requires no destructor work (`Box::from_raw` drop path in
    /// `__torajs_regex_drop` leaves the meta untouched), and the
    /// `None` discriminant keeps the original runtime path
    /// allocation-free.
    ///
    /// Consumed by [`RegExp::baked_dfa_view`] which stack-constructs
    /// a `DfaProgram` whose `states` is `DfaStates::Static(...)`
    /// over the baked slice — completely sidestepping the
    /// `UnsafeCell<Option<DfaProgram>>` borrow shape that produced
    /// the chunk-7.6 / chunk-7.7 SIGBUS (see `dfa_cache` doc above
    /// for the trail).
    pub baked_dfa: Option<core::ptr::NonNull<crate::dfa::BakedDfaMeta>>,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Round 3 Phase B sub-batch 7.2
    /// (2026-06-25) — runtime-baked DFA cache. The `__torajs_regex_compile`
    /// constructor eager-builds `build_dfa(&prog, flag_bits)` once when the
    /// program is DFA-eligible (`prog.can_dfa && prog_ops_dfa_safe(&prog)`)
    /// and stores the owned `DfaProgram` here, so the surface match path
    /// (`__torajs_str_match_regex` / `__torajs_regex_exec` / replace /
    /// matchAll / split) can borrow it through [`RegExp::dfa_runtime`]
    /// and never re-build per call. Closes the
    /// `vm::search_from_with_ws::dfa_built_local` per-call build path
    /// (deleted in sub-batch 7.3).
    ///
    /// Always `None` for the AOT path (`compile_aot.rs`) — those
    /// regexes have a `.rodata`-baked DFA reachable through
    /// [`RegExp::baked_dfa_view`] which is preferred over the runtime
    /// build in every surface caller (`baked_dfa_view().as_ref().or(re.dfa_runtime.as_ref())`).
    ///
    /// Storage shape rationale: plain `Option<DfaProgram>` (no
    /// `UnsafeCell`) because the slot is written once at ctor and
    /// immutable for the RegExp's lifetime. The interior-mutable
    /// cache shape that produced the chunk-7.6 SIGBUS family
    /// (`UnsafeCell<Option<DfaProgram>>`, also the deleted
    /// chunk-7.5 `OnceCell` variant) is structurally avoided here —
    /// no cross-call mutation, no aliasing-with-mutation, drop is the
    /// natural chain via `Box::from_raw(re_ptr as *mut RegExp)` in
    /// `__torajs_regex_drop`.
    pub dfa_runtime: Option<crate::dfa::DfaProgram>,
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
    pub fn __torajs_str_alloc_ascii(src: *const u8, len: i64) -> *mut u8;
    pub fn __torajs_str_drop(s: *mut c_void);
    pub fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    pub fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    /// `/d` match-indices — the `.indices` result and its `[start,
    /// end]` pairs are self-describing Array<Any> cells (NaN-box
    /// slots) so the Any lanes print / index them without an
    /// external elem-kind mark.
    pub fn __torajs_arr_alloc_any(cap: u64) -> *mut c_void;
    pub fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut c_void;
    pub fn __torajs_dynobj_alloc() -> *mut c_void;
    pub fn __torajs_dynobj_mark_null_proto(obj: *mut c_void);
    pub fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    pub fn __torajs_arrprops_set(arr_ptr: *mut c_void, key: *mut c_void, tag: i64, value: i64);
    /// Round 5 attack #4 — batch exec-triple attach (torajs-arr →
    /// torajs-dynobj `attach_exec3` fast path: no probe, no per-key
    /// rc_inc, const hashes).
    pub fn __torajs_arrprops_attach_exec3(
        arr_ptr: *mut c_void,
        k_index: *mut c_void,
        index_val: i64,
        k_input: *mut c_void,
        input_ptr: i64,
        k_groups: *mut c_void,
    );
    pub fn __torajs_throw_type_error(msg: *const u8);
    pub fn __torajs_throw_syntax_error(msg: *const u8);
    /// RFC 20260707 chunk 2 — the immortal `undefined` sentinel Str
    /// cell (torajs-str undef_sentinel.rs). Miss-capture slots push
    /// it instead of NULL so downstream print / eq / typeof can tell
    /// JS undefined from JS null.
    pub fn __torajs_str_undef() -> *mut u8;
    /// torajs-anyvalue — §7.1.4 ToNumber over a NaN-box AnyValue
    /// (lastIndex boxed-form consumption; numeric fast path never
    /// calls it).
    pub fn __torajs_anyv_to_number(v: u64) -> f64;
    /// torajs-value-drop — universal heap-value release (NaN-box-safe:
    /// the cell gate inside filters immediates, so callers pass the
    /// raw box bits unconditionally).
    pub fn __torajs_value_drop_heap(child: *mut c_void);
}

impl RegExp {
    /// §22.2.7.2-shaped `lastIndex` consumption — ToLength'd to a
    /// non-negative i64 (NaN → 0 via the max). Numeric form reads the
    /// f64 slot directly; the boxed form (a non-numeric value stored
    /// verbatim by the any-lane setter) coerces through ToNumber.
    pub fn last_index_i64(&self) -> i64 {
        if self.last_index_boxed != 0 {
            let n = unsafe { __torajs_anyv_to_number(self.last_index_boxed) };
            if n.is_nan() {
                return 0;
            }
            return n.max(0.0) as i64;
        }
        self.last_index.max(0.0) as i64
    }

    /// Internal numeric `lastIndex` write (exec/test advance +
    /// global/sticky miss reset) — releases any boxed-form value and
    /// returns the pair to numeric form.
    pub fn set_last_index_num(&mut self, n: f64) {
        if self.last_index_boxed != 0 {
            unsafe { __torajs_value_drop_heap(self.last_index_boxed as *mut c_void) };
            self.last_index_boxed = 0;
        }
        self.last_index = n;
    }
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
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-2 — view the
    /// AOT-baked DFA (when present) as an owned [`crate::dfa::DfaProgram`]
    /// whose `states` is `DfaStates::Static(&'static [DfaState])`
    /// over the `.rodata`-baked slice. Returns `None` when this
    /// `RegExp` came from the runtime `__torajs_regex_compile` path
    /// (no `baked_dfa` set) — callers fall back to the per-call
    /// `build_dfa` path.
    ///
    /// The returned `DfaProgram` is a thin stack-owned view: the
    /// state slice payload lives in `.rodata` for the program's
    /// lifetime, and `DfaProgram`'s 16-byte slice header (ptr +
    /// len) plus 16 bytes of `start*` fields fit in 56 bytes on
    /// aarch64 — well under the cost of one DFA build. Crucially,
    /// the view is **not** stored in any `UnsafeCell` slot: the
    /// caller `let`-binds it on the stack and borrows it through
    /// `Option::as_ref` directly into `search_from_with_ws`, so the
    /// borrow shape that produced the chunk-7.6 SIGBUS never
    /// reappears for AOT-eligible literal regexes.
    pub fn baked_dfa_view(&self) -> Option<crate::dfa::DfaProgram> {
        let meta_ptr = self.baked_dfa?;
        // SAFETY: `meta_ptr` was stored by
        // `__torajs_regex_compile_from_static_dfa` from the
        // `.rodata`-resident `BakedDfaMeta` the AOT pipeline
        // emitted; the pointee + the slice it indirects to live for
        // the program's lifetime, so the resulting `&'static`
        // borrows are sound.
        let meta = unsafe { meta_ptr.as_ref() };
        let states_slice: &'static [crate::dfa::DfaState] =
            unsafe { core::slice::from_raw_parts(meta.states_ptr, meta.states_len as usize) };
        // Round 3 Phase B attack #R-A2 — derive `all_starts_equal`
        // locally from the four baked start indices. Saves baking the
        // bool into `BakedDfaMeta` (already-stored fields suffice);
        // the ~5 ns compare runs once per `__torajs_str_match_regex`
        // call, not per-iter, so it's free against the saved 12
        // ns/iter inside the loop.
        let all_starts_equal = meta.start == meta.start_mid
            && meta.start == meta.start_mid_word
            && meta.start == meta.start_mid_nonword;
        Some(crate::dfa::DfaProgram {
            states: crate::dfa::DfaStates::Static(states_slice),
            start: meta.start,
            start_mid: meta.start_mid,
            start_mid_word: meta.start_mid_word,
            start_mid_nonword: meta.start_mid_nonword,
            all_starts_equal,
            // Round 3 Phase B attack #R-E — baked at host build time
            // by ssa_lower (see `try_bake_regex_dfa`); reading the
            // `.rodata` byte is free.
            any_accept_before_byte: meta.any_accept_before_byte,
            // Poisoned DFAs are never baked (`try_bake_regex_dfa`
            // refuses them), so a baked view is always sound.
            poisoned: false,
        })
    }
}
