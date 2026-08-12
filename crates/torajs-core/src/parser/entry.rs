//! How a parse is started — and, for each way of starting one, what
//! strictness the outermost body begins in.
//!
//! That second question is why these four live together. Every other
//! consequence of the goal bit can be answered after the parse, by a
//! gate reading the finished AST. Module strictness cannot: a function
//! body learns it is strict from a directive, and if none is in scope
//! by the time the body finishes the parser writes none for the rest
//! of the pipeline to read (`strict_directive::finish_fn_body_strict`).
//! So the seed has to arrive at the constructor, and each entry point
//! answers for its own callers — which is a small decision table, and
//! reads better as one.

use super::*;

/// Parse with no goal to go on: the outermost body starts sloppy.
///
/// The callers are the formatter, the linter and the checker's own
/// fixtures — none of which has a file extension to decide a goal
/// from. Anything that does should reach for [`parse_goal`].
pub fn parse(source: &str, tokens: &[Spanned]) -> Result<Ast, String> {
    let mut ast = Ast::default();
    parse_into_seeded(source, tokens, &mut ast, false, false)?;
    Ok(ast)
}

/// Whole-program parse that knows its goal. Module code is strict
/// (§16.1), so the goal arrives here rather than being stamped onto
/// the result afterwards — and the stamp becomes this function's job,
/// so that the seed and the bit cannot disagree.
pub fn parse_goal(source: &str, tokens: &[Spanned], sloppy_goal: bool) -> Result<Ast, String> {
    let mut ast = Ast::default();
    ast.sloppy_script_goal = sloppy_goal;
    parse_into_seeded(source, tokens, &mut ast, false, !sloppy_goal)?;
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
    // An appended parse joins a program whose goal is already stamped,
    // so the seed is readable off the target: an imported file merges
    // into the importer's AST and shares its goal, and eval text is
    // strict when the program that spelled it is (§19.2.1.1 — the
    // caller's own strictness reaches this text through the same bit).
    let strict_seed = !target.sloppy_script_goal;
    parse_into_seeded(source, tokens, target, super_prop_ok, strict_seed)
}
