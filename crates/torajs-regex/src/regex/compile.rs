//! `__torajs_regex_compile` — port of `runtime_regex.c` L1445-1512.
//!
//! Drives the full compile pipeline:
//! 1. Decode pattern + flag-string bytes (Str payloads).
//! 2. Parse with [`crate::parser::Parser`] to an AST.
//! 3. Resolve `\k<name>` + validate `\N` via
//!    [`crate::resolve::resolve_backrefs`].
//! 4. Build named-capture name table for `.groups` construction at
//!    match time.
//! 5. Compile to bytecode via [`crate::compiler::compile`].
//! 6. On parse failure: emit a never-match stub program + mark
//!    `rejected = 1` so the surface methods abort with
//!    "not yet supported:" rather than producing wrong matches.
//!
//! The resulting `RegExp` is heap-allocated via
//! `Box::into_raw(Box::new(...))` so the universal header sits at
//! offset 0 of the allocation block. `__torajs_value_drop_heap`'s
//! tag dispatch on `header.type_tag = TAG_REGEX` routes drops back
//! to [`super::lifecycle::__torajs_regex_drop`].

use alloc::{boxed::Box, vec::Vec};
use core::ffi::c_void;

use super::{__torajs_throw_syntax_error, HeapHeader, RegExp, TAG_REGEX, str_slice};
use crate::compiler::compile;
use crate::flags::parse_flags;
use crate::parser::{
    Parser, RE_FLAG_D, RE_FLAG_G, RE_FLAG_I, RE_FLAG_M, RE_FLAG_S, RE_FLAG_U, RE_FLAG_V, RE_FLAG_Y,
};
use crate::program::{Inst, Program};
use crate::resolve::resolve_backrefs;

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
    let pat = unsafe { str_slice(pattern_str) };
    let fl = unsafe { str_slice(flags_str) };
    compile_bytes(pat, fl)
}

/// Bytes-level compile pipeline — the extern face above decodes the
/// Str cells first; the subclass `super(pattern)` kernel (RFC
/// 20260730 blade 2) enters here with bytes it already owns.
pub(crate) fn compile_bytes(pat: Vec<u8>, fl: Vec<u8>) -> *mut c_void {
    let flag_bits_opt = parse_flags(&fl);
    let flag_bits = flag_bits_opt.unwrap_or(0);
    // ES §22.2.3.1 — `u` and `v` are mutually exclusive, and
    // duplicate / unknown flag letters are also Early Errors
    // (`parse_flags` returns `None` on either). Both funnel through
    // the never-match rejected-stub path so `__torajs_regex_compile_or_throw`
    // sees `rejected == 1` and records a SyntaxError.
    let flag_conflict = flag_bits_opt.is_none()
        || (flag_bits & crate::parser::RE_FLAG_U != 0 && flag_bits & crate::parser::RE_FLAG_V != 0);
    let src_bytes = pat.clone();

    let mut parser = Parser::new(&pat, flag_bits);
    let parse_result = parser.parse();
    let n_captures = parser.n_captures;

    // Build a Vec<Vec<u8>> snapshot of the name table for later
    // attach_groups; the parser's names are still valid here.
    let names_snapshot = parser.names.clone();

    // Resolve backrefs against the now-known capture count + name
    // table. Failure here promotes the regex to `rejected`.
    let mut root_ok = match parse_result {
        Some(_) if flag_conflict => None,
        Some(mut root) => {
            if resolve_backrefs(&mut root, &names_snapshot, n_captures, flag_bits) {
                Some(root)
            } else {
                None
            }
        }
        None => None,
    };

    // Persist named-capture table for `.groups` construction at
    // match time. Owned `Vec<Vec<u8>>` survives the parser drop —
    // matches the C port's malloc+memcpy.
    let mut capture_names: Vec<Vec<u8>> = Vec::with_capacity(n_captures + 1);
    let mut n_named_captures: i32 = 0;
    if root_ok.is_some() && n_captures > 0 {
        // Index 0 reserved.
        capture_names.push(Vec::new());
        for i in 1..=n_captures {
            let name = names_snapshot.get(i).cloned().unwrap_or_default();
            if !name.is_empty() {
                n_named_captures += 1;
            }
            capture_names.push(name);
        }
    }

    // Compile + emit terminator. On parse failure, emit a
    // never-match stub (OP_CHAR 0xff + OP_MATCH) so the `.test()`
    // path returns false silently.
    let mut prog = Program::new();
    let rejected = if let Some(root) = root_ok.take() {
        // DFA eligibility — runs post-resolve so named backrefs are
        // correctly identified as blockers. RC-4: a multiline `$`
        // (per-inst m-bit, regexp-modifiers) is VM-only (the DFA
        // closure can't see the right byte, so `$` before `\n`
        // silently missed).
        prog.can_dfa = crate::dfa::analyze(&root).is_eligible()
            && !crate::dfa::tree_contains_ml_anchor_end(&root);
        compile(&mut prog, &root, flag_bits);
        prog.emit(Inst::match_accept());
        prog.has_save = prog.any_save();
        prog.finalize_backref_caps();
        // regexp-modifiers — classify the emitted `^` anchors' m-bits.
        // Uniform multiline drives the wire's line-start entry
        // selection (`has_ml_anchor_b`); MIXED m-bits gate the DFA
        // off entirely: the four-entry scheme folds every line-start
        // position onto the text-start entry, which advances ALL
        // AnchorB pcs — wrong for a plain `^` sharing the program
        // with a `(?m:^)`. The Pike VM resolves per instruction.
        let (mut any_ml, mut any_plain) = (false, false);
        for inst in &prog.insts {
            if inst.op == crate::program::Op::AnchorB as u8 {
                if inst.pad as u8 & crate::parser::RE_FLAG_M != 0 {
                    any_ml = true;
                } else {
                    any_plain = true;
                }
            }
        }
        prog.has_ml_anchor_b = any_ml;
        if any_ml && any_plain {
            prog.can_dfa = false;
        }
        0u8
    } else {
        prog.emit(Inst::char_lit(0xff));
        prog.emit(Inst::match_accept());
        1u8
    };

    // V0.2 P14-S2 perf — detect a literal-byte prefix anchor.
    // Walk the emitted bytecode forward, skipping zero-width
    // bookkeeping ops (`Save` for `(...)` capture brackets,
    // `AnchorB` for `^`, `WBound` etc), until we either hit an
    // `OP_CHAR(b)` (set anchor) or a byte-consuming op of a
    // different shape (`AnyChar` / `Class` / `Split` / ... → no
    // anchor). The leading char's baked i-bit (per-inst
    // ignoreCase, regexp-modifiers) invalidates the byte comparison
    // (memchr can't match both cases without a lookup table) so
    // skip the anchor there. The u flag is fine — leading
    // `Char` ops only emit ASCII bytes; non-ASCII literals decode
    // to `Class` at parse time which lands in the "different
    // shape" branch.
    if rejected == 0 {
        for inst in &prog.insts {
            match crate::program::Op::from_u8(inst.op) {
                Some(crate::program::Op::Save)
                | Some(crate::program::Op::AnchorB)
                | Some(crate::program::Op::AnchorE)
                | Some(crate::program::Op::WBound)
                | Some(crate::program::Op::NWBound) => continue,
                Some(crate::program::Op::Char) => {
                    if inst.pad as u8 & crate::parser::RE_FLAG_I == 0 {
                        prog.prefix_byte = Some(inst.ch);
                    }
                    break;
                }
                _ => break,
            }
        }
    }

    // Round 3 Phase B sub-batch 7.2 — eager-build runtime DFA at ctor
    // time when the program is DFA-eligible, so the per-call
    // `dfa_built_local` path in `vm::search_from_with_ws` becomes
    // dead (deleted in sub-batch 7.3). For non-AOT-baked literal
    // regexes hot-looped via LICM-hoisted `for i { s.match(re) }`,
    // moves ~1-3 µs/iter of `build_dfa(...)` out of the inner loop
    // into a one-shot RegExp constructor cost.
    //
    // Storage `Option<DfaProgram>` (no `UnsafeCell`) — eagerly
    // populated once at ctor, immutable for the RegExp's lifetime;
    // closes the chunk-7.6 SIGBUS UB family which originated from
    // interior-mutable cache shapes.
    let dfa_runtime = if rejected == 0 && prog.can_dfa && crate::dfa::prog_ops_dfa_safe(&prog) {
        // RFC 20260711 chunk B — a poisoned build (unserveable
        // K-PROPERTY shape) is discarded; the Pike VM serves.
        Some(crate::dfa::build_dfa(&prog, flag_bits)).filter(|d| !d.poisoned)
    } else {
        None
    };

    let re = Box::new(RegExp {
        header: HeapHeader {
            refcount: 1,
            type_tag: TAG_REGEX,
            flags: 0,
        },
        flags: flag_bits,
        rejected,
        _pad: [0; 2],
        n_captures: n_captures as i32,
        prog,
        src_bytes,
        capture_names,
        n_named_captures,
        last_index: 0.0,
        last_index_boxed: 0,
        // V0.2 P14-S8 — lazy-init Pike VM workspace cache.
        workspace_cache: core::cell::UnsafeCell::new(None),
        replace_out_cache: core::cell::UnsafeCell::new(alloc::vec::Vec::new()),
        // V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-2 — runtime
        // compile path: no AOT-baked DFA. Set non-`None` only by
        // [`super::compile_aot::__torajs_regex_compile_from_static_dfa`]
        // when the user binary's AOT pipeline emitted a
        // `BakedDfaMeta` for this literal regex.
        baked_dfa: None,
        dfa_runtime,
    });
    Box::into_raw(re) as *mut c_void
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
    buf.extend_from_slice(&re.src_bytes);
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
