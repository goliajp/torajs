//! Dynamic spread-call desugar pass (RFC 20260708-spread-call).
//!
//! `f(a0, …, ak-1, ...src)` against a fixed-arity top-level FnDecl
//! callee desugars into index-read direct expansion:
//!
//! ```text
//! f(a0, …, ak-1,
//!   src[__torajs_spread_guard(src.length, need)],   // guard returns 0
//!   src[1], …, src[need-1])
//! ```
//!
//! where `need = N - k` (N = callee user-param count). The guard
//! throws when `src.length < need` — call args evaluate left to
//! right, so the guard runs before any element read; once it
//! passes, every constant index is in bounds and the typed-array
//! OOB lane is never reached. Elements flow through the existing
//! Index-read + coerce lanes (typed slot direct read for `T[]`,
//! P1.4 Arr<Any> box read for `any[]`), so checker / ssa-lower /
//! runtime need no new surface.
//!
//! Shapes deliberately left alone (the checker's existing loud
//! reject — "spread `...` is only valid inside an array literal" —
//! covers them): Member/builtin callees, closure-value callees,
//! non-trailing or multiple spreads, non-Ident spread sources,
//! unknown callees, rest-param callees (consumed earlier by
//! `apply_rest_args`), and pre-spread fixed args already exceeding
//! the declared arity.
//!
//! Runs after `apply_rest_args`, before typecheck; wired at the
//! same three call sites (cli main / cmd_build / lsp).

use super::{Ast, BinOp, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

const GUARD_NAME: &str = "__torajs_spread_guard";

pub fn apply_spread_args(ast: &mut Ast) {
    // Map: callee name -> fixed user-param count. Rest-param callees
    // are skipped (apply_rest_args already consumed their spread
    // call sites); closure-shape FnDecls (`__env` first param) are
    // skipped — their call sites don't name the FnDecl directly.
    //
    // Chunk 684 — `new C(...src)`: desugar_classes runs before this
    // pass and rewrites user-class News to `__new_<C>(…)` Calls, so
    // ctor spreads ride this same Call rewrite through the factory's
    // user-param arity. One carve: an arity-0 FACTORY is skipped —
    // a ctor-less derived class synthesizes an empty factory (its JS
    // default ctor forwards args to the parent, a recorded gap that
    // rejects loud), and expanding its spread to zero args would
    // flip that loud reject into silently-uninitialized fields. The
    // cost is that a genuine `constructor()` + spread stays on the
    // checker's loud reject too.
    let mut fn_arity: HashMap<String, usize> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            if params.first().is_some_and(|p| p.name == "__env") {
                continue;
            }
            let user_params: &[Param] = params;
            if user_params.last().is_some_and(|p| p.is_rest) {
                continue;
            }
            if name.starts_with("__new_") && user_params.is_empty() {
                continue;
            }
            fn_arity.insert(name.clone(), user_params.len());
        }
    }
    if fn_arity.is_empty() {
        return;
    }
    // Collect rewrites first: (call expr index, callee name, fixed
    // prefix args, spread source ident, need).
    struct Rewrite {
        call_idx: usize,
        prefix: Vec<ExprId>,
        src_name: String,
        need: usize,
    }
    let mut rewrites: Vec<Rewrite> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let Expr::Ident(name) = ast.get_expr(*callee) else {
            continue;
        };
        let Some(&arity) = fn_arity.get(name) else {
            continue;
        };
        // Exactly one spread, at the last position, over an Ident.
        let spread_count = args
            .iter()
            .filter(|a| matches!(ast.get_expr(**a), Expr::Spread { .. }))
            .count();
        if spread_count != 1 {
            continue;
        }
        let Some((&last, prefix)) = args.split_last() else {
            continue;
        };
        let Expr::Spread { expr } = ast.get_expr(last) else {
            continue;
        };
        let Expr::Ident(src_name) = ast.get_expr(*expr) else {
            continue;
        };
        let k = prefix.len();
        if k > arity {
            continue;
        }
        rewrites.push(Rewrite {
            call_idx: i,
            prefix: prefix.to_vec(),
            src_name: src_name.clone(),
            need: arity - k,
        });
    }
    if rewrites.is_empty() {
        return;
    }
    synthesize_guard(ast);
    for rw in rewrites {
        let mut new_args = rw.prefix;
        for j in 0..rw.need {
            let obj = ast.add_expr(Expr::Ident(rw.src_name.clone()));
            let index = if j == 0 {
                // First slot carries the length guard: guard(src.length,
                // need) returns 0, so `src[guard(...)]` reads slot 0
                // after the guard has run.
                let len_obj = ast.add_expr(Expr::Ident(rw.src_name.clone()));
                let len = ast.add_expr(Expr::Member {
                    obj: len_obj,
                    name: "length".into(),
                });
                let need_lit = ast.add_expr(Expr::Number(rw.need as f64));
                let guard_callee = ast.add_expr(Expr::Ident(GUARD_NAME.into()));
                ast.add_expr(Expr::Call {
                    callee: guard_callee,
                    args: vec![len, need_lit],
                })
            } else {
                ast.add_expr(Expr::Number(j as f64))
            };
            new_args.push(ast.add_expr(Expr::Index { obj, index }));
        }
        let Expr::Call { callee, .. } = &ast.exprs[rw.call_idx] else {
            unreachable!()
        };
        let callee = *callee;
        ast.exprs[rw.call_idx] = Expr::Call {
            callee,
            args: new_args,
        };
    }
}

/// `function __torajs_spread_guard(len: number, need: number): number {
///   if (len < need) { throw "spread source array shorter than
///   parameter count"; } return 0; }`
fn synthesize_guard(ast: &mut Ast) {
    let exists = ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == GUARD_NAME));
    if exists {
        return;
    }
    let len_id = ast.add_expr(Expr::Ident("len".into()));
    let need_id = ast.add_expr(Expr::Ident("need".into()));
    let cond = ast.add_expr(Expr::BinOp {
        op: BinOp::Lt,
        left: len_id,
        right: need_id,
    });
    let msg = ast.add_expr(Expr::String(
        "spread source array shorter than parameter count".into(),
    ));
    let zero = ast.add_expr(Expr::Number(0.0));
    let body = vec![
        Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Block(vec![Stmt::Throw(msg)])),
            else_branch: None,
        },
        Stmt::Return(Some(zero)),
    ];
    let num_param = |name: &str| Param {
        name: name.into(),
        type_ann: Some("number".into()),
        default: None,
        is_rest: false,
    };
    ast.stmts.push(Stmt::FnDecl {
        name: GUARD_NAME.into(),
        type_params: Vec::new(),
        params: vec![num_param("len"), num_param("need")],
        return_type: Some("number".into()),
        body,
        is_generator: false,
    });
}
