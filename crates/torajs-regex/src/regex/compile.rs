//! `__torajs_regex_compile` — port of `runtime_regex.c` L1445-1512.
//!
//! Compiles a pattern met at run time through
//! [`crate::compiled::compile_pattern`] (flags, parse, backreference
//! resolution, capture names, bytecode; a rejected pattern becomes a
//! never-match stub with `rejected = 1` so the surface methods report
//! it rather than match wrongly), then builds the RegExp cell around
//! the result with [`new_cell`], which every construction path shares.
//!
//! The resulting `RegExp` is heap-allocated via
//! `Box::into_raw(Box::new(...))` so the universal header sits at
//! offset 0 of the allocation block. `__torajs_value_drop_heap`'s
//! tag dispatch on `header.type_tag = TAG_REGEX` routes drops back
//! to [`super::lifecycle::__torajs_regex_drop`].

use alloc::{boxed::Box, vec::Vec};
use core::ffi::c_void;
use core::ptr::NonNull;

use super::{__torajs_throw_syntax_error, HeapHeader, MatcherFn, RegExp, TAG_REGEX, str_units};
use crate::compiled::{Compiled, compile_pattern};
use crate::dfa::BakedDfaMeta;
use crate::parser::{
    RE_FLAG_D, RE_FLAG_G, RE_FLAG_I, RE_FLAG_M, RE_FLAG_S, RE_FLAG_U, RE_FLAG_V, RE_FLAG_Y,
};
use crate::program::Program;

/// # Safety
///
/// `pattern_str` and `flags_str` must point at live `Str *` heap
/// objects (refcount > 0; well-formed header). The returned
/// pointer is heap-allocated with refcount = 1 and type_tag =
/// `TAG_REGEX`; release with `__torajs_regex_drop`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_compile(
    pattern_str: *const c_void,
    flags_str: *const c_void,
) -> *mut c_void {
    let pat = unsafe { str_units(pattern_str) };
    let fl = unsafe { str_units(flags_str) };
    compile_bytes(pat, fl)
}

/// Bytes-level compile pipeline — the extern face above decodes the
/// Str cells first; the subclass `super(pattern)` kernel (RFC
/// 20260730 blade 2) enters here with bytes it already owns.
pub(crate) fn compile_bytes(pat: Vec<u8>, fl: Vec<u8>) -> *mut c_void {
    let compiled = compile_pattern(&pat, &fl);
    // `src_bytes` keeps the code-unit form (flag-independent — a
    // `new RegExp(re, "u")` re-derives from it)
    new_cell(compiled, pat, None, Matchers::NONE)
}

/// The one-pass matchers the compiler synthesized for a literal, one
/// per payload encoding (`None` where it declined an encoding).
pub(crate) struct Matchers {
    pub latin1: Option<MatcherFn>,
    pub utf16: Option<MatcherFn>,
}

impl Matchers {
    pub const NONE: Matchers = Matchers {
        latin1: None,
        utf16: None,
    };
}

/// The RegExp cell around `c`: refcount 1, no own property, `lastIndex`
/// +0. A `baked` DFA table binds only when the program it runs is
/// DFA-serveable (the searches trust a bound table without asking
/// again, `vm::search::resolve_dfa`); otherwise the DFA is decided
/// here once, by [`runtime_dfa`].
pub(crate) fn new_cell(
    mut c: Compiled,
    src_bytes: Vec<u8>,
    baked: Option<NonNull<BakedDfaMeta>>,
    matchers: Matchers,
) -> *mut c_void {
    let serveable = c.rejected == 0 && c.prog.can_dfa && crate::dfa::prog_ops_dfa_safe(&c.prog);
    let baked_dfa = baked.filter(|_| serveable);
    let dfa_runtime = if c.rejected == 0 && baked_dfa.is_none() {
        runtime_dfa(&mut c.prog, c.flags)
    } else {
        None
    };
    let matcher_stride = if matchers.latin1.is_some() || matchers.utf16.is_some() {
        crate::vm::saves_arena::detect_stride(&c.prog) as u32
    } else {
        0
    };
    let re = Box::new(RegExp {
        header: HeapHeader {
            refcount: 1,
            type_tag: TAG_REGEX,
            flags: 0,
        },
        // No own property has been written yet — the bag is
        // minted by the first `re.zz = 1`, never here.
        props: core::ptr::null_mut(),
        flags: c.flags,
        rejected: c.rejected,
        last_index_frozen: 0,
        _pad: [0; 1],
        n_captures: c.n_captures,
        prog: c.prog,
        src_bytes,
        capture_names: c.capture_names,
        n_named_captures: c.n_named_captures,
        last_index: 0.0,
        last_index_boxed: 0,
        // V0.2 P14-S8 — lazy-init Pike VM workspace cache.
        workspace_cache: core::cell::UnsafeCell::new(None),
        replace_out_cache: core::cell::UnsafeCell::new(alloc::vec::Vec::new()),
        baked_dfa,
        matcher_latin1: matchers.latin1,
        matcher_utf16: matchers.utf16,
        matcher_stride,
        dfa_runtime,
    });
    Box::into_raw(re) as *mut c_void
}

/// The DFA a runtime-compiled RegExp searches with, decided once at
/// construction (Round 3 Phase B sub-batch 7.2): an eager table when it
/// fits the cap, else a lazily filled one parked in `prog.dfa_lazy`, else
/// none.
///
/// Storage `Option<DfaProgram>` (no `UnsafeCell`) — populated once here,
/// immutable for the RegExp's lifetime; the interior-mutable cache shapes
/// of chunk 7.6 SIGBUSed. A hot `for i { s.match(re) }` over a hoisted
/// literal pays the build once instead of per iteration.
fn runtime_dfa(prog: &mut Program, flag_bits: u8) -> Option<crate::dfa::DfaProgram> {
    if !(prog.can_dfa && crate::dfa::prog_ops_dfa_safe(prog)) {
        return None;
    }
    // RFC 20260711 chunk B — a poisoned build (unserveable K-PROPERTY
    // shape) is discarded and the Pike VM serves. Poison is a property
    // of the program, so the gate closes here for good. Poison met past
    // the cap disables the lazy table at the search that meets it.
    match crate::dfa::build_dfa_capped(prog, flag_bits) {
        crate::dfa::CappedDfa::Table(dfa) if dfa.poisoned => {
            prog.can_dfa = false;
            None
        }
        crate::dfa::CappedDfa::Table(dfa) => Some(dfa),
        crate::dfa::CappedDfa::Over(part) => {
            match crate::dfa::LazySlot::from_partial(prog, flag_bits, part) {
                Some(slot) => prog.dfa_lazy = slot,
                None => prog.can_dfa = false,
            }
            None
        }
    }
}

/// `new RegExp(pattern, flags)` entry — wraps [`__torajs_regex_compile`]
/// and records a catchable `SyntaxError` on the TLS pending-throw
/// slot when the parser rejects the pattern. The lowering caller
/// (`lower_regexp` in `ssa_lower_new.rs`) emits an
/// `emit_throw_check_owned` right after so the throw propagates as a
/// real JS exception rather than the pre-existing silent never-match
/// stub.
///
/// Literal regex `/pat/flags` in `ssa_lower_lit.rs` intentionally
/// keeps calling the plain [`__torajs_regex_compile`] (silent stub);
/// literal-time SyntaxError propagation needs an entry-block-safe
/// throw-check shape (the literal call is hoisted to `BlockId(0)` for
/// LICM) and is deferred to L3b.
///
/// # Safety
///
/// Same contract as [`__torajs_regex_compile`]: `pattern_str` and
/// `flags_str` must point at live `Str *` heap objects with valid
/// headers. The returned pointer is heap-allocated refcount=1 with
/// `type_tag = TAG_REGEX`; release with `__torajs_regex_drop`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_compile_or_throw(
    pattern_str: *const c_void,
    flags_str: *const c_void,
) -> *mut c_void {
    let raw = unsafe { __torajs_regex_compile(pattern_str, flags_str) };
    unsafe { throw_if_rejected(raw) };
    raw
}

/// Shared rejected-pattern tail — `regex_compile` always returns a
/// non-null RegExp (stub or valid); peek at the rejected byte +
/// src/flags to record a spec-shaped SyntaxError when the parser
/// rejected the pattern. Message shape mirrors bun/JSC: `Invalid
/// regular expression: /<pattern>/<flags>`. The parser tracks a bool
/// err flag only (no per-site reason), so tr's message omits the
/// ": <reason>" trailer — sufficient to distinguish from other error
/// kinds in user `try/catch` blocks. Flag-byte → letter mapping is
/// [`flag_letters`] (ES §22.2.6.4 order — a deterministic canonical
/// spelling for tests).
///
/// # Safety
/// `raw` is NULL or a live RegExp cell.
pub(crate) unsafe fn throw_if_rejected(raw: *mut c_void) {
    if raw.is_null() {
        return;
    }
    let re = unsafe { &*(raw as *const RegExp) };
    if re.rejected == 0 {
        return;
    }
    let mut buf: Vec<u8> = Vec::with_capacity(32 + re.src_bytes.len() + 8);
    buf.extend_from_slice(b"Invalid regular expression: /");
    buf.extend_from_slice(&crate::utf8::merge_surrogate_pairs(&re.src_bytes));
    buf.push(b'/');
    buf.extend_from_slice(&flag_letters(re.flags));
    buf.push(0);
    unsafe {
        __torajs_throw_syntax_error(buf.as_ptr());
    }
    drop(buf);
}

/// Flag byte → canonical letter spelling, ES §22.2.6.4 order
/// (`d g i m s u v y`) — the `parse_flags` inverse. Shared by the
/// SyntaxError message above and the subclass `super(regexArg)`
/// source+flags copy (§22.2.3.1 step 5).
pub(crate) fn flag_letters(f: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    for (bit, ch) in [
        (RE_FLAG_D, b'd'),
        (RE_FLAG_G, b'g'),
        (RE_FLAG_I, b'i'),
        (RE_FLAG_M, b'm'),
        (RE_FLAG_S, b's'),
        (RE_FLAG_U, b'u'),
        (RE_FLAG_V, b'v'),
        (RE_FLAG_Y, b'y'),
    ] {
        if f & bit != 0 {
            out.push(ch);
        }
    }
    out
}

unsafe extern "C" {
    /// torajs-anyvalue — the boxed value's slot tag / payload.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-anyvalue — ToString over a boxed value (fresh Str;
    /// a Symbol argument or a throwing user toString records a
    /// pending throw and the caller's check unwinds).
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    /// torajs-throw — pending-throw plumbing (`throw_type_error`
    /// rides the crate-wide declaration in `super`).
    fn __torajs_throw_check() -> i64;
    /// torajs-str — release a Str temp.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-rc — take a share of the returned receiver.
    fn __torajs_rc_inc(p: *mut c_void);
}

/// AnySlotTag mirror (`__torajs_anyv_box_from_pair` contract).
const ANYTAG_UNDEF: i64 = 5;

/// The argument's RegExp cell, or `None` for everything else.
/// Cell-likeness answers first (rotation 546 form, mirroring
/// `torajs_rc::nan_box_is_cell_like` — local copy per the
/// runtime-crate FFI-only convention): a ShortStr reports tag Heap
/// and `unbox_value` would leak one materialized Str per probe; a
/// real cell's box IS the raw pointer, so no unbox is needed.
unsafe fn anyv_regexp_cell(av: u64) -> Option<*const RegExp> {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    if av == 0 || (av & TOP_16_MASK) != 0 || (av & TAG_BIT_TYPE_OTHER) != 0 {
        return None;
    }
    let p = av as *const RegExp;
    if unsafe { (*p).header.type_tag } != TAG_REGEX {
        return None;
    }
    Some(p)
}

/// `av`'s ToString bytes, or `None` when the coercion recorded a
/// pending throw (Symbol → TypeError, user toString raise). An
/// undefined argument is §22.2.3.2 RegExpInitialize step 1/3's
/// empty string, not the text "undefined".
unsafe fn anyv_pattern_bytes(av: u64) -> Option<Vec<u8>> {
    if unsafe { __torajs_anyv_unbox_tag(av) } == ANYTAG_UNDEF {
        return Some(Vec::new());
    }
    let s = unsafe { __torajs_anyv_to_str(av) };
    if unsafe { __torajs_throw_check() } != 0 {
        unsafe { __torajs_str_drop(s) };
        return None;
    }
    let bytes = unsafe { str_units(s as *const c_void) };
    unsafe { __torajs_str_drop(s) };
    Some(bytes)
}

/// Annex B §B.2.4.1 `RegExp.prototype.compile(pattern, flags)` —
/// re-initialize the RECEIVER cell in place and return it.
///
/// Steps: a RegExp `pattern` with a present `flags` is a TypeError
/// (step 3.a); a RegExp pattern otherwise donates its own source
/// and flags (step 3.b); anything else takes
/// `ToString(pattern) / ToString(flags)` with undefined mapping to
/// the empty string (step 4 → §22.2.3.2 steps 1-3). Coercion
/// throws propagate BEFORE the receiver changes — the four
/// annexB/compile cases pin `lastIndex` unmoved across a throwing
/// call. A rejected parse records the same catchable SyntaxError
/// as `new RegExp(...)` and leaves the receiver untouched; success
/// swaps the freshly compiled body under the receiver's own header
/// (refcount, subclass flag) and resets `lastIndex` to +0
/// (§22.2.3.2 step 12), releasing the old body — including a
/// boxed-form lastIndex stake — through the ordinary drop path.
///
/// # Safety
/// `re_ptr` is null or a live `*RegExp`; `pat` / `flags` carry
/// valid AnyValue bit patterns. Returns an OWNED share of the
/// receiver (rc+1), or null when a pending throw is recorded.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_compile_inplace(
    re_ptr: *mut c_void,
    pat: u64,
    flags: u64,
) -> *mut c_void {
    unsafe {
        if re_ptr.is_null() {
            return core::ptr::null_mut();
        }
        let (p_bytes, f_bytes) = if let Some(src) = anyv_regexp_cell(pat) {
            if __torajs_anyv_unbox_tag(flags) != ANYTAG_UNDEF {
                super::__torajs_throw_type_error(
                    c"RegExp.prototype.compile: cannot supply flags when compiling a RegExp"
                        .as_ptr()
                        .cast(),
                );
                return core::ptr::null_mut();
            }
            ((*src).src_bytes.clone(), flag_letters((*src).flags))
        } else {
            let Some(p) = anyv_pattern_bytes(pat) else {
                return core::ptr::null_mut();
            };
            let Some(f) = anyv_pattern_bytes(flags) else {
                return core::ptr::null_mut();
            };
            (p, f)
        };
        let fresh = compile_bytes(p_bytes, f_bytes) as *mut RegExp;
        throw_if_rejected(fresh as *mut c_void);
        if __torajs_throw_check() != 0 {
            super::lifecycle::__torajs_regex_drop(fresh as *mut c_void);
            return core::ptr::null_mut();
        }
        // Swap the compiled body under the receiver's header: the
        // whole-struct swap moves every field (program, source,
        // capture tables, the zeroed lastIndex pair, the empty
        // caches), then the header swap hands the receiver back its
        // own refcount + subclass flag. The old body — old boxed
        // lastIndex stake included — rides out on the fresh cell's
        // rc=1 header through the ordinary drop.
        let old = re_ptr as *mut RegExp;
        core::ptr::swap(old, fresh);
        core::ptr::swap(&raw mut (*old).header, &raw mut (*fresh).header);
        super::lifecycle::__torajs_regex_drop(fresh as *mut c_void);
        __torajs_rc_inc(re_ptr);
        re_ptr
    }
}
