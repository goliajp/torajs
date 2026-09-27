//! Generator body walker: the J.4 yield-into expansion, moved out of
//! `desugar_generators_walkers` for file size.

use super::{Ast, Expr, Stmt};

/// J.4 — recursively expand every `Stmt::YieldInto { var, type_ann,
/// value }` in `s` into the pair `[Stmt::Yield(value);
/// Stmt::LetDecl { name: var, type_ann, init: this.__sent }]`. The
/// pair is wrapped in `Stmt::Multi` so it occupies the YieldInto's
/// original slot without disturbing surrounding scope. Walks into
/// nested control-flow.
///
/// `yield_ty` is the surrounding generator's declared yield type; it
/// supplies the let's annotation when the user omitted one (so the
/// J.2.b lift picks the right field type).
pub(crate) fn expand_yield_into_in_stmt(ast: &mut Ast, s: &mut Stmt, yield_ty: &str) {
    match s {
        Stmt::YieldInto {
            var,
            type_ann,
            value,
        } => {
            let var = std::mem::take(var);
            // Unannotated by the user: the resume channel's own type
            // (`sent_slot_ann`), which can hold the `undefined` a bare
            // `next()` sends. A written annotation is kept as written.
            let ty = type_ann.clone().or_else(|| {
                Some(crate::ast::desugar_generators_methods::sent_slot_ann(
                    yield_ty,
                ))
            });
            let value = *value;
            let yield_stmt = Stmt::Yield(value);
            let this_id = ast.add_expr(Expr::This);
            let sent_member = ast.add_expr(Expr::Member {
                obj: this_id,
                name: "__sent".into(),
            });
            // A yield the async rewrite made of `let v = await e`: the
            // sent value IS `e`'s settled value, so the width analysis
            // reads this init as `e.value` (`yield_into_await_origins`).
            if ast.await_yield_values.contains(&value) {
                ast.yield_into_await_origins.insert(sent_member, value);
            }
            let let_stmt = Stmt::LetDecl {
                mutable: true,
                name: var,
                type_ann: ty,
                init: sent_member,
                is_var: false,
            };
            *s = Stmt::Multi(vec![yield_stmt, let_stmt]);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            expand_yield_into_in_stmt(ast, then_branch, yield_ty);
            if let Some(eb) = else_branch.as_deref_mut() {
                expand_yield_into_in_stmt(ast, eb, yield_ty);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::Labeled { body, .. } => {
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                expand_yield_into_in_stmt(ast, i, yield_ty);
            }
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                expand_yield_into_in_stmt(ast, s, yield_ty);
            }
        }
        // RFC 20260802 — try/catch is in yield scope now. Descending
        // into a yield-free try is a no-op (nothing to expand), so no
        // gate is needed here.
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body.iter_mut().chain(catch_body.iter_mut()) {
                expand_yield_into_in_stmt(ast, s, yield_ty);
            }
            if let Some(fs) = finally_body {
                for s in fs {
                    expand_yield_into_in_stmt(ast, s, yield_ty);
                }
            }
        }
        // Switch cases not yet in yield scope (J.2.b).
        _ => {}
    }
}
