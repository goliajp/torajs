//! The yield type of a synchronous generator written without one,
//! when its own body settles it.
//!
//! An unannotated generator used to yield through the `any` lane: every
//! `yield i` NaN-boxed its operand and every `for (const v of g())`
//! unboxed it, although the body said plainly what came out. The value
//! channel only — what `next()` answers in `value`. What a `yield`
//! expression receives back (`next(x)`'s `x`) stays `any`: TypeScript
//! types it `unknown` for an unannotated generator, so `g.next("s")`
//! is a legal call whatever the generator yields.
//!
//! The rule is deliberately narrow, because a yield is lowered as
//! `<operand> as <yield type>` and a wrong answer misplaces a value
//! instead of being refused. Every `yield` operand must be one of
//!
//! - a number or string literal, or a negated number literal;
//! - a param or lifted local whose field annotation is exactly
//!   `number` / `string`;
//! - a `+` of two operands that both answer the same one of those,
//!
//! all operands must give the same answer, and the body must not
//! `return <expr>` (its value would share the step's `value` slot).
//! Only `number` and `string` are answered: they have an `undefined`
//! spelling in a typed slot, which the exhausted step's `value` needs;
//! `boolean` / `bigint` would read back as `false` / `0n`.

use super::desugar_generators_walkers::LiftCtx;
use super::desugar_with::walk::stmt_children_ref;
use super::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};

/// `number` / `string` when every `yield` in `body` provably yields
/// that type, else `None` (the generator keeps the `any` lane).
/// `body` is the generator's body after the let-lift, so `ctx.binds`
/// knows every local a `yield` can name.
pub(super) fn infer_yield_ty(ast: &Ast, body: &[Stmt], ctx: &LiftCtx) -> Option<String> {
    let mut answer: Option<&'static str> = None;
    body.iter()
        .all(|s| settles(ast, s, ctx, &mut answer))
        .then_some(answer?.to_string())
}

/// Whether anything in the program can hand a generator's `return` a
/// value. `it.return(v)` answers `{ value: v, done: true }` out of the
/// same step slot the yields fill, so a typed slot would convert `v`
/// (a string sent through a number generator read back `NaN`). Only
/// the zero-argument call is harmless — it completes with `undefined`,
/// which a typed slot spells as its sentinel; every other use of a
/// `return` member (with an argument, or taken as a value for `.call`
/// / `.apply` / a computed `["return"]`) keeps generators on the `any`
/// lane. By name, over the whole program: whose `return` it is cannot
/// be told this early.
pub(super) fn return_can_take_a_value(ast: &Ast) -> bool {
    let bare_callees: std::collections::HashSet<u32> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, args } | Expr::OptCall { callee, args } if args.is_empty() => {
                Some(callee.0)
            }
            _ => None,
        })
        .collect();
    ast.exprs.iter().enumerate().any(|(i, e)| {
        let names_return = match e {
            Expr::Member { name, .. } | Expr::OptChain { name, .. } => name == "return",
            Expr::Index { index, .. } | Expr::OptIndex { index, .. } => {
                matches!(ast.get_expr(*index), Expr::String(s) if *s == "return")
            }
            _ => false,
        };
        names_return && !bare_callees.contains(&(i as u32))
    })
}

/// Whether `s` keeps `answer` consistent; `false` as soon as one
/// `yield` falls outside the rule or disagrees with an earlier one.
fn settles(ast: &Ast, s: &Stmt, ctx: &LiftCtx, answer: &mut Option<&'static str>) -> bool {
    match s {
        Stmt::Yield(e) | Stmt::YieldInto { value: e, .. } => {
            let Some(ty) = operand_ty(ast, *e, ctx) else {
                return false;
            };
            if answer.is_some_and(|a| a != ty) {
                return false;
            }
            *answer = Some(ty);
            true
        }
        Stmt::Return(Some(_)) => false,
        // A nested function yields (and returns) for itself.
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } => true,
        _ => stmt_children_ref(s)
            .into_iter()
            .all(|c| settles(ast, c, ctx, answer)),
    }
}

fn operand_ty(ast: &Ast, e: ExprId, ctx: &LiftCtx) -> Option<&'static str> {
    match ast.get_expr(e) {
        Expr::Number(_) => Some("number"),
        Expr::String(_) => Some("string"),
        Expr::Unary {
            op: UnaryOp::Neg,
            expr,
        } if matches!(ast.get_expr(*expr), Expr::Number(_)) => Some("number"),
        Expr::Ident(n) => match ctx.binds.get(n).map(String::as_str) {
            Some("number") => Some("number"),
            Some("string") => Some("string"),
            _ => None,
        },
        Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } => {
            let l = operand_ty(ast, *left, ctx)?;
            (operand_ty(ast, *right, ctx)? == l).then_some(l)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn ctx<'a>(sigs: &'a HashMap<String, String>, binds: &[(&str, &str)]) -> LiftCtx<'a> {
        LiftCtx {
            params: &[],
            reserved: Vec::new(),
            fn_sigs: sigs,
            binds: binds
                .iter()
                .map(|(n, t)| (n.to_string(), t.to_string()))
                .collect(),
            local_classes: Default::default(),
            machine_class: String::new(),
        }
    }

    fn yield_of(ast: &mut Ast, e: Expr) -> Stmt {
        Stmt::Yield(ast.add_expr(e))
    }

    #[test]
    fn a_number_local_and_a_literal_agree_on_number() {
        let sigs = HashMap::new();
        let c = ctx(&sigs, &[("i", "number")]);
        let mut ast = Ast::default();
        let a = yield_of(&mut ast, Expr::Ident("i".into()));
        let b = yield_of(&mut ast, Expr::Number(3.0));
        assert_eq!(infer_yield_ty(&ast, &[a, b], &c).as_deref(), Some("number"));
    }

    #[test]
    fn a_string_and_a_number_disagree() {
        let sigs = HashMap::new();
        let c = ctx(&sigs, &[]);
        let mut ast = Ast::default();
        let a = yield_of(&mut ast, Expr::Number(1.0));
        let b = yield_of(&mut ast, Expr::String("s".into()));
        assert_eq!(infer_yield_ty(&ast, &[a, b], &c), None);
    }

    #[test]
    fn an_any_local_or_a_valued_return_declines() {
        let sigs = HashMap::new();
        let c = ctx(&sigs, &[("x", "any")]);
        let mut ast = Ast::default();
        let a = yield_of(&mut ast, Expr::Ident("x".into()));
        assert_eq!(infer_yield_ty(&ast, &[a], &c), None);
        let b = yield_of(&mut ast, Expr::Number(1.0));
        let r = Stmt::Return(Some(ast.add_expr(Expr::Number(2.0))));
        assert_eq!(infer_yield_ty(&ast, &[b, r], &c), None);
    }

    #[test]
    fn a_body_without_a_yield_answers_nothing() {
        let sigs = HashMap::new();
        let c = ctx(&sigs, &[]);
        let ast = Ast::default();
        assert_eq!(infer_yield_ty(&ast, &[], &c), None);
    }
}
