//! §19.2.1.1 step 12 — a DIRECT eval called from a class field
//! initializer may not bring `arguments` in with it.
//!
//! §15.7.1 already states the prohibition for the initializer a
//! program writes literally, and the parser enforces it there
//! (`parser::class_field_early_errors`). But PerformEval carries
//! `inClassFieldInitializer` into the eval and re-states it over the
//! eval'd body, because the text arrives at that position too. tr
//! checked the literal half and never the eval half — so
//! `class C { x = eval('arguments') }` ran the body and answered
//! whatever `arguments` happened to resolve to.
//!
//! The verdict belongs at the CALL, not at parse time: the spec
//! throws when the initializer runs (`assert.throws(SyntaxError,
//! () => new C())`), and test262 also asserts the body did not
//! execute. So the rewrite is the same throw-IIFE carrier
//! `param_default` uses for the §19.2.1.3 special case next door,
//! and it runs before the value-position collapse for the same
//! reason: a collapsed source is no longer an eval call, and the
//! throw would be folded away with it.
//!
//! Static blocks are out: §15.7.1 states the literal prohibition for
//! them too, but §19.2.1.1 keys the eval half on
//! `inClassFieldInitializer`, which only a field initializer sets —
//! measured against bun, `static { eval('arguments') }` does not
//! throw.
//!
//! Ownership is the field-initializer position reached without
//! crossing anything that binds its own `arguments`. That boundary
//! rule is §8.4.1's and already has one implementation, so this pass
//! rides `class_field_early_errors::expr_walk` rather than spelling
//! it again — arrows are pierced (an arrow in a field initializer
//! still has no `arguments` of its own), function expressions,
//! methods and nested classes stop it.
//!
//! Instance initializers are reached through
//! `Ast::class_field_init_roots`: by the time any desugar runs the
//! parser has folded them into the constructor prefix, where they
//! are indistinguishable from constructor code — and a constructor
//! DOES have `arguments`, so re-deriving the position from the ctor
//! body would refuse legal programs. Static fields and static blocks
//! are still on the ClassDecl and are walked from there.

use super::super::{Ast, ExprId, Stmt};
use super::completion;
use super::recognize::literal_eval_call;
use super::source::{
    CallForm, blank_appended_exprs, eval_strictness, first_line, parse_eval_source,
    syntax_error_throw,
};
use super::walk;
use crate::parser::class_field_early_errors as cfe;

/// Rewrite every direct literal eval in class-field-initializer
/// position whose source contains `arguments` into a SyntaxError
/// throw IIFE.
pub(super) fn rewrite_class_field_arguments_evals(ast: &mut Ast) {
    let owned = field_init_owned_exprs(ast);
    if !owned.iter().any(|f| *f) {
        return;
    }
    let caller_strict = walk::caller_strict_exprs(ast);
    let mut i = 0;
    while i < ast.exprs.len() {
        if !owned.get(i).copied().unwrap_or(false) {
            i += 1;
            continue;
        }
        let eid = ExprId(i as u32);
        let Some((src, CallForm::Direct)) = literal_eval_call(eid, ast) else {
            i += 1;
            continue;
        };
        // Necessary condition, checked on the text: ContainsArguments
        // cannot be true of a source that never spells the word. Most
        // evals stop here and are never parsed twice.
        if !src.contains("arguments") {
            i += 1;
            continue;
        }
        // A source that fails to parse is not this pass's problem —
        // the completion pass turns it into the same throw.
        let strict = eval_strictness(CallForm::Direct, i, &caller_strict);
        let exprs_before = ast.exprs.len();
        let Ok(body) = parse_eval_source(&src, ast, false, strict) else {
            i += 1;
            continue;
        };
        let hit = body_contains_arguments(ast, &body);
        // This parse existed only to ask the question. Whichever way
        // it answered, the statements are dropped here — and a
        // dropped statement's expressions stay in the arena, where
        // the closure collector finds a complete `ArrowFn` out of the
        // eval text and lowers it as if the program had written it.
        // `eval("() => arguments")` in a field initializer got its
        // SyntaxError AND left that arrow behind, so the compile died
        // on `unknown ident arguments` instead. The rewrite below
        // reparses what it needs.
        blank_appended_exprs(ast, exprs_before);
        if hit {
            let throw = syntax_error_throw(
                format!(
                    "eval in a class field initializer may not contain `arguments` (ES 19.2.1.1): {}",
                    first_line(&src)
                ),
                ast,
            );
            completion::wrap_iife(i, vec![throw], ast);
        }
        i += 1;
    }
}

/// ContainsArguments over a parsed eval source, under §8.4.1's
/// boundary rule. The source's own arrows were registered in the
/// side tables when it parsed, so the shared walk reads them the
/// same way it reads the program's.
fn body_contains_arguments(ast: &Ast, stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| {
        cfe::stmt_walk(
            ast,
            s,
            &mut |e| matches!(ast.get_expr(e), super::super::Expr::Ident(n) if n == "arguments"),
        )
    })
}

/// One flag per arena slot: does this expression sit in a class field
/// initializer, reached without crossing anything that binds its own
/// `arguments`?
///
/// Static blocks are deliberately NOT here. §15.7.1 does state the
/// literal prohibition twice — once for a field initializer, once for
/// a ClassStaticBlockStatementList — but §19.2.1.1 keys the eval half
/// on `inClassFieldInitializer`, which only a field initializer sets
/// (a static block has no [[ClassFieldInitializerName]]). Measured
/// against bun: a direct eval naming `arguments` inside `static { }`
/// does not throw.
fn field_init_owned_exprs(ast: &Ast) -> Vec<bool> {
    let mut owned = vec![false; ast.exprs.len()];
    for root in &ast.class_field_init_roots {
        mark_from_expr(ast, *root, &mut owned);
    }
    owned
}

fn mark_from_expr(ast: &Ast, root: ExprId, owned: &mut [bool]) {
    let mut seen: Vec<ExprId> = Vec::new();
    cfe::expr_walk(ast, root, &mut |e| {
        seen.push(e);
        false
    });
    for e in seen {
        if let Some(slot) = owned.get_mut(e.0 as usize) {
            *slot = true;
        }
    }
}
