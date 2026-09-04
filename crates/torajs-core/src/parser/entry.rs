//! Parse entry points — `parse` / `parse_into` /
//! `parse_into_eval`, split from `parser.rs` when the r438
//! span-snapshot rework left it 2 lines under the 500 limit (the
//! r438 watch\'s predicted cut: the parse_into double-entry family).
//! The `Parser` struct and its grammar impls stay with the parent;
//! this file only constructs the machine and runs a whole program
//! through it.

use super::*;

pub fn parse(source: &str, tokens: &[Spanned]) -> Result<Ast, String> {
    let mut ast = Ast::default();
    parse_into(source, tokens, &mut ast)?;
    Ok(ast)
}

/// Phase K.2 — append-mode parse. Parses `tokens` into the existing
/// `target` AST, sharing its `exprs` arena so any newly-minted ExprIds
/// continue numbering from `target.exprs.len()`. Returns the index of
/// the first appended Stmt in `target.stmts` (caller can drain from
/// there to extract just the new section).
///
/// Used by `modules::resolve_imports` to merge an imported file's AST
/// into the main file's AST without an ExprId remap pass — every Expr
/// landed via `add_expr`, which mints a fresh u32 from the current
/// `exprs.len()`, so values originating in the imported file are
/// already indexed correctly within the merged arena.
///
/// The Parser-internal `desugar_id` counter is seeded with the current
/// arena length so any temp-name minting (`__step_<n>`, etc.) emitted
/// by parse-time desugars in the imported file can't collide with
/// names already minted while parsing the main file (or any earlier
/// imported file).
pub fn parse_into(source: &str, tokens: &[Spanned], target: &mut Ast) -> Result<usize, String> {
    parse_into_eval(source, tokens, target, false, false)
}

/// Eval-source variant of [`parse_into`], carrying the two facts about
/// the text that only its CALL SITE knows.
///
/// §19.2.1.1 PerformEval decides SuperProperty legality from the call
/// site's environment (steps 4-6: direct eval within a method context
/// may contain `super.x`), which a fresh parse of the eval text cannot
/// see — the eval desugar passes the verdict in as `super_prop_ok`.
///
/// `strict` is step 2's other half: a DIRECT eval's code is strict mode
/// code when the CALLING code is (§19.2.1.1 step 8 / §11.2.2), and the
/// call site is the only place that knows. Seeding `in_strict_fn` with
/// it is what puts the eval text under the judges the parser already
/// owns — `yield` and the §12.7.2 reserved words as identifiers, the
/// Annex B function-declaration positions, duplicate parameters, `with`
/// — rather than growing a second spelling of each for eval. (A whole
/// program cannot use this lane: its goal is stamped only after the
/// parse, which is why those judges have goal-half gates at all.)
pub fn parse_into_eval(
    source: &str,
    tokens: &[Spanned],
    target: &mut Ast,
    super_prop_ok: bool,
    strict: bool,
) -> Result<usize, String> {
    // Which `__`-prefixed names did the PROGRAM spell? Recorded from
    // the token stream, before the parser starts minting Ident nodes
    // of its own with that prefix (doc on the pass). Every stream that
    // feeds this Ast passes through here — the whole program, each
    // imported module, each direct `eval` source — and the set
    // accumulates across them.
    crate::ast::record_source_dunder_idents(target, tokens);
    // annexB §B.1.1 / §B.1.2 legacy-octal spellings, from the same
    // stream and for the same reason — the question is what the program
    // wrote, and a `Token::Number(8.0)` cannot say whether that was `8`
    // or `010`. Its span can, so the pass reads the text back.
    crate::ast::record_legacy_octal_sites(target, source, tokens);
    let stmt_offset = target.stmts.len();
    let id_offset = target.exprs.len() as u32;
    // 420-06 — a NESTED parse (lib / eval / builtin-injection source
    // appended to a non-empty arena) records class spans that index
    // ITS source text, not `ast.source`; slicing the main text with
    // them is garbage or out of bounds (the disposable-stack inject
    // panicked erase_types). Snapshot the WHOLE table and restore it
    // on exit: a key-only snapshot dropped the nested parse's new
    // entries but let a same-named nested class OVERWRITE the main
    // file's row in place (knife D — `class S1` in both entry and
    // lib), handing the entry's toString slice a span into the lib's
    // text. A whole-program parse starts from an empty arena and
    // keeps its spans.
    let nested_spans_snapshot =
        (stmt_offset > 0 || id_offset > 0).then(|| target.class_decl_spans.clone());
    // Same nested-parse story for the program-level `"use strict"`
    // verdict: an imported module is its own program and an eval text
    // is its own Script, so neither may answer the question "did the
    // ENTRY say it" (the field's doc). Snapshot and restore.
    let nested_prologue =
        (stmt_offset > 0 || id_offset > 0).then_some(target.program_strict_prologue);
    let taken = std::mem::take(target);
    let mut p = Parser {
        source,
        tokens,
        pos: 0,
        type_close_peel: 0,
        type_ann_depth: 0,
        in_arrow_ret_ann: false,
        ast: taken,
        desugar_id: id_offset,
        generator_fns: std::collections::HashMap::new(),
        current_class: None,
        class_stack: Vec::new(),
        in_for_init: false,
        bare_logical: false,
        in_gen_class_method: false,
        gen_recv_minted: false,
        void_folds: std::collections::HashSet::new(),
        in_async_gen: false,
        in_generator: false,
        in_strict_fn: strict,
        pending_async_fn_expr: false,
        static_this_class: None,
        super_call_allowed: false,
        super_prop_allowed: super_prop_ok,
        current_class_has_parent: false,
        synth_classes: Vec::new(),
        synth_classes_local: Vec::new(),
        stmt_depth: 0,
        class_value_aliases: std::collections::HashMap::new(),
        yield_hoist_buf: Vec::new(),
        dstra_saw_yield: false,
        dstra_deferred_rest_ids: std::collections::HashSet::new(),
        yield_hoist_allowed: true,
        in_formal_params: false,
        await_allowed: true,
    };
    let result = p.parse_program();
    // Private `#x` references parse to `__privu_<site>__<raw>`
    // placeholders (their declaring class is undecidable mid-body);
    // resolve them against the recorded lexical scopes now that every
    // declaration of this file is in. Error paths skip it — the parse
    // failed and the arena is moot.
    let result = result.and_then(|r| {
        p.resolve_private_refs(id_offset)?;
        Ok(r)
    });
    *target = p.ast;
    if let Some(snap) = nested_spans_snapshot {
        target.class_decl_spans = snap;
    }
    if let Some(was) = nested_prologue {
        target.program_strict_prologue = was;
    }
    result?;
    Ok(stmt_offset)
}
