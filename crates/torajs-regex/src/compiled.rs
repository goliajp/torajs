//! The pattern-to-program pipeline every RegExp construction runs:
//! flag letters, parse, backreference resolution, the capture-name
//! table, bytecode, and the search facts derived from the bytecode.
//!
//! The runtime runs it for a pattern it only meets at run time
//! (`new RegExp(s)`, `RegExp.prototype.compile`, a string argument
//! coerced by `match` / `search`). A literal's result is a pure
//! function of its source text, so the compiler runs it at build time
//! and bakes the [`Compiled`] into the binary ([`crate::image`]); the
//! literal's construction then decodes it and never reaches the
//! parser or the Unicode property tables the parser resolves names in.

use alloc::vec::Vec;

use crate::compiler::compile;
use crate::flags::parse_flags;
use crate::parser::{Parser, RE_FLAG_M, RE_FLAG_U, RE_FLAG_V};
use crate::program::{Inst, Op, Program};
use crate::resolve::resolve_backrefs;

/// Everything a RegExp keeps from compiling its pattern.
#[derive(Debug)]
pub struct Compiled {
    /// `RE_FLAG_*` bits (0 when the flag letters are rejected).
    pub flags: u8,
    /// 1 when the pattern or the flags are a SyntaxError: the program
    /// is then the never-match stub.
    pub rejected: u8,
    pub n_captures: i32,
    /// One name per capture group, index 0 reserved; empty when the
    /// pattern has no group or is rejected.
    pub capture_names: Vec<Vec<u8>>,
    pub n_named_captures: i32,
    pub prog: Program,
}

/// Run the pipeline over `pat` in its code-unit form (a supplementary
/// character spelled as two surrogate forms, as a Str transcodes) and
/// the flag letters `fl`.
pub fn compile_pattern(pat: &[u8], fl: &[u8]) -> Compiled {
    let flag_bits_opt = parse_flags(fl);
    let flag_bits = flag_bits_opt.unwrap_or(0);
    // ES §22.2.3.1 — `u` and `v` are mutually exclusive, and
    // duplicate / unknown flag letters are also Early Errors
    // (`parse_flags` returns `None` on either). Both take the
    // never-match rejected stub.
    let flag_conflict =
        flag_bits_opt.is_none() || (flag_bits & RE_FLAG_U != 0 && flag_bits & RE_FLAG_V != 0);
    // the parser reads the form the flags name (§22.2.2.1)
    let merged;
    let pat = if crate::flags::unicode_mode(flag_bits) {
        merged = crate::utf8::merge_surrogate_pairs(pat);
        &merged[..]
    } else {
        pat
    };

    let mut parser = Parser::new(pat, flag_bits);
    let parse_result = parser.parse();
    let n_captures = parser.n_captures;
    let names = parser.names.clone();

    let root_ok = match parse_result {
        Some(_) if flag_conflict => None,
        Some(mut root) => {
            resolve_backrefs(&mut root, &names, n_captures, flag_bits).then_some(root)
        }
        None => None,
    };

    let mut capture_names: Vec<Vec<u8>> = Vec::with_capacity(n_captures + 1);
    let mut n_named_captures: i32 = 0;
    if root_ok.is_some() && n_captures > 0 {
        capture_names.push(Vec::new());
        for i in 1..=n_captures {
            let name = names.get(i).cloned().unwrap_or_default();
            if !name.is_empty() {
                n_named_captures += 1;
            }
            capture_names.push(name);
        }
    }

    let mut prog = Program::new();
    let rejected = if let Some(root) = root_ok {
        // DFA eligibility runs post-resolve so named backrefs count as
        // blockers. A multiline `$` (per-inst m-bit) is VM-only: the
        // DFA closure can't see the byte after it.
        prog.can_dfa = crate::dfa::analyze(&root).is_eligible()
            && !crate::dfa::tree_contains_ml_anchor_end(&root);
        compile(&mut prog, &root, flag_bits);
        prog.emit(Inst::match_accept());
        prog.has_save = prog.any_save();
        prog.finalize_backref_caps();
        // Uniform multiline `^` drives the DFA wire's line-start entry
        // selection; mixed m-bits gate the DFA off, since the
        // four-entry scheme folds every line start onto the
        // text-start entry and so advances every `^` there.
        let (mut any_ml, mut any_plain) = (false, false);
        for inst in &prog.insts {
            if inst.op == Op::AnchorB as u8 {
                if inst.pad as u8 & RE_FLAG_M != 0 {
                    any_ml = true;
                } else {
                    any_plain = true;
                }
            }
        }
        prog.has_ml_anchor_b = any_ml;
        if any_ml && (any_plain || crate::vm::line_terminator::ml_line_start_after_consume(&prog)) {
            prog.can_dfa = false;
        }
        // literal-prefix anchor and whole-literal fast paths
        prog.derive_search_anchors();
        0u8
    } else {
        prog.emit(Inst::char_lit(0xffu8));
        prog.emit(Inst::match_accept());
        1u8
    };

    Compiled {
        flags: flag_bits,
        rejected,
        n_captures: n_captures as i32,
        capture_names,
        n_named_captures,
        prog,
    }
}
