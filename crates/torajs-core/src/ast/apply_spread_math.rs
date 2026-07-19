//! Chunk 686 (spread longtail #1) — `Math.min(...arr)` /
//! `Math.max(...arr)` desugar walk, split out of apply_spread.rs
//! (file-size). Runs from `apply_spread_args` before the main
//! fixed-arity walk.

use super::{Ast, BinOp, Expr, Param, Stmt};

const MINARR_NAME: &str = "__torajs_math_minarr";
const MAXARR_NAME: &str = "__torajs_math_maxarr";

/// `Math.min(...arr)` / `Math.max(...arr)`: the pairwise-reduction
/// lane only takes static args, so a trailing dynamic spread
/// rewrites its Spread arg into a call to a synthesized whole-array
/// reducer (`__torajs_math_{min,max}arr(a: number[]): number` —
/// identity seed + `for` loop over `Math.{min,max}(r, a[i])`).
/// Everything downstream is existing surface: the loop guard keeps
/// the index reads bounds-proven, the 2-arg Math lane reduces, an
/// empty array answers the spec identity (§21.3.2.{24,25}), and NaN
/// propagates. A prefix stays in place (`Math.max(x, ...arr)` →
/// `Math.max(x, maxarr(arr))` — associativity holds). Non-number[]
/// sources stay on the checker's loud reject (the synth param is
/// `number[]`).
pub(super) fn apply_math_min_max_spread(ast: &mut Ast) {
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
        span: crate::lexer::Span { start: 0, end: 0 },
    });
}
