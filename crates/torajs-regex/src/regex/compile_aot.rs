//! AOT-baked DFA path — Phase C-2 substrate of chunk 7.7 v2 step 12 C2.
//!
//! Adds `__torajs_regex_compile_from_static_dfa`, the second entry
//! point the AOT pipeline (ssa_lower, Phase C-4) emits for literal
//! regexes whose DFA was build-time precomputed and baked into the
//! user binary's `.rodata`. Distinct from
//! [`super::compile::__torajs_regex_compile`] only in that it stamps
//! the resulting `RegExp`'s `baked_dfa` slot with the caller-supplied
//! `BakedDfaMeta` pointer so the surface match path can read
//! [`super::RegExp::baked_dfa_view`] and skip the runtime
//! `build_dfa`. Pattern + flag parsing still runs because the Pike VM
//! second pass (capture extraction) consumes the same `Program`
//! bytecode the parser builds.
//!
//! The shape mirrors `__torajs_regex_compile` deliberately so a
//! future commit can refactor the shared pipeline into a private
//! `build_regex_inner` once Phase C lands and bakes in (the
//! refactor adds noise during the cross-substrate handoff but
//! contributes no Phase C semantic — leaving it as a follow-up
//! keeps Phase C-2 byte-minimal and inspectable).
//!
//! Why this lives in its own file: `compile.rs` is the runtime
//! literal / `new RegExp(...)` entry; `compile_aot.rs` is the AOT
//! variant. Future Phase D / E variants (full-Program AOT,
//! YARR-style JIT) extend along this axis, so the per-path split
//! keeps the surface stable.

use alloc::{boxed::Box, vec::Vec};
use core::ffi::c_void;
use core::ptr::NonNull;

use super::{HeapHeader, RegExp, TAG_REGEX, str_slice};
use crate::compiler::compile;
use crate::dfa::BakedDfaMeta;
use crate::flags::parse_flags;
use crate::parser::Parser;
use crate::program::{Inst, Program};
use crate::resolve::resolve_backrefs;

/// AOT-baked DFA entry. Identical contract to
/// [`super::compile::__torajs_regex_compile`] except the returned
/// `RegExp` has `baked_dfa = Some(meta)` so
/// [`super::RegExp::baked_dfa_view`] returns a `'static`-backed
/// `DfaProgram` and the surface match path completely skips the
/// `dfa_cache`/`UnsafeCell` lazy-build path.
///
/// # Safety
///
/// `meta` must point at a `BakedDfaMeta` whose `states_ptr` indirect
/// is a `[DfaState; states_len]` table living for the program's
/// lifetime (typically `.rodata` of the host binary, chain-LC
/// rebased by dyld). `pattern_str` and `flags_str` follow
/// `__torajs_regex_compile`'s contract: live `Str *` heap objects
/// with refcount > 0 + well-formed header.
///
/// Returned pointer is heap-allocated with refcount = 1 and
/// type_tag = `TAG_REGEX`; release with `__torajs_regex_drop` (same
/// drop path — `baked_dfa` is a non-owning raw ptr to `.rodata`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_compile_from_static_dfa(
    meta: *const BakedDfaMeta,
    pattern_str: *const c_void,
    flags_str: *const c_void,
) -> *mut c_void {
    let pat = unsafe { str_slice(pattern_str) };
    let fl = unsafe { str_slice(flags_str) };

    let flag_bits_opt = parse_flags(&fl);
    let flag_bits = flag_bits_opt.unwrap_or(0);
    // ES §22.2.3.1 — `u` and `v` are mutually exclusive, and
    // duplicate / unknown flag letters are Early Errors
    // (`parse_flags` returns `None`). Both funnel through the
    // never-match rejected-stub path.
    let flag_conflict = flag_bits_opt.is_none()
        || (flag_bits & crate::parser::RE_FLAG_U != 0 && flag_bits & crate::parser::RE_FLAG_V != 0);
    let src_bytes = pat.clone();

    let mut parser = Parser::new(&pat, flag_bits);
    let parse_result = parser.parse();
    let n_captures = parser.n_captures;
    let names_snapshot = parser.names.clone();

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

    let mut capture_names: Vec<Vec<u8>> = Vec::with_capacity(n_captures + 1);
    let mut n_named_captures: i32 = 0;
    if root_ok.is_some() && n_captures > 0 {
        capture_names.push(Vec::new());
        for i in 1..=n_captures {
            let name = names_snapshot.get(i).cloned().unwrap_or_default();
            if !name.is_empty() {
                n_named_captures += 1;
            }
            capture_names.push(name);
        }
    }

    let mut prog = Program::new();
    let rejected = if let Some(root) = root_ok.take() {
        // RC-4: multiline `$` (per-inst m-bit) is VM-only, mirroring
        // compile.rs.
        prog.can_dfa = crate::dfa::analyze(&root).is_eligible()
            && !crate::dfa::tree_contains_ml_anchor_end(&root);
        compile(&mut prog, &root, flag_bits);
        prog.emit(Inst::match_accept());
        prog.has_save = prog.any_save();
        prog.finalize_backref_caps();
        // regexp-modifiers — mirror compile.rs: uniform-ml drives the
        // wire's line-start entry selection; mixed m-bits gate the
        // DFA off (the text-start-entry fold can't represent them).
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

    // V0.2 P14-S2 — literal-byte prefix anchor detection (mirrors
    // compile.rs). The baked DFA path benefits from the prefix anchor
    // too: it shrinks the search window before the DFA byte-walk runs.
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

    // Baked-DFA bind: stamp the meta ptr into the new field so
    // `baked_dfa_view` resolves it on the surface match path. If the
    // caller passed a null meta (defensive — the AOT emitter never
    // does, but `extern "C"` callers can lie) we degrade gracefully to
    // the runtime DFA build path by leaving `baked_dfa = None`.
    let baked_dfa = NonNull::new(meta as *mut BakedDfaMeta);

    let re = Box::new(RegExp {
        header: HeapHeader {
            refcount: 1,
            type_tag: TAG_REGEX,
            flags: 0,
        },
        // No own property has been written yet — the bag is
        // minted by the first `re.zz = 1`, never here.
        props: core::ptr::null_mut(),
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
        workspace_cache: core::cell::UnsafeCell::new(None),
        replace_out_cache: core::cell::UnsafeCell::new(alloc::vec::Vec::new()),
        baked_dfa,
        // Round 3 Phase B sub-batch 7.2 — AOT path uses the
        // `.rodata`-baked DFA via `baked_dfa` / `baked_dfa_view`,
        // so the runtime-build slot stays `None` (avoids both a
        // wasteful per-ctor `build_dfa` and a duplicate `DfaProgram`
        // allocation when the baked view already serves matches).
        dfa_runtime: None,
    });
    Box::into_raw(re) as *mut c_void
}
