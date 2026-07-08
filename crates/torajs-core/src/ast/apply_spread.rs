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
const MINARR_NAME: &str = "__torajs_math_minarr";
const MAXARR_NAME: &str = "__torajs_math_maxarr";

pub fn apply_spread_args(ast: &mut Ast) {
    apply_math_min_max_spread(ast);
    super::apply_spread_push::apply_spread_push(ast);
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
    // Chunk 685 — closure-value callees (`const f = (a, b) => …;
    // f(...arr)`): the lifted FnDecl is keyed by the synthetic
    // `__closure_N` name, so the call site's `f` resolves through a
    // let-alias walk (the apply_default_args shape). A capturing
    // closure's `__env` first param is peeled for the user arity;
    // its FnDecl only enters `closure_arity` (call sites never name
    // it directly).
    let mut fn_arity: HashMap<String, usize> = HashMap::new();
    let mut closure_arity: HashMap<String, usize> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            let is_closure = params.first().is_some_and(|p| p.name == "__env");
            let user_params: &[Param] = if is_closure { &params[1..] } else { params };
            if user_params.last().is_some_and(|p| p.is_rest) {
                continue;
            }
            if is_closure {
                closure_arity.insert(name.clone(), user_params.len());
                continue;
            }
            if name.starts_with("__new_") && user_params.is_empty() {
                continue;
            }
            fn_arity.insert(name.clone(), user_params.len());
        }
    }
    if fn_arity.is_empty() && closure_arity.is_empty() {
        return;
    }
    // Alias map: `let f = __closure_N` (or `Closure{fn_name}`) — a
    // call to `f(...)` adopts the lifted closure's arity.
    let mut let_alias: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::LetDecl { name, init, .. } = s {
            let target = match ast.get_expr(*init) {
                Expr::Ident(n) if n.starts_with("__closure_") => Some(n.clone()),
                Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
                _ => None,
            };
            if let Some(t) = target {
                let_alias.insert(name.clone(), t);
            }
        }
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
        let arity = match fn_arity.get(name).copied().or_else(|| {
            let_alias.get(name).and_then(|a| {
                fn_arity
                    .get(a)
                    .copied()
                    .or_else(|| closure_arity.get(a).copied())
            })
        }) {
            Some(a) => a,
            None => continue,
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

/// Chunk 686 (spread longtail #1) — `Math.min(...arr)` /
/// `Math.max(...arr)`: the pairwise-reduction lane only takes
/// static args, so a trailing dynamic spread rewrites its Spread
/// arg into a call to a synthesized whole-array reducer
/// (`__torajs_math_{min,max}arr(a: number[]): number` — identity
/// seed + `for` loop over `Math.{min,max}(r, a[i])`). Everything
/// downstream is existing surface: the loop guard keeps the index
/// reads bounds-proven, the 2-arg Math lane reduces, an empty
/// array answers the spec identity (§21.3.2.{24,25}), and NaN
/// propagates. A prefix stays in place (`Math.max(x, ...arr)` →
/// `Math.max(x, maxarr(arr))` — associativity holds). Non-number[]
/// sources stay on the checker's loud reject (the synth param is
/// `number[]`).
fn apply_math_min_max_spread(ast: &mut Ast) {
    struct MathRewrite {
        call_idx: usize,
        spread_arg_pos: usize,
        src_name: String,
        is_min: bool,
    }
    let mut rewrites: Vec<MathRewrite> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        let is_min = name == "min";
        if !is_min && name != "max" {
            continue;
        }
        if !matches!(ast.get_expr(*obj), Expr::Ident(ns) if ns == "Math") {
            continue;
        }
        let spread_count = args
            .iter()
            .filter(|a| matches!(ast.get_expr(**a), Expr::Spread { .. }))
            .count();
        if spread_count != 1 {
            continue;
        }
        let Some((&last, _)) = args.split_last() else {
            continue;
        };
        let Expr::Spread { expr } = ast.get_expr(last) else {
            continue;
        };
        let Expr::Ident(src_name) = ast.get_expr(*expr) else {
            continue;
        };
        rewrites.push(MathRewrite {
            call_idx: i,
            spread_arg_pos: args.len() - 1,
            src_name: src_name.clone(),
            is_min,
        });
    }
    if rewrites.is_empty() {
        return;
    }
    let (need_min, need_max) = (
        rewrites.iter().any(|r| r.is_min),
        rewrites.iter().any(|r| !r.is_min),
    );
    if need_min {
        synthesize_math_reducer(ast, true);
    }
    if need_max {
        synthesize_math_reducer(ast, false);
    }
    for rw in rewrites {
        let reducer = if rw.is_min { MINARR_NAME } else { MAXARR_NAME };
        let callee = ast.add_expr(Expr::Ident(reducer.into()));
        let src = ast.add_expr(Expr::Ident(rw.src_name));
        let reduced = ast.add_expr(Expr::Call {
            callee,
            args: vec![src],
        });
        let Expr::Call { args, .. } = &mut ast.exprs[rw.call_idx] else {
            unreachable!()
        };
        args[rw.spread_arg_pos] = reduced;
    }
}

/// `function __torajs_math_{min,max}arr(a: number[]): number {
///   let r: number = <identity>;
///   for (let i: number = 0; i < a.length; i = i + 1) {
///     r = Math.{min,max}(r, a[i]);
///   }
///   return r; }`
fn synthesize_math_reducer(ast: &mut Ast, is_min: bool) {
    let (fname, method, identity) = if is_min {
        (MINARR_NAME, "min", f64::INFINITY)
    } else {
        (MAXARR_NAME, "max", f64::NEG_INFINITY)
    };
    if ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == fname))
    {
        return;
    }
    let identity_lit = ast.add_expr(Expr::Number(identity));
    let r_decl = Stmt::LetDecl {
        mutable: true,
        name: "r".into(),
        type_ann: Some("number".into()),
        init: identity_lit,
        is_var: false,
    };
    let zero = ast.add_expr(Expr::Number(0.0));
    let i_decl = Stmt::LetDecl {
        mutable: true,
        name: "i".into(),
        type_ann: Some("number".into()),
        init: zero,
        is_var: false,
    };
    let i_ref = ast.add_expr(Expr::Ident("i".into()));
    let a_ref = ast.add_expr(Expr::Ident("a".into()));
    let len = ast.add_expr(Expr::Member {
        obj: a_ref,
        name: "length".into(),
    });
    let cond = ast.add_expr(Expr::BinOp {
        op: BinOp::Lt,
        left: i_ref,
        right: len,
    });
    let i_ref2 = ast.add_expr(Expr::Ident("i".into()));
    let one = ast.add_expr(Expr::Number(1.0));
    let i_plus = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: i_ref2,
        right: one,
    });
    let i_tgt = ast.add_expr(Expr::Ident("i".into()));
    let step = ast.add_expr(Expr::Assign {
        target: i_tgt,
        value: i_plus,
    });
    let math_ns = ast.add_expr(Expr::Ident("Math".into()));
    let math_m = ast.add_expr(Expr::Member {
        obj: math_ns,
        name: method.into(),
    });
    let r_ref = ast.add_expr(Expr::Ident("r".into()));
    let a_ref2 = ast.add_expr(Expr::Ident("a".into()));
    let i_ref3 = ast.add_expr(Expr::Ident("i".into()));
    let elem = ast.add_expr(Expr::Index {
        obj: a_ref2,
        index: i_ref3,
    });
    let reduce = ast.add_expr(Expr::Call {
        callee: math_m,
        args: vec![r_ref, elem],
    });
    let r_tgt = ast.add_expr(Expr::Ident("r".into()));
    let body_assign = ast.add_expr(Expr::Assign {
        target: r_tgt,
        value: reduce,
    });
    let r_ret = ast.add_expr(Expr::Ident("r".into()));
    let for_stmt = Stmt::For {
        init: Some(Box::new(i_decl)),
        cond: Some(cond),
        step: Some(step),
        body: Box::new(Stmt::Block(vec![Stmt::Expr(body_assign)])),
    };
    ast.stmts.push(Stmt::FnDecl {
        name: fname.into(),
        type_params: Vec::new(),
        params: vec![Param {
            name: "a".into(),
            type_ann: Some("number[]".into()),
            default: None,
            is_rest: false,
        }],
        return_type: Some("number".into()),
        body: vec![r_decl, for_stmt, Stmt::Return(Some(r_ret))],
        is_generator: false,
    });
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
