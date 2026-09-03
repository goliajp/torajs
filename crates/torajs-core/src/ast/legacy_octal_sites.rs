//! Annex B legacy octal, goal half — §B.1.1 numeric literals and
//! §B.1.2 string escapes are ordinary sloppy-script spellings and a
//! SyntaxError under a strict goal.
//!
//! Same three-part shape as [`super::strict_reserved_goal`] and
//! [`super::yield_ident_goal`]: the lexer already gave every one of
//! these its sloppy VALUE (`"\101"` is `"A"`, `010` is `8` — see
//! [`crate::lexer::legacy_octal`]), the positions park here, and this
//! gate answers the one question the lexer could not, the goal being
//! stamped only after the parse.
//!
//! Recorded from the token stream rather than the arena, for the
//! reason [`super::source_dunder_idents`] spells out: the question is
//! what the PROGRAM SPELLED, and by the time a literal reaches the
//! arena its spelling is gone — a `Token::Number(8.0)` cannot say
//! whether the author wrote `8` or `010`. The token's span can, so
//! this reads the source text back through it.
//!
//! One gap, deliberate and recorded here because it is invisible from
//! the call site: a `${…}` interpolation's tokens are lexed from a
//! SEPARATE slice ([`crate::lexer::scan_template`] hands
//! `bytes[expr_start..expr_end]` to a fresh `tokenize`), so their spans
//! index the interpolation, not the program. Reading them against
//! `ast.source` would slice unrelated text, so `${"\101"}` gets its
//! sloppy VALUE (the lexer is unconditional) but no strict-goal
//! rejection. The fix is absolute interpolation spans, which is a
//! change to what every downstream consumer of those spans sees and
//! not this pass's to make.
//!
//! The escape walk itself lives with the grammar it asks
//! ([`crate::lexer::legacy_octal::first_legacy_escape`]) rather than
//! here, because the untagged-template gate in
//! [`crate::parser::expr_entry`] asks the same question of a template's
//! raw text.

use super::Ast;
use crate::lexer::legacy_octal::{first_legacy_escape, legacy_octal_number_value};
use crate::lexer::{Spanned, Token};

/// Fill [`Ast::legacy_octal_positions`] from one token stream. Called
/// from the same two parse entry points that record the `__` spellings,
/// so every stream feeding the shared Ast is covered.
pub fn record_legacy_octal_sites(ast: &mut Ast, source: &str, tokens: &[Spanned]) {
    let bytes = source.as_bytes();
    for t in tokens {
        let (lo, hi) = (t.span.start, t.span.end);
        if hi as usize > bytes.len() {
            continue;
        }
        match &t.token {
            Token::String(_) => {
                if let Some(at) = first_legacy_escape(bytes, lo, hi) {
                    ast.legacy_octal_positions.push((at, ESCAPE));
                }
            }
            Token::Number(_) => {
                let raw = &source[lo as usize..hi as usize];
                if legacy_octal_number_value(raw).is_some() {
                    ast.legacy_octal_positions.push((lo, NUMBER));
                }
            }
            _ => {}
        }
    }
}

const ESCAPE: &str = "an octal or `\\8` / `\\9` escape sequence";
const NUMBER: &str = "a `0`-prefixed octal-style number";

/// `Some(msg)` = SyntaxError. A strict goal condemns every recorded
/// site; a sloppy goal condemns only those inside a body an explicit
/// `"use strict"` made strict ([`Ast::strict_prologue_spans`]), and
/// gives the rest the values the lexer already produced.
pub fn triage_legacy_octal(ast: &Ast) -> Option<String> {
    let condemned = |at: u32| {
        !ast.sloppy_script_goal
            || ast
                .strict_prologue_spans
                .iter()
                .any(|&(lo, hi)| at >= lo && at < hi)
    };
    ast.legacy_octal_positions
        .iter()
        .find(|(at, _)| condemned(*at))
        .map(|(at, what)| {
            format!(
                "{what} is not allowed in strict code \
                 at {at} (ES annexB §B.1.1 / §B.1.2)"
            )
        })
}
