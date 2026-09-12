//! Deep-clone Stmt helper extracted from `ssa_lower.rs` chunk 365; the
//! Expr half lives in the sibling `ssa_lower_deep_clone_expr` (rotation
//! 637 displacement — the statement arms had to grow past the cap).
//!
//! Used exclusively by generic monomorphization
//! (`check_monomorph::monomorphize_and_check`) — each mono
//! instantiation gets a private copy of the body's expressions so that
//! subsequent per-instantiation rewrites (substitute_in_stmt +
//! rewrite_tvdefault_in_stmt) do not clobber each other. The clone
//! allocates fresh ExprIds by appending to `Ast::exprs`.

use crate::ast::{Ast, ExprId, Stmt};
use crate::ssa_lower_deep_clone_expr::deep_clone_expr;

/// Deep-clone a Stmt's expression graph into the AST's arena, returning
/// a Stmt that references freshly-allocated ExprIds. Used by
/// monomorphization so each instantiation gets its own private copy of
/// the body's expressions (no shared rewriting between instantiations).
///
/// `map` records every `(original ExprId, cloned ExprId)` pair in
/// traversal order (a Vec, not a HashMap, so downstream consumers
/// iterate deterministically). The monomorphizer uses it to migrate
/// the checker's per-ExprId side tables (generic_call_sites /
/// arity_pad_count) onto the cloned body — those are keyed by the
/// ORIGINAL body's ExprIds and would otherwise never match a clone.
pub(crate) fn deep_clone_stmt(ast: &mut Ast, map: &mut Vec<(ExprId, ExprId)>, s: &Stmt) -> Stmt {
    match s {
        Stmt::Expr(eid) => Stmt::Expr(deep_clone_expr(ast, map, *eid)),
        Stmt::Throw(eid) => Stmt::Throw(deep_clone_expr(ast, map, *eid)),
        Stmt::Return(maybe) => Stmt::Return(maybe.map(|eid| deep_clone_expr(ast, map, eid))),
        Stmt::LetDecl {
            mutable,
            name,
            type_ann,
            init,
            is_var,
        } => Stmt::LetDecl {
            mutable: *mutable,
            name: name.clone(),
            type_ann: type_ann.clone(),
            init: deep_clone_expr(ast, map, *init),
            // a deep clone must preserve `is_var` — hardcoding false
            // silently turned cloned `var` decls into `let`/`const`,
            // dropping var-hoist semantics (zero-warn surfaced it).
            is_var: *is_var,
        },
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If {
            cond: deep_clone_expr(ast, map, *cond),
            then_branch: Box::new(deep_clone_stmt(ast, map, then_branch)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(deep_clone_stmt(ast, map, e))),
        },
        Stmt::Labeled { label, body } => Stmt::Labeled {
            label: label.clone(),
            body: Box::new(deep_clone_stmt(ast, map, body)),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: deep_clone_expr(ast, map, *cond),
            body: Box::new(deep_clone_stmt(ast, map, body)),
        },
        Stmt::DoWhile { body, cond } => Stmt::DoWhile {
            body: Box::new(deep_clone_stmt(ast, map, body)),
            cond: deep_clone_expr(ast, map, *cond),
        },
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => Stmt::For {
            init: init
                .as_ref()
                .map(|i| Box::new(deep_clone_stmt(ast, map, i))),
            cond: cond.map(|c| deep_clone_expr(ast, map, c)),
            step: step.map(|s2| deep_clone_expr(ast, map, s2)),
            body: Box::new(deep_clone_stmt(ast, map, body)),
        },
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => Stmt::Switch {
            scrutinee: deep_clone_expr(ast, map, *scrutinee),
            cases: cases
                .iter()
                .map(|c| crate::ast::SwitchCase {
                    value: deep_clone_expr(ast, map, c.value),
                    body: c
                        .body
                        .iter()
                        .map(|s| deep_clone_stmt(ast, map, s))
                        .collect(),
                })
                .collect(),
            default: default
                .as_ref()
                .map(|db| db.iter().map(|s| deep_clone_stmt(ast, map, s)).collect()),
        },
        Stmt::Block(stmts) => {
            Stmt::Block(stmts.iter().map(|s| deep_clone_stmt(ast, map, s)).collect())
        }
        Stmt::Multi(stmts) => {
            Stmt::Multi(stmts.iter().map(|s| deep_clone_stmt(ast, map, s)).collect())
        }
        Stmt::Try {
            body,
            had_catch,
            catch_param,
            catch_type,
            catch_body,
            finally_body,
        } => Stmt::Try {
            body: body.iter().map(|s| deep_clone_stmt(ast, map, s)).collect(),
            had_catch: *had_catch,
            catch_param: catch_param.clone(),
            catch_type: catch_type.clone(),
            catch_body: catch_body
                .iter()
                .map(|s| deep_clone_stmt(ast, map, s))
                .collect(),
            finally_body: finally_body
                .as_ref()
                .map(|fb| fb.iter().map(|s| deep_clone_stmt(ast, map, s)).collect()),
        },
        // Rotation 637 (637-06) — these carried nested ExprIds through a
        // shallow `.clone()`: a spec's for-of body kept the TEMPLATE's
        // ids, so a closure constructed inside it was never found by
        // `clone_spec_closures` (the id_map is what it scans) and stayed
        // ONE lifted decl shared by every specialization, its captures
        // typed by whichever spec checked last — `typeof thisv` inside
        // `for (var key of …) callIt(() => …)` answered `key`'s type.
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => Stmt::ForOfSplitIter {
            var_name: var_name.clone(),
            parent: deep_clone_expr(ast, map, *parent),
            sep: deep_clone_expr(ast, map, *sep),
            body: Box::new(deep_clone_stmt(ast, map, body)),
        },
        Stmt::ForOf {
            var_name,
            var_type_ann,
            src_ident,
            i_ident,
            elem_expr,
            body,
            forin_obj,
            is_await,
        } => Stmt::ForOf {
            var_name: var_name.clone(),
            var_type_ann: var_type_ann.clone(),
            src_ident: src_ident.clone(),
            i_ident: i_ident.clone(),
            elem_expr: deep_clone_expr(ast, map, *elem_expr),
            body: Box::new(deep_clone_stmt(ast, map, body)),
            forin_obj: forin_obj.map(|e| deep_clone_expr(ast, map, e)),
            is_await: *is_await,
        },
        Stmt::UsingDecl {
            name,
            type_ann,
            init,
            is_await,
        } => Stmt::UsingDecl {
            name: name.clone(),
            type_ann: type_ann.clone(),
            init: deep_clone_expr(ast, map, *init),
            is_await: *is_await,
        },
        Stmt::Yield(eid) => Stmt::Yield(deep_clone_expr(ast, map, *eid)),
        Stmt::YieldInto {
            var,
            type_ann,
            value,
        } => Stmt::YieldInto {
            var: var.clone(),
            type_ann: type_ann.clone(),
            value: deep_clone_expr(ast, map, *value),
        },
        // Declarations and jumps carry no expression of the body's own
        // (a nested `FnDecl` is lifted long before a spec is cloned).
        other => other.clone(),
    }
}
