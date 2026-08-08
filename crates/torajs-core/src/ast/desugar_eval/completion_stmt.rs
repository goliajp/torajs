//! Statement completion value — the UpdateEmpty machinery (§8.4)
//! desugared onto a completion variable, for a DIRECT/INDIRECT source
//! whose final statement is NOT an ExpressionStatement. The sibling
//! trailing-expression rewrite in `completion.rs` covers the shape
//! where the tail overrides everything; here the value comes out of a
//! control-flow statement's own V accumulation:
//!
//!   eval('2; switch (x) { case a: { 3; break; } default: }')  →  3
//!
//! The model mirrors the spec mechanically:
//! - Each ExpressionStatement assigns the current domain's variable;
//!   declarations / break / continue complete empty and leave it alone.
//! - Every breakable statement (loop / switch) and every labeled
//!   statement opens its OWN V domain starting at undefined
//!   (CaseBlockEvaluation and ForBodyEvaluation both begin with
//!   V = undefined), materialized as `let __evcvN: any = undefined`
//!   in a scope-transparent `Multi` before the statement, with
//!   `outer = __evcvN` after it. A switch nested in a loop body
//!   re-declares its variable each iteration — exactly the spec's
//!   fresh V per CaseBlockEvaluation.
//! - `if` (§14.6.2) and `try` (§14.15.3) wrap their completion in
//!   UpdateEmpty(R, undefined): they share the enclosing domain but
//!   reset it to undefined first. A `finally` body's own value is
//!   discarded (F normal → completion is B), so it is left untouched.
//! - A control transfer that leaves a domain without passing its
//!   trailing `outer = inner` assignment (continue out of a switch,
//!   `break label` across domains) carries the innermost V with it
//!   per the UpdateEmpty chain — desugared as one assignment
//!   `target-domain var = current var` inserted before the jump.

use super::super::{Ast, Expr, ExprId, Stmt};

pub(super) struct Ctx {
    counter: u32,
}

/// Label names ride on the domain of the statement they annotate:
/// `l: for (...)` is ONE domain that is both the loop and the target
/// of `break l` / `continue l` — splitting them would force a `Multi`
/// between the label and the loop, and ssa_lower requires `continue l`
/// to target a label glued directly onto its loop.
enum DomainKind {
    Root,
    Loop(Vec<String>),
    Switch(Vec<String>),
    LabeledOther(Vec<String>),
}

struct Domain {
    var: String,
    kind: DomainKind,
}

/// IIFE body for a source whose tail is a statement: seed the root
/// completion variable, transform every statement against the domain
/// stack, then return the accumulated value.
pub(super) fn rewrite_trailing_stmt_completion(body: Vec<Stmt>, ast: &mut Ast) -> Vec<Stmt> {
    let mut ctx = Ctx { counter: 0 };
    let root = ctx.fresh();
    let mut domains = vec![Domain {
        var: root.clone(),
        kind: DomainKind::Root,
    }];
    let mut out = vec![let_undef(&root, ast)];
    for s in body {
        out.push(xform(s, ast, &mut ctx, &mut domains));
    }
    let ret = ast.add_expr(Expr::Ident(root));
    out.push(Stmt::Return(Some(ret)));
    out
}

impl Ctx {
    fn fresh(&mut self) -> String {
        let n = self.counter;
        self.counter += 1;
        format!("__evcv{n}")
    }
}

fn xform(s: Stmt, ast: &mut Ast, ctx: &mut Ctx, domains: &mut Vec<Domain>) -> Stmt {
    let cur = domains.last().expect("root domain").var.clone();
    match s {
        Stmt::Expr(e) => assign_stmt(&cur, e, ast),
        Stmt::Block(b) => Stmt::Block(xform_list(b, ast, ctx, domains)),
        Stmt::Multi(b) => Stmt::Multi(xform_list(b, ast, ctx, domains)),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::Multi(vec![
            reset_undef(&cur, ast),
            Stmt::If {
                cond,
                then_branch: Box::new(xform(*then_branch, ast, ctx, domains)),
                else_branch: else_branch.map(|e| Box::new(xform(*e, ast, ctx, domains))),
            },
        ]),
        Stmt::Try {
            body,
            had_catch,
            catch_param,
            catch_type,
            catch_body,
            finally_body,
        } => Stmt::Multi(vec![
            reset_undef(&cur, ast),
            Stmt::Try {
                body: xform_list(body, ast, ctx, domains),
                had_catch,
                catch_param,
                catch_type,
                catch_body: xform_list(catch_body, ast, ctx, domains),
                finally_body,
            },
        ]),
        s @ (Stmt::While { .. }
        | Stmt::DoWhile { .. }
        | Stmt::For { .. }
        | Stmt::ForOf { .. }
        | Stmt::ForOfSplitIter { .. }) => wrap_domain(
            DomainKind::Loop(Vec::new()),
            ast,
            ctx,
            domains,
            |a, c, d| xform_loop(s, a, c, d),
        ),
        s @ Stmt::Switch { .. } => wrap_domain(
            DomainKind::Switch(Vec::new()),
            ast,
            ctx,
            domains,
            |a, c, d| xform_switch(s, a, c, d),
        ),
        Stmt::Labeled { label, body } => {
            let mut labels = vec![label];
            let mut inner = *body;
            while let Stmt::Labeled { label, body } = inner {
                labels.push(label);
                inner = *body;
            }
            let kind = if is_loop(&inner) {
                DomainKind::Loop(labels.clone())
            } else if matches!(inner, Stmt::Switch { .. }) {
                DomainKind::Switch(labels.clone())
            } else {
                DomainKind::LabeledOther(labels.clone())
            };
            wrap_domain(kind, ast, ctx, domains, move |a, c, d| {
                let new_inner = match inner {
                    s @ (Stmt::While { .. }
                    | Stmt::DoWhile { .. }
                    | Stmt::For { .. }
                    | Stmt::ForOf { .. }
                    | Stmt::ForOfSplitIter { .. }) => xform_loop(s, a, c, d),
                    s @ Stmt::Switch { .. } => xform_switch(s, a, c, d),
                    s => xform(s, a, c, d),
                };
                labels
                    .into_iter()
                    .rev()
                    .fold(new_inner, |acc, l| Stmt::Labeled {
                        label: l,
                        body: Box::new(acc),
                    })
            })
        }
        Stmt::Break(target) => {
            let t = match &target {
                None => find_last(domains, |k| {
                    matches!(k, DomainKind::Loop(_) | DomainKind::Switch(_))
                }),
                Some(l) => find_label(domains, l),
            };
            carry_then(t, Stmt::Break(target), &cur, ast, domains)
        }
        Stmt::Continue(target) => {
            let t = match &target {
                None => find_last(domains, |k| matches!(k, DomainKind::Loop(_))),
                // `continue l` resumes the loop carrying that label —
                // labels live on the loop's own domain.
                Some(l) => find_last(
                    domains,
                    |k| matches!(k, DomainKind::Loop(ls) if ls.iter().any(|x| x == l)),
                ),
            };
            carry_then(t, Stmt::Continue(target), &cur, ast, domains)
        }
        other => other,
    }
}

fn is_loop(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::While { .. }
            | Stmt::DoWhile { .. }
            | Stmt::For { .. }
            | Stmt::ForOf { .. }
            | Stmt::ForOfSplitIter { .. }
    )
}

/// Rebuild a loop with only its body transformed; every other field
/// rides through untouched (a `for` init's value is discarded by
/// ForBodyEvaluation — V starts at undefined after it).
fn xform_loop(s: Stmt, ast: &mut Ast, ctx: &mut Ctx, domains: &mut Vec<Domain>) -> Stmt {
    match s {
        Stmt::While { cond, body } => Stmt::While {
            cond,
            body: Box::new(xform(*body, ast, ctx, domains)),
        },
        Stmt::DoWhile { body, cond } => Stmt::DoWhile {
            body: Box::new(xform(*body, ast, ctx, domains)),
            cond,
        },
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => Stmt::For {
            init,
            cond,
            step,
            body: Box::new(xform(*body, ast, ctx, domains)),
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
            var_name,
            var_type_ann,
            src_ident,
            i_ident,
            elem_expr,
            body: Box::new(xform(*body, ast, ctx, domains)),
            forin_obj,
            is_await,
        },
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body: Box::new(xform(*body, ast, ctx, domains)),
        },
        other => other,
    }
}

fn xform_switch(s: Stmt, ast: &mut Ast, ctx: &mut Ctx, domains: &mut Vec<Domain>) -> Stmt {
    match s {
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => Stmt::Switch {
            scrutinee,
            cases: cases
                .into_iter()
                .map(|case| super::super::SwitchCase {
                    value: case.value,
                    body: xform_list(case.body, ast, ctx, domains),
                })
                .collect(),
            default: default.map(|b| xform_list(b, ast, ctx, domains)),
        },
        other => other,
    }
}

fn xform_list(b: Vec<Stmt>, ast: &mut Ast, ctx: &mut Ctx, domains: &mut Vec<Domain>) -> Vec<Stmt> {
    b.into_iter().map(|s| xform(s, ast, ctx, domains)).collect()
}

/// Open a fresh V domain around a breakable / labeled statement:
/// `Multi[let __evcvN = undefined, <stmt>, outer = __evcvN]`.
fn wrap_domain(
    kind: DomainKind,
    ast: &mut Ast,
    ctx: &mut Ctx,
    domains: &mut Vec<Domain>,
    build: impl FnOnce(&mut Ast, &mut Ctx, &mut Vec<Domain>) -> Stmt,
) -> Stmt {
    let var = ctx.fresh();
    domains.push(Domain {
        var: var.clone(),
        kind,
    });
    let inner = build(ast, ctx, domains);
    domains.pop();
    let outer = domains.last().expect("root domain").var.clone();
    Stmt::Multi(vec![
        let_undef(&var, ast),
        inner,
        assign_ident(&outer, &var, ast),
    ])
}

/// A jump that leaves domains without passing their trailing
/// assignments carries the innermost V straight to the target domain.
/// Jumps already landing past their own domain's tail (bare break in
/// its own breakable) need nothing.
fn carry_then(
    target: Option<usize>,
    jump: Stmt,
    cur: &str,
    ast: &mut Ast,
    domains: &[Domain],
) -> Stmt {
    match target {
        Some(t) if t + 1 < domains.len() => {
            let tv = domains[t].var.clone();
            Stmt::Multi(vec![assign_ident(&tv, cur, ast), jump])
        }
        _ => jump,
    }
}

fn find_last(domains: &[Domain], pred: impl Fn(&DomainKind) -> bool) -> Option<usize> {
    domains.iter().rposition(|d| pred(&d.kind))
}

fn find_label(domains: &[Domain], label: &str) -> Option<usize> {
    domains.iter().rposition(|d| match &d.kind {
        DomainKind::Loop(ls) | DomainKind::Switch(ls) | DomainKind::LabeledOther(ls) => {
            ls.iter().any(|x| x == label)
        }
        DomainKind::Root => false,
    })
}

fn let_undef(name: &str, ast: &mut Ast) -> Stmt {
    let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
    Stmt::LetDecl {
        mutable: true,
        name: name.to_string(),
        type_ann: Some("any".to_string()),
        init: undef,
        is_var: false,
    }
}

fn reset_undef(name: &str, ast: &mut Ast) -> Stmt {
    let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
    assign_stmt(name, undef, ast)
}

fn assign_ident(target: &str, src: &str, ast: &mut Ast) -> Stmt {
    let value = ast.add_expr(Expr::Ident(src.to_string()));
    assign_stmt(target, value, ast)
}

fn assign_stmt(target: &str, value: ExprId, ast: &mut Ast) -> Stmt {
    let tgt = ast.add_expr(Expr::Ident(target.to_string()));
    let assign = ast.add_expr(Expr::Assign { target: tgt, value });
    Stmt::Expr(assign)
}
