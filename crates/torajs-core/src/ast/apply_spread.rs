//! Dynamic spread-call desugar pass (RFC 20260708-spread-call).
//!
//! `f(a0, …, ak-1, ...src)` against a fixed-arity top-level FnDecl
//! callee desugars into index-read direct expansion:
//!
//! ```text
//! f(a0, …, ak-1, src[0], src[1], …, src[need-1])
//! ```
//!
//! where `need = N - k` (N = callee user-param count). A source
//! shorter than `need` leaves the tail reading past the end, which
//! answers `undefined` (ES §10.4.2.1) exactly as the same read does
//! anywhere else. It used to be a synthesized length guard that
//! threw instead — written when reading past the end of most
//! element families threw too, so the guard was standing in front
//! of a hole rather than describing a rule. Elements flow through
//! the existing Index-read + coerce lanes (typed slot direct read
//! for `T[]`, P1.4 Arr<Any> box read for `any[]`), so checker /
//! ssa-lower / runtime need no new surface.
//!
//! Closure-value callees resolve through a let-alias walk (chunk
//! 685); `Math.min/max` and `xs.push` spreads desugar in sibling
//! walks (chunks 686/687); defaulted callees expand their defaulted
//! slots as `j < src.length ? src[j] : default` ternaries (chunk
//! 688). Shapes deliberately left alone (the checker's existing
//! loud reject — "spread `...` is only valid inside an array
//! literal" — covers them): other Member/builtin callees,
//! non-trailing or multiple spreads, non-Ident spread sources,
//! unknown callees, rest-param callees (consumed earlier by
//! `apply_rest_args`), pre-spread fixed args already exceeding the
//! declared arity, and defaulted-param-before-required shapes.
//!
//! Runs after `apply_rest_args`, before typecheck; wired at the
//! same three call sites (cli main / cmd_build / lsp).

use super::apply_args::collect_fn_defaults;
use super::{Ast, BinOp, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

/// One collected spread call site: expr index, fixed prefix args,
/// spread source ident, and the spread-covered slot plan.
struct Rewrite {
    call_idx: usize,
    prefix: Vec<ExprId>,
    src_name: String,
    need: usize,
    /// `need` entries: `None` = required slot (constant index
    /// read), `Some(eid)` = defaulted slot (ternary).
    slot_defaults: Vec<Option<ExprId>>,
}

pub fn apply_spread_args(ast: &mut Ast) {
    super::apply_spread_hoist::apply_spread_hoist(ast);
    super::apply_spread_math::apply_math_min_max_spread(ast);
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
    // Chunk 688 — defaulted callees (`function d(a, b = 5)` +
    // `d(...arr)`): `apply_default_args` skips spread-carrying calls
    // (padding would push the spread out of trailing position), so
    // the defaulted slots are expanded here as runtime length probes:
    // `j < src.length ? src[j] : <default expr>`. The guard only
    // covers the required prefix — a source shorter than the
    // defaulted tail is legal and falls back per slot. Only the
    // canonical "required prefix + defaulted tail" shape is taken;
    // a defaulted param followed by a required one stays on the
    // checker's loud reject. Semantics note (recorded): an `any[]`
    // source holding an explicit `undefined` element does NOT
    // trigger the default here (JS would) — the probe is
    // length-based.
    let fn_defaults = collect_fn_defaults(ast);
    // Collect rewrites first (see `Rewrite`), then expand.
    let mut rewrites: Vec<Rewrite> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let Expr::Ident(name) = ast.get_expr(*callee) else {
            continue;
        };
        let (arity, resolved) = match fn_arity.get(name).copied() {
            Some(a) => (a, name.clone()),
            None => match let_alias.get(name).and_then(|a| {
                fn_arity
                    .get(a)
                    .copied()
                    .or_else(|| closure_arity.get(a).copied())
                    .map(|n| (n, a.clone()))
            }) {
                Some(pair) => pair,
                None => continue,
            },
        };
        // Rotation 372 — an arguments-carrying callee must NOT be
        // index-read expanded: the expansion trims the list to the
        // declared/inferred arity, and the real argc dies with the
        // trimmed tail (`f(...s)` against a 0-param arguments-style
        // fn expanded to `f()` — arguments.length answered 0,
        // silent). The face records desugar_arguments_object left on
        // the Ast are the signal (the `arguments` idents themselves
        // are already rewritten by now); such a call keeps its
        // spread and rides the runtime lane
        // (`ssa_lower_call_spread`), whose boxed dispatch feeds the
        // true argc.
        if ast.closure_argv_fns.contains(&resolved)
            || ast.headless_argc_fns.contains(&resolved)
            || ast.closure_argc_locals.contains(name)
            || ast.closure_argv_locals.contains(name)
        {
            continue;
        }
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
        let need = arity - k;
        // Defaults for the spread-covered slots. `first_default` is
        // the split point R: params[..R] required, params[R..] must
        // all carry defaults (canonical TS shape) — otherwise skip.
        let slot_defaults = match fn_defaults.get(&resolved) {
            Some(defaults) => {
                let r = defaults.iter().position(|d| d.is_some()).unwrap_or(arity);
                if defaults[r..].iter().any(|d| d.is_none()) {
                    continue;
                }
                (k..arity).map(|p| defaults[p]).collect()
            }
            None => vec![None; need],
        };
        rewrites.push(Rewrite {
            call_idx: i,
            prefix: prefix.to_vec(),
            src_name: src_name.clone(),
            need,
            slot_defaults,
        });
    }
    if rewrites.is_empty() {
        return;
    }
    for rw in rewrites {
        expand_call(ast, rw);
    }
}

/// Rewrite one collected call site: fixed prefix + one expanded
/// arg per spread-covered slot (required slots as guarded constant
/// index reads, defaulted slots as length-probe ternaries).
fn expand_call(ast: &mut Ast, rw: Rewrite) {
    let mut new_args = rw.prefix;
    for j in 0..rw.need {
        let obj = ast.add_expr(Expr::Ident(rw.src_name.clone()));
        if let Some(default_eid) = rw.slot_defaults[j] {
            // Defaulted slot: `j < src.length ? src[j] : default`.
            // The probe branch keeps the read in bounds; the
            // default expr only evaluates when the source is
            // short (per-call, matching JS default semantics).
            let j_lit = ast.add_expr(Expr::Number(j as f64));
            let len_obj = ast.add_expr(Expr::Ident(rw.src_name.clone()));
            let len = ast.add_expr(Expr::Member {
                obj: len_obj,
                name: "length".into(),
            });
            let cond = ast.add_expr(Expr::BinOp {
                op: BinOp::Lt,
                left: j_lit,
                right: len,
            });
            let index = ast.add_expr(Expr::Number(j as f64));
            let read = ast.add_expr(Expr::Index { obj, index });
            new_args.push(ast.add_expr(Expr::Ternary {
                cond,
                then_branch: read,
                else_branch: default_eid,
            }));
            continue;
        }
        // A required slot the source is too short to cover reads
        // past the end, which is the same question `src[j]` asks
        // anywhere else and now answers the same way: `undefined`
        // (ES §10.4.2.1). It used to be a length guard that threw
        // instead, because back then reading past the end of most
        // element families threw too — the guard was standing in
        // front of a hole rather than describing a rule.
        let index = ast.add_expr(Expr::Number(j as f64));
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
