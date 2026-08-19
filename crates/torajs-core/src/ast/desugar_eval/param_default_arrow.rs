//! Arrow-position `declare arguments` param-default evals — the legal
//! half of the §19.2.1.3 special case that `param_default` leaves out.
//!
//! An arrow has no `arguments` binding of its own, so a direct eval in
//! its default-parameter position that var-declares `arguments` is
//! LEGAL in sloppy code: EvalDeclarationInstantiation creates the
//! binding in the arrow's parameter scope, fresh per call. Every later
//! default and the body resolve a bare `arguments` to it — unless the
//! body declares its own (`var`/`let`/`function arguments`), which is
//! a separate body-scope binding that shadows only body references
//! (the t262 `arrow-fn-*-arrow-func-declare-arguments-assign` family).
//!
//! The rewrite expresses "a var binding in the parameter scope" with
//! shapes tr already lowers correctly:
//!   1. defaults move into the body as `if (p === undefined) p = D;`
//!      prefix statements (the textbook default lowering — needed
//!      because tr's native default-position lowering loses closure
//!      writes to enclosing locals, and a default cannot see body
//!      vars),
//!   2. a synthesized `var __evargs_<id>;` at the top of the body
//!      carries the binding (fresh per call, visible to the moved
//!      defaults and the body alike),
//!   3. the eval call collapses to `(__evargs = init, undefined)` —
//!      declaration hoisted into (2), assignment kept in place, eval
//!      completion undefined,
//!   4. references that lexically resolve to the parameter-scope
//!      `arguments` are α-renamed to the synthesized name: always in
//!      the moved defaults; in the body only when the body does not
//!      rebind `arguments` anywhere (`rebinds_in_stmt` — any rebind,
//!      even a deep block-scoped one, keeps the whole body untouched:
//!      honest current behavior over a wrong rename).
//!
//! When a parameter is itself named `arguments`, the declaration
//! collides with the parameter binding and the call must throw a
//! SyntaxError instead — same throw carrier `param_default` uses.
//!
//! Out of scope (kept honest): async arrows, rest / destructuring
//! parameter lists, and eval sources that mix other statements in
//! with the `var arguments` declarations.

use super::super::desugar_generators_alpha::rebinds_in_stmt;
use super::super::nested_fns_idents::arrow_rebinds;
use super::super::{Ast, BinOp, Expr, ExprId, Param, Stmt};
use super::completion;
use super::param_default::{declares_var_arguments, mark_defaults};
use super::source::{
    CallForm, DeleteSites, first_line, literal_eval_call, parse_eval_source, syntax_error_throw,
};
use std::collections::HashMap;

/// Rewrite every true-arrow whose default-parameter position holds a
/// direct literal eval that var-declares `arguments`.
pub(super) fn rewrite_arrow_param_default_arguments_evals(ast: &mut Ast) {
    let n = ast.exprs.len();
    for ai in 0..n {
        let Expr::ArrowFn { params, body, .. } = &ast.exprs[ai] else {
            continue;
        };
        if has_own_arguments(ast, ExprId(ai as u32)) {
            continue;
        }
        if params.iter().all(|p| p.default.is_none()) {
            continue;
        }
        let params_c = params.clone();
        let body_c = body.clone();
        // Ownership FIRST, parse second: only eval slots reached from
        // this arrow's defaults (without crossing another function
        // boundary) get their source parsed. Parsing is not free of
        // side effects — `parse_into` fills name-keyed side-tables —
        // so an eval that is not in arrow-default position must not
        // be parsed here (a statement-position `eval("(function f(){
        // arguments = 10 })()")` regressed to a lowering error when
        // an earlier draft pre-parsed every literal eval).
        let mut owned = vec![false; ast.exprs.len()];
        mark_defaults(ast, &params_c, &mut owned);
        let decl_evals = collect_declaring_evals(ast, &owned);
        let owned_evals: Vec<usize> = decl_evals.keys().copied().collect();
        if owned_evals.is_empty() {
            continue;
        }
        if params_c.iter().any(|p| p.name == "arguments") {
            // The declaration collides with the parameter binding —
            // EvalDeclarationInstantiation refuses it at call time.
            for slot in owned_evals {
                let throw =
                    syntax_error_throw(format!("eval: {}", first_line(&decl_evals[&slot].0)), ast);
                completion::wrap_iife(slot, vec![throw], ast);
            }
            continue;
        }
        // Legal-lane gate — shapes the rewrite can express exactly.
        if params_c
            .iter()
            .any(|p| p.is_rest || p.name.starts_with("__param_destr_"))
        {
            continue;
        }
        let pure = owned_evals
            .iter()
            .all(|s| decl_evals[s].1.iter().all(is_var_arguments_decl));
        if !pure {
            continue;
        }
        rewrite_legal_arrow(ast, ai, params_c, body_c, &owned_evals, &decl_evals);
    }
}

/// Every OWNED direct literal eval slot whose source var-declares
/// `arguments`, with its source text and parsed statements. Slots the
/// non-arrow pass already rewrote are no longer eval calls and do not
/// match; slots outside the owned map are never parsed (see the
/// caller's ownership-first note).
fn collect_declaring_evals(ast: &mut Ast, owned: &[bool]) -> HashMap<usize, (String, Vec<Stmt>)> {
    let mut found = HashMap::new();
    for i in 0..owned.len() {
        if !owned[i] {
            continue;
        }
        let eid = ExprId(i as u32);
        if let Some((src, CallForm::Direct)) = literal_eval_call(eid, ast) {
            if let Ok(stmts) = parse_eval_source(&src, ast, false, DeleteSites::Strict) {
                if declares_var_arguments(&stmts) {
                    found.insert(i, (src, stmts));
                }
            }
        }
    }
    found
}

/// The legal-lane transform: defaults into the body, synthesized var,
/// eval collapsed to assignment, α-rename.
fn rewrite_legal_arrow(
    ast: &mut Ast,
    ai: usize,
    mut params: Vec<Param>,
    mut body: Vec<Stmt>,
    owned_evals: &[usize],
    decl_evals: &HashMap<usize, (String, Vec<Stmt>)>,
) {
    let fresh = format!("__evargs_{ai}");
    for slot in owned_evals {
        collapse_eval_to_assigns(ast, *slot, &decl_evals[slot].1, &fresh);
    }
    let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
    let mut prelude = vec![Stmt::LetDecl {
        mutable: true,
        name: fresh.clone(),
        type_ann: None,
        init: undef,
        is_var: true,
    }];
    for p in params.iter_mut() {
        if let Some(d) = p.default.take() {
            let pid = ast.add_expr(Expr::Ident(p.name.clone()));
            let u = ast.add_expr(Expr::Ident("undefined".to_string()));
            let cond = ast.add_expr(Expr::BinOp {
                op: BinOp::Eq,
                left: pid,
                right: u,
            });
            let tgt = ast.add_expr(Expr::Ident(p.name.clone()));
            let asg = ast.add_expr(Expr::Assign {
                target: tgt,
                value: d,
            });
            prelude.push(Stmt::If {
                cond,
                then_branch: Box::new(Stmt::Expr(asg)),
                else_branch: None,
            });
        }
    }
    // Moved defaults sat in the parameter scope, so their bare
    // `arguments` always meant the eval's binding. The body's did too
    // — unless the body rebinds the name anywhere (then every body
    // reference belongs to a body binding or is left as-is).
    rename_arguments_in_stmts(ast, &mut prelude, &fresh);
    if !body.iter().any(|s| rebinds_in_stmt(s, "arguments")) {
        rename_arguments_in_stmts(ast, &mut body, &fresh);
    }
    let mut new_body = prelude;
    new_body.append(&mut body);
    if let Expr::ArrowFn {
        params: ps,
        body: bs,
        ..
    } = &mut ast.exprs[ai]
    {
        *ps = params;
        *bs = new_body;
    }
}

/// `var arguments = A, arguments = B` → `(__evargs = A, (__evargs = B,
/// undefined))`; declarations without initializers contribute nothing
/// (the hoisted var already exists). Written right-to-left so the
/// leftmost assignment is outermost-first in evaluation order.
fn collapse_eval_to_assigns(ast: &mut Ast, slot: usize, stmts: &[Stmt], fresh: &str) {
    let mut inits: Vec<ExprId> = Vec::new();
    collect_arguments_inits(stmts, &mut inits);
    let mut acc = ast.add_expr(Expr::Ident("undefined".to_string()));
    for init in inits.into_iter().rev() {
        let tgt = ast.add_expr(Expr::Ident(fresh.to_string()));
        let asg = ast.add_expr(Expr::Assign {
            target: tgt,
            value: init,
        });
        acc = ast.add_expr(Expr::Sequence {
            left: asg,
            right: acc,
        });
    }
    ast.exprs[slot] = ast.exprs[acc.0 as usize].clone();
}

fn collect_arguments_inits(stmts: &[Stmt], out: &mut Vec<ExprId>) {
    for s in stmts {
        match s {
            Stmt::LetDecl { init, .. } => out.push(*init),
            Stmt::Multi(inner) => collect_arguments_inits(inner, out),
            _ => {}
        }
    }
}

/// A statement the gate accepts as part of a pure declaring source:
/// nothing but var-scoped `arguments` declarations. Parser-elided
/// initializers arrive as `Ident("undefined")` inits and are handled
/// by `collect_arguments_inits` uniformly (an extra `__evargs =
/// undefined` write is the declared semantics anyway).
fn is_var_arguments_decl(s: &Stmt) -> bool {
    match s {
        Stmt::LetDecl { name, is_var, .. } => *is_var && name == "arguments",
        Stmt::Multi(inner) => inner.iter().all(is_var_arguments_decl),
        _ => false,
    }
}

fn has_own_arguments(ast: &Ast, eid: ExprId) -> bool {
    ast.fn_expr_exprs.contains(&eid)
        || ast.gen_fn_exprs.contains_key(&eid)
        || ast.objlit_method_exprs.contains(&eid)
        || ast.async_fn_value_exprs.contains(&eid)
}

/// α-rename walk for bare `arguments` references. The general
/// `nested_fns_idents` walk cannot serve here: it descends into every
/// arena `ArrowFn`, but a function expression / generator / method
/// parses as an arena `ArrowFn` too and OWNS its `arguments` — those
/// must not be renamed. This walk descends only into true arrows that
/// do not rebind the name (checking their param defaults as well,
/// which the general walk skips), and never into statement-position
/// functions or class members (own `arguments` / conservative).
fn rename_arguments_in_stmts(ast: &mut Ast, stmts: &mut [Stmt], fresh: &str) {
    for s in stmts.iter_mut() {
        rename_arguments_in_stmt(ast, s, fresh);
    }
}

fn rename_arguments_in_stmt(ast: &mut Ast, s: &mut Stmt, fresh: &str) {
    match s {
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Throw(eid) => {
            rename_arguments_in_expr(ast, *eid, fresh)
        }
        Stmt::LetDecl { init, .. } => rename_arguments_in_expr(ast, *init, fresh),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rename_arguments_in_expr(ast, *cond, fresh);
            rename_arguments_in_stmt(ast, then_branch, fresh);
            if let Some(eb) = else_branch.as_deref_mut() {
                rename_arguments_in_stmt(ast, eb, fresh);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rename_arguments_in_expr(ast, *cond, fresh);
            rename_arguments_in_stmt(ast, body, fresh);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init.as_deref_mut() {
                rename_arguments_in_stmt(ast, i, fresh);
            }
            if let Some(c) = cond {
                rename_arguments_in_expr(ast, *c, fresh);
            }
            if let Some(st) = step {
                rename_arguments_in_expr(ast, *st, fresh);
            }
            rename_arguments_in_stmt(ast, body, fresh);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            rename_arguments_in_expr(ast, *elem_expr, fresh);
            rename_arguments_in_stmt(ast, body, fresh);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rename_arguments_in_expr(ast, *parent, fresh);
            rename_arguments_in_expr(ast, *sep, fresh);
            rename_arguments_in_stmt(ast, body, fresh);
        }
        Stmt::Labeled { body, .. } => rename_arguments_in_stmt(ast, body, fresh),
        Stmt::Block(b) | Stmt::Multi(b) => rename_arguments_in_stmts(ast, b, fresh),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rename_arguments_in_stmts(ast, body, fresh);
            rename_arguments_in_stmts(ast, catch_body, fresh);
            if let Some(fb) = finally_body {
                rename_arguments_in_stmts(ast, fb, fresh);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rename_arguments_in_expr(ast, *scrutinee, fresh);
            for c in cases.iter_mut() {
                rename_arguments_in_expr(ast, c.value, fresh);
                rename_arguments_in_stmts(ast, &mut c.body, fresh);
            }
            if let Some(d) = default {
                rename_arguments_in_stmts(ast, d, fresh);
            }
        }
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => {
            rename_arguments_in_expr(ast, *eid, fresh)
        }
        _ => {}
    }
}

fn rename_arguments_in_expr(ast: &mut Ast, eid: ExprId, fresh: &str) {
    use std::collections::HashSet;
    let mut seen: HashSet<ExprId> = HashSet::new();
    let mut stack: Vec<ExprId> = vec![eid];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if matches!(&ast.exprs[id.0 as usize], Expr::Ident(n) if n == "arguments") {
            ast.exprs[id.0 as usize] = Expr::Ident(fresh.to_string());
            continue;
        }
        match ast.exprs[id.0 as usize].clone() {
            Expr::BinOp { left, right, .. }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            }
            | Expr::Sequence { left, right }
            | Expr::Assign {
                target: left,
                value: right,
            }
            | Expr::Index {
                obj: left,
                index: right,
            }
            | Expr::OptIndex {
                obj: left,
                index: right,
            }
            | Expr::InstanceOf {
                expr: left,
                rhs: right,
            } => {
                stack.push(left);
                stack.push(right);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::Delete { expr }
            | Expr::As { expr, .. }
            | Expr::PostIncr { target: expr, .. } => stack.push(expr),
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => stack.push(obj),
            Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            Expr::NewDynamic { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    stack.push(a);
                }
            }
            Expr::Array(els) => {
                for e in els {
                    stack.push(e);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    stack.push(e);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                stack.push(cond);
                stack.push(then_branch);
                stack.push(else_branch);
            }
            Expr::ArrowFn { params, body, .. } => {
                if has_own_arguments(ast, id) || arrow_rebinds(&params, &body, "arguments") {
                    continue;
                }
                for p in &params {
                    if let Some(d) = p.default {
                        stack.push(d);
                    }
                }
                let mut inner = body;
                rename_arguments_in_stmts(ast, &mut inner, fresh);
                if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[id.0 as usize] {
                    *slot = inner;
                }
            }
            _ => {}
        }
    }
}
