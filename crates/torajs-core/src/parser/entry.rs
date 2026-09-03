//! Parse entry points — `parse` / `parse_into` /
//! `parse_into_super_prop`, split from `parser.rs` when the r438
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
    parse_into_super_prop(source, tokens, target, false)
}

/// Eval-source variant of [`parse_into`]. §19.2.1.1 PerformEval decides
/// SuperProperty legality from the CALL SITE's environment (steps 4-6:
/// direct eval within a method context may contain `super.x`), which a
/// fresh parse of the eval text cannot see — the eval desugar passes
/// the verdict in as `super_prop_ok`. Everything else parses with the
/// same default position flags as a whole program.
pub fn parse_into_super_prop(
    source: &str,
    tokens: &[Spanned],
    target: &mut Ast,
    super_prop_ok: bool,
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
        in_strict_fn: false,
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
    result?;
    Ok(stmt_offset)
}
