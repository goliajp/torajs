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
pub mod replace;
pub mod replace_fn;
pub mod replace_fn_dispatch;
pub mod split;
pub mod test_find;

use core::ffi::c_void;

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
    pub fn __torajs_rc_dec(p: *mut c_void) -> i32;
    pub fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    pub fn __torajs_str_drop(s: *mut c_void);
    pub fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    pub fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    pub fn __torajs_dynobj_alloc() -> *mut c_void;
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
pub unsafe fn str_slice<'a>(p: *const c_void) -> &'a [u8] {
    let len = unsafe { *((p as *const u8).add(8) as *const u64) };
    let data_ptr = unsafe { (p as *const u8).add(STR_HDR_SIZE) };
    unsafe { core::slice::from_raw_parts(data_ptr, len as usize) }
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
    let p = unsafe { __torajs_str_alloc_pooled(data.len() as u64) };
    if !data.is_empty() {
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), p.add(STR_HDR_SIZE), data.len());
        }
    }
    p
}

/// Abort with "not yet supported:" for a rejected regex. The
/// test262 runner classifies stderr starting with this prefix as
/// `incompatible` (subset boundary) — preserves tr-accepted parity
/// by keeping these cases out of the bug bucket.
pub fn abort_unsupported(re: &RegExp) {
    eprint!("not yet supported: regex feature not yet implemented in v0.2 #1.c — pattern: /");
    if !re.src_bytes.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&re.src_bytes));
    }
    eprintln!("/");
    std::process::exit(1);
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
