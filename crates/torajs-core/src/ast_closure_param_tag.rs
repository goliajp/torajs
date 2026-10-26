//! bug-327 C2 — closure-valued arguments into `__fn(...)`-annotated
//! params.
//!
//! Fn-typed param annotations map to `Type::FnSig` (bare-fn-pointer
//! direct dispatch — the P3 narrow-surface ABI; see
//! cases#closure-abi-broad-surface). Anonymous function expressions
//! (`f(function() {})`, `f(() => {})`) lower to closure env blocks
//! (`Type::Closure`), so the callee's `thunk()` `blr`'d into the env
//! block's heap header — SIGBUS. This crashed every test262
//! `assert.throws` case at the `__t262_throws_runtime(thunk)` call
//! (284 of the 309 listed bug-bucket entries).
//!
//! Fix, keeping the hot path narrow: only the params that actually
//! receive a closure-shaped argument anywhere in the program are
//! retagged `__fn(` → `__cls(` (env-first CallIndirect ABI). Named-fn
//! arguments at those same params are wrapped in the existing
//! `__forward_<name>` zero-capture closure shape so both value shapes
//! reach the slot uniformly (mirrors
//! `synthesize_fn_to_closure_forwarders`'s ObjectLit arm). Params that
//! only ever see named-fn references keep direct dispatch — `reduce(xs,
//! add1)`-style hot sites are untouched.
//!
//! Pipeline position: after `lift_arrow_fns` (function expressions are
//! `Expr::Closure` by then), before `synthesize_fn_to_closure_forwarders`
//! (shared `__forward_*` namespace; both passes skip already-synthesized
//! forwarders).
//!
//! bug-553 (RFC 20260705 chunk 553) — the **return lane** mirror:
//! `function keep(f: (n) => n): (n) => n { return f; }` kept the
//! `__fn(` return annotation, so the closure env pointer flowed out
//! through the FnSig lane and the caller's `held(7)` `blr`'d the heap
//! header — same SIGBUS family. Fns whose `__fn(`-annotated return
//! yields a closure-shaped value anywhere are retagged `__cls(` on the
//! return type (an unannotated fn yielding one joins the lane with no
//! annotation to retag); `return g` named-fn sites in those fns get the same
//! `__forward_<g>` wrap; a `let h = keep(...)` result joins the
//! closure-holding idents so downstream params mark transitively.
//!
//! Approximations (sound, both directions):
//!  - Call sites are collected by a linear arena scan, so dead exprs
//!    from earlier desugars may over-mark a param. Over-marking only
//!    costs that one cold param its direct-dispatch ABI; correctness
//!    is preserved by the forwarder wrap.
//!  - Closure-holding idents are `let x = <Expr::Closure>` bindings
//!    (plus ret-marked call results) collected per-name program-wide
//!    (no scope precision). A miss (closure reaching a param or a
//!    return through a ternary, a re-assignment...) leaves that call
//!    site exactly as broken as it is today — plan-state L3b tracks
//!    the residue.

mod children;
pub(crate) mod defaults;
pub(crate) mod slot_bind;
pub(crate) mod slot_walk;

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::ast_closure_param_tag_member::CallSites;
pub(crate) use children::push_child_stmts;
use defaults::mark_closure_defaults;
use std::collections::{HashMap, HashSet};

/// Does this annotation parse to `Type::FnSig` at the SSA layer?
/// `(() => number) | null` is spelled `__nullable(__fn()->(number))`
/// and lowers to that same FnSig (see the retag sibling's
/// `retag_to_cls` for why the wrapper is transparent here), so it has
/// to come off before the test — otherwise the param is invisible to
/// the marking below and takes a closure cell down the bare-pointer
/// lane.
pub(crate) fn is_fnsig_ann(ann: &Option<String>) -> bool {
    matches!(ann, Some(s) if {
        let t = s.trim_start();
        t.strip_prefix("__nullable(").unwrap_or(t).starts_with("__fn(")
    })
}

pub fn tag_closure_arg_params(ast: &mut Ast) {
    // ── Snapshot FnDecls with at least one __fn(-annotated param.
    // fn name → [(param idx, param name)] of fn-typed params.
    let mut fn_params: HashMap<String, Vec<(usize, String)>> = HashMap::new();
    // Signatures of plain (non-closure-shaped, non-forwarder) decls
    // for forwarder synthesis.
    let mut fn_sigs: HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)> =
        HashMap::new();
    let mut existing_forwarders: HashSet<String> = HashSet::new();
    crate::ast_closure_param_tag_collect::collect_fn_decls(
        &ast.stmts,
        &mut fn_params,
        &mut fn_sigs,
        &mut existing_forwarders,
    );
    // bug-553 — FnDecls with an `__fn(`-annotated return type or none:
    // their return-site ExprIds, for the return-lane marking round
    // below (the unannotated ones read by `is_closure_valued`).
    let mut fn_returns: HashMap<String, Vec<ExprId>> = HashMap::new();
    let mut unannotated: HashSet<String> = HashSet::new();
    crate::ast_closure_param_tag_collect::collect_fn_returns(
        &ast.stmts,
        &mut fn_returns,
        &mut unannotated,
    );
    // r549 — `const t = <arrow>` is called as `t(...)`: mirror the
    // lifted decl's fn-typed params / returns under the ident so the
    // by-callee-name rounds below see those calls (see the collect
    // sibling's `closure_let_aliases`).
    let closure_aliases = crate::ast_closure_param_tag_collect::closure_let_aliases(ast);
    crate::ast_closure_param_tag_collect::mirror_closure_aliases(
        &closure_aliases,
        &mut fn_params,
        &mut fn_returns,
        &mut unannotated,
    );
    if fn_params.is_empty() && fn_returns.is_empty() {
        return;
    }

    // ── Program-wide binding snapshots (see the collect sibling's
    // per-fn docs): closure-holding lets, and named-fn alias lets
    // whose init wraps once a marked param receives them.
    let mut closure_idents = crate::ast_closure_param_tag_collect::closure_holding_lets(ast);
    let relayed = crate::ast::escaping_fn_aliases(ast, &|g| fn_sigs.contains_key(g));
    closure_idents.extend(relayed.into_iter().map(|(_, local, _)| local));
    let fn_alias_lets = crate::ast_closure_param_tag_collect::fn_alias_lets(ast, &fn_sigs);
    let bound = crate::ast_closure_param_tag::slot_bind::slot_bound_reads(ast);
    let mut alias_wraps: HashSet<String> = HashSet::new();

    // ── Mark fn-typed params that receive a closure-shaped argument
    // and `__fn(`-annotated returns that yield one. Fixpoint: a marked
    // param's name itself becomes closure-holding (it may be forwarded
    // verbatim into another fn's fn-typed param or returned), and a
    // ret-marked fn's call results are closure-shaped in turn.
    // the usage-axis seeds (see the collect sibling's doc), and the
    // calls the rounds below read with the function each sits in
    let sites = CallSites::collect(ast, &fn_params, &closure_aliases);
    let mut marked =
        crate::ast_closure_param_tag_collect::usage_seed_marks(ast, &fn_params, &closure_aliases);
    marked.extend(sites.escaped_marks(&fn_params));
    marked.extend(crate::ast_closure_param_tag_axes::nested_fn_arg_seeds(
        ast, &fn_params,
    ));
    let mut ret_marked: HashSet<String> = HashSet::new();
    loop {
        let mut changed = false;
        for e in &ast.exprs {
            let Expr::Call { callee, args } = e else {
                continue;
            };
            let Expr::Ident(fname) = ast.get_expr(*callee) else {
                continue;
            };
            let Some(fps) = fn_params.get(fname) else {
                continue;
            };
            for (idx, _) in fps {
                let Some(arg) = args.get(*idx) else {
                    continue;
                };
                if is_closure_shaped(ast, *arg, &closure_idents, &bound, &ret_marked)
                    && marked.insert((fname.clone(), *idx))
                {
                    changed = true;
                }
                // Named-fn alias into an already-marked slot — see
                // `fn_alias_lets` above.
                if marked.contains(&(fname.clone(), *idx))
                    && let Expr::Ident(x) = ast.get_expr(*arg)
                    && fn_alias_lets.contains_key(x)
                    && alias_wraps.insert(x.clone())
                {
                    closure_idents.insert(x.clone());
                    changed = true;
                }
            }
        }
        for (fname, idx) in &marked {
            if let Some(fps) = fn_params.get(fname)
                && let Some((_, pname)) = fps.iter().find(|(i, _)| i == idx)
                && closure_idents.insert(pname.clone())
            {
                changed = true;
            }
        }
        changed |= mark_closure_returns(
            ast,
            &fn_returns,
            &unannotated,
            (&closure_idents, &bound),
            &mut ret_marked,
        );
        changed |= mark_ret_lets(ast, &ret_marked, &mut closure_idents);
        // member calls, and the backward round: what reaches a marked
        // slot from the calling frame carries the closure shape too
        // (see the member sibling)
        let shaped = |e| is_closure_shaped(ast, e, &closure_idents, &bound, &ret_marked);
        let (ms, rs) = sites.round(ast, &fn_params, &fn_returns, &marked, &shaped);
        for m in ms {
            changed |= marked.insert(m);
        }
        for r in rs {
            changed |= ret_marked.insert(r);
        }
        let held = (&closure_idents, &bound);
        if mark_closure_defaults(ast, &fn_params, held, &ret_marked, &mut marked) {
            changed = true;
        }
        if !changed {
            break;
        }
    }
    if marked.is_empty() && ret_marked.is_empty() {
        return;
    }

    // ── Retag the marked params' / returns' annotations in place
    // (alias marks folded back onto their lifted decls first).
    crate::ast_closure_param_tag_collect::fold_closure_alias_marks(
        &closure_aliases,
        &mut marked,
        &mut ret_marked,
    );
    crate::ast_closure_param_tag_retag::retag_fn_decls(&mut ast.stmts, &marked);
    crate::ast_closure_param_tag_retag::retag_fn_return_types(&mut ast.stmts, &ret_marked);

    // Alias-let init sites the fixpoint elected for the wrap — their
    // rewrite rides the same forwarder machinery as direct named-fn
    // argument sites.
    let alias_init_sites: Vec<(ExprId, String)> = alias_wraps
        .iter()
        .filter_map(|x| fn_alias_lets.get(x).cloned())
        .collect();
    crate::ast_closure_param_tag_wrap::wrap_named_fn_values(
        ast,
        &fn_params,
        &fn_returns,
        &fn_sigs,
        &marked,
        &ret_marked,
        (&closure_idents, &bound),
        &alias_init_sites,
        &mut existing_forwarders,
    );
    crate::ast_closure_param_tag_axes::wrap_ns_static_values(
        ast,
        &fn_params,
        &fn_returns,
        &marked,
        &ret_marked,
        &mut existing_forwarders,
    );
}

/// One return-lane round of [`tag_closure_arg_params`]'s fixpoint: a
/// fn whose return sites yield a closure-shaped value (any value, for
/// an unannotated fn) is ret-marked. True when it marked one.
fn mark_closure_returns(
    ast: &Ast,
    fn_returns: &HashMap<String, Vec<ExprId>>,
    unannotated: &HashSet<String>,
    (closure_idents, bound): Held,
    ret_marked: &mut HashSet<String>,
) -> bool {
    let mut changed = false;
    for (fname, rets) in fn_returns {
        let valued = |r: ExprId| is_closure_valued(ast, r, closure_idents, ret_marked);
        let shaped = |r: ExprId| is_closure_shaped(ast, r, closure_idents, bound, ret_marked);
        let yields: &dyn Fn(ExprId) -> bool = if unannotated.contains(fname) {
            &valued
        } else {
            &shaped
        };
        if !ret_marked.contains(fname) && rets.iter().any(|r| yields(*r)) {
            ret_marked.insert(fname.clone());
            changed = true;
        }
    }
    changed
}

/// One round of [`tag_closure_arg_params`]'s fixpoint: a `let` bound
/// to a ret-marked fn's call result holds a closure. True when it
/// added one.
fn mark_ret_lets(
    ast: &Ast,
    ret_marked: &HashSet<String>,
    closure_idents: &mut HashSet<String>,
) -> bool {
    let mut changed = false;
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::Call { callee, .. } = ast.get_expr(*init)
            && let Expr::Ident(f) = ast.get_expr(*callee)
            && ret_marked.contains(f)
            && closure_idents.insert(name.clone())
        {
            changed = true;
        }
        push_child_stmts(s, &mut stack);
    }
    changed
}

/// The closure-holding names, and the reads of a slot-read binding
/// resolved by scope (see the slot_bind sibling).
pub(crate) type Held<'a> = (&'a HashSet<String>, &'a HashSet<ExprId>);

/// Closure-shaped value predicate shared by the param-arg and
/// return-site marking rounds: a closure literal, a closure-holding
/// ident, or a call to a fn whose return lane is closure-marked.
pub(crate) fn is_closure_shaped(
    ast: &Ast,
    eid: ExprId,
    closure_idents: &HashSet<String>,
    bound: &HashSet<ExprId>,
    ret_marked: &HashSet<String>,
) -> bool {
    closure_shape(ast, eid, closure_idents, ret_marked, Some(bound))
}

/// [`is_closure_shaped`] without the slot-read arm, for the returns of
/// an UNANNOTATED function: its return type is whatever the body
/// yields, so `function first(a: number[]) { return a[0] }` must not
/// read as returning a closure — `const inc = first(xs)` would join the
/// closure-holding names program-wide and a top-level function `inc`
/// would reach a retagged parameter unwrapped.
fn is_closure_valued(
    ast: &Ast,
    eid: ExprId,
    closure_idents: &HashSet<String>,
    ret_marked: &HashSet<String>,
) -> bool {
    closure_shape(ast, eid, closure_idents, ret_marked, None)
}

fn closure_shape(
    ast: &Ast,
    eid: ExprId,
    closure_idents: &HashSet<String>,
    ret_marked: &HashSet<String>,
    slot_reads: Option<&HashSet<ExprId>>,
) -> bool {
    let again = |e| closure_shape(ast, e, closure_idents, ret_marked, slot_reads);
    match ast.get_expr(eid) {
        Expr::Closure { .. } => true,
        // a binding a slot read initializes holds what the slot held
        Expr::Ident(n) => {
            closure_idents.contains(n) || slot_reads.is_some_and(|b| b.contains(&eid))
        }
        // a method that hands back an element (`hs.at(i)`, `hs.pop()`)
        // reads a slot as an index does
        Expr::Call { callee, .. } => {
            matches!(ast.get_expr(*callee), Expr::Ident(f) if ret_marked.contains(f))
                || (slot_reads.is_some()
                    && crate::check::face_slots::elem_source(ast, eid).is_some())
        }
        // ANY member or index read: a fn value living in a slot is
        // Closure repr across the board — struct / class fn-typed
        // fields are `__cls`-retagged (`tag_struct_field_closure_types`),
        // fn-typed array elements are Closure-repr (chunk 733), and a
        // namespace-static fn value (`Math.sin` outside a call
        // position) lowers to the interned dispatcher cell (RFC
        // 20260719-ns-static-value-reify). Left unmarked, the `__fn(`
        // param keeps the bare-fn-pointer lane and `blr`s the cell's
        // heap header — the bug-327 SIGBUS family (third value shape:
        // test262 S12.9_A4 `DD_operator(Math.sin, ...)`; fourth:
        // `__t262_throws_runtime(o.f)`, the `{ f: top_fn }` field
        // read that crashed the eval-code `*-cntns-arguments-*`
        // family). Same approximation grade as the rest of this pass:
        // a member read that would have arrived as some other repr
        // over-marks the param, which costs direct dispatch but keeps
        // the shapes uniform — a fn-typed param's argument is a fn
        // value either way, so the retag stays sound.
        Expr::Member { .. } | Expr::Index { .. } => slot_reads.is_some(),
        // The value-passthrough operators hand one of their operands
        // straight through, so the shape question is the operands'.
        // Without this a `__fn(`-annotated return whose value was
        // `k > 0 ? f1 : f2` never got ret-marked: the annotation kept
        // the bare-fn-pointer lane while the value arrived as a
        // closure cell, and calling the result `blr`'d the cell's heap
        // header — EXIT=138, the same bug-327 SIGBUS family the rest
        // of this pass exists for (553-03; the alias spelling `type
        // Thunk = () => number` was right all along because it never
        // enters this lane at all).
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => again(*then_branch) || again(*else_branch),
        Expr::Nullish { lhs, rhs } => again(*lhs) || again(*rhs),
        Expr::BinOp {
            op: crate::ast::BinOp::LAnd | crate::ast::BinOp::LOr,
            left,
            right,
        } => again(*left) || again(*right),
        Expr::Sequence { right, .. } => again(*right),
        Expr::As { expr, .. } => again(*expr),
        Expr::Assign { value, .. } => again(*value),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::lift_arrow_fns;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn pass_output(src: &str) -> Ast {
        let toks = tokenize(src).expect("lex");
        let mut ast = parse(src, &toks).expect("parse");
        lift_arrow_fns(&mut ast);
        tag_closure_arg_params(&mut ast);
        ast
    }

    fn fn_param_ann(ast: &Ast, fn_name: &str, idx: usize) -> Option<String> {
        for s in &ast.stmts {
            if let Stmt::FnDecl { name, params, .. } = s
                && name == fn_name
            {
                return params[idx].type_ann.clone();
            }
        }
        None
    }

    #[test]
    fn closure_let_alias_call_retags_lifted_param() {
        // r549 — the arrow is called by its binding name; the lifted
        // decl's user param sits behind the synthetic `__env`.
        let ast = pass_output("const t = (f: () => number) => f();\nconsole.log(t(() => 1));\n");
        let lifted = ast
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::FnDecl { name, params, .. }
                    if name.starts_with("__closure_") && params.iter().any(|p| p.name == "f") =>
                {
                    Some(name.clone())
                }
                _ => None,
            })
            .expect("lifted decl for t");
        let idx = 1;
        let ann = fn_param_ann(&ast, &lifted, idx).unwrap();
        assert!(ann.starts_with("__cls("), "expected __cls retag, got {ann}");
    }

    #[test]
    fn anon_fn_arg_retags_param() {
        let ast =
            pass_output("function f(thunk: () => void): void { thunk(); }\nf(function() {});\n");
        let ann = fn_param_ann(&ast, "f", 0).unwrap();
        assert!(ann.starts_with("__cls("), "expected __cls retag, got {ann}");
    }

    #[test]
    fn destructured_binding_arg_retags_param() {
        // `{ g }` binds `let g = src.g`: a slot read, so a cell
        let ast = pass_output(
            "function m(f: (x: number) => number): number { return f(1); }\n\
             function o({ g }: { g: (x: number) => number }) { return m(g); }\n",
        );
        let ann = fn_param_ann(&ast, "m", 0).unwrap();
        assert!(ann.starts_with("__cls("), "expected __cls retag, got {ann}");
    }

    #[test]
    fn slot_read_binding_elsewhere_keeps_named_fn_wrap() {
        // `const inc = o.inc` in another function names a number; the
        // top-level `inc` reaching the retagged param still wraps
        let ast = pass_output(
            "function m(f: (x: number) => number): number { return f(1); }\n\
             function inc(x: number): number { return x + 1; }\n\
             function c(o: { inc: number }) { const inc = o.inc; return inc; }\n\
             m((x) => x); m(inc);\n",
        );
        let wrapped = ast
            .exprs
            .iter()
            .any(|e| matches!(e, Expr::Closure { fn_name, .. } if fn_name == "__forward_inc"));
        assert!(wrapped, "expected `inc` wrapped into __forward_inc");
    }

    #[test]
    fn named_fn_arg_keeps_fnsig() {
        let ast = pass_output(
            "function f(thunk: () => void): void { thunk(); }\nfunction g(): void {}\nf(g);\n",
        );
        let ann = fn_param_ann(&ast, "f", 0).unwrap();
        assert!(
            ann.starts_with("__fn("),
            "no closure arg → no retag, got {ann}"
        );
    }

    #[test]
    fn closure_via_local_var_retags() {
        let ast = pass_output(
            "function f(thunk: () => void): void { thunk(); }\nlet h = function() {};\nf(h);\n",
        );
        let ann = fn_param_ann(&ast, "f", 0).unwrap();
        assert!(ann.starts_with("__cls("), "expected __cls retag, got {ann}");
    }

    #[test]
    fn named_fn_arg_at_retagged_param_gets_forwarder() {
        let ast = pass_output(
            "function f(thunk: () => void): void { thunk(); }\nfunction g(): void {}\nf(function() {});\nf(g);\n",
        );
        // param retagged by the anon arg…
        let ann = fn_param_ann(&ast, "f", 0).unwrap();
        assert!(ann.starts_with("__cls("));
        // …and the named arg now reaches it as a forwarder closure.
        assert!(
            ast.stmts.iter().any(|s| matches!(
                s,
                Stmt::FnDecl { name, .. } if name == "__forward_g"
            )),
            "expected __forward_g decl"
        );
        let has_fwd_closure_arg = ast
            .exprs
            .iter()
            .any(|e| matches!(e, Expr::Closure { fn_name, .. } if fn_name == "__forward_g"));
        assert!(has_fwd_closure_arg, "f(g) arg should be rewritten");
    }

    fn fn_ret_ann(ast: &Ast, fn_name: &str) -> Option<String> {
        for s in &ast.stmts {
            if let Stmt::FnDecl {
                name, return_type, ..
            } = s
                && name == fn_name
            {
                return return_type.clone();
            }
        }
        None
    }

    #[test]
    fn returned_closure_param_retags_return() {
        // bug-553 repro shape: param marked by the closure arg, and the
        // `return f` passthrough marks the return lane too.
        let ast = pass_output(
            "function keep(f: (n: number) => number): (n: number) => number { return f; }\nlet held = keep((n: number) => n * 3);\nheld(7);\n",
        );
        assert!(fn_param_ann(&ast, "keep", 0).unwrap().starts_with("__cls("));
        assert!(fn_ret_ann(&ast, "keep").unwrap().starts_with("__cls("));
    }

    #[test]
    fn returned_closure_literal_retags_return() {
        let ast = pass_output(
            "function make(): (n: number) => number { return (n: number) => n + 10; }\nlet m = make();\nm(5);\n",
        );
        assert!(fn_ret_ann(&ast, "make").unwrap().starts_with("__cls("));
    }

    #[test]
    fn returned_named_fn_only_keeps_fnsig_return() {
        let ast = pass_output(
            "function double(n: number): number { return n * 2; }\nfunction choose(): (n: number) => number { return double; }\nlet c = choose();\nc(3);\n",
        );
        assert!(fn_ret_ann(&ast, "choose").unwrap().starts_with("__fn("));
    }

    #[test]
    fn named_fn_return_in_ret_marked_fn_gets_forwarder() {
        let ast = pass_output(
            "function double(n: number): number { return n * 2; }\nfunction chooseMix(flag: boolean): (n: number) => number { if (flag) { return (n: number) => n + 100; } return double; }\nchooseMix(true);\n",
        );
        assert!(fn_ret_ann(&ast, "chooseMix").unwrap().starts_with("__cls("));
        assert!(
            ast.stmts.iter().any(|s| matches!(
                s,
                Stmt::FnDecl { name, .. } if name == "__forward_double"
            )),
            "expected __forward_double decl"
        );
        let has_fwd_return = ast
            .exprs
            .iter()
            .any(|e| matches!(e, Expr::Closure { fn_name, .. } if fn_name == "__forward_double"));
        assert!(has_fwd_return, "return double should be rewritten");
    }

    #[test]
    fn ret_marked_call_result_marks_downstream_param() {
        // let h = keep(closure) → h is closure-holding → useIt's param marks.
        let ast = pass_output(
            "function keep(f: (n: number) => number): (n: number) => number { return f; }\nfunction useIt(g: (n: number) => number): number { return g(9); }\nlet h = keep((n: number) => n * 3);\nuseIt(h);\n",
        );
        assert!(
            fn_param_ann(&ast, "useIt", 0)
                .unwrap()
                .starts_with("__cls(")
        );
    }

    #[test]
    fn replace_cb_param_retags_and_wraps_named_arg() {
        // chunk 631 — a fn-typed param consumed as a functional
        // replaceValue retags even though its only call site passes a
        // named fn, and that arg gets the forwarder wrap.
        let ast = pass_output(
            "function up(s: string): string { return s; }\nfunction g(cb: (s: string) => string): void { \"a\".replace(\"a\", cb); }\ng(up);\n",
        );
        let ann = fn_param_ann(&ast, "g", 0).unwrap();
        assert!(ann.starts_with("__cls("), "expected __cls retag, got {ann}");
        assert!(
            ast.stmts.iter().any(|s| matches!(
                s,
                Stmt::FnDecl { name, .. } if name == "__forward_up"
            )),
            "expected __forward_up decl"
        );
        let has_fwd_arg = ast
            .exprs
            .iter()
            .any(|e| matches!(e, Expr::Closure { fn_name, .. } if fn_name == "__forward_up"));
        assert!(has_fwd_arg, "g(up) arg should be rewritten");
    }

    #[test]
    fn replace_cb_name_elsewhere_leaves_unrelated_param() {
        // A replace cb that is a plain local (not any fn's param name)
        // seeds nothing — unrelated fn-typed params keep direct dispatch.
        let ast = pass_output(
            "function f(thunk: () => void): void { thunk(); }\nfunction id(s: string): string { return s; }\nf(id);\nlet r = (m: string): string => m;\n\"a\".replace(\"a\", r);\n",
        );
        let ann = fn_param_ann(&ast, "f", 0).unwrap();
        assert!(ann.starts_with("__fn("), "no seed expected, got {ann}");
    }

    #[test]
    fn fnsig_param_into_marked_param_backward_marks() {
        // chunk 632 / probe p631c shape: g3 marked by a closure call
        // site; h passes its own fn-typed param into g3 — the backward
        // round marks h's param and h(up) gets the forwarder wrap.
        let ast = pass_output(
            "function up(s: string): string { return s; }\nfunction g3(cb: (s: string) => string): void { cb(\"x\"); }\nfunction h(cb2: (s: string) => string): void { g3(cb2); }\ng3((s: string): string => s + \"!\");\nh(up);\n",
        );
        assert!(fn_param_ann(&ast, "g3", 0).unwrap().starts_with("__cls("));
        assert!(fn_param_ann(&ast, "h", 0).unwrap().starts_with("__cls("));
        assert!(
            ast.exprs
                .iter()
                .any(|e| matches!(e, Expr::Closure { fn_name, .. } if fn_name == "__forward_up")),
            "h(up) arg should be rewritten"
        );
    }

    #[test]
    fn backward_round_resolves_the_name_where_the_call_sits() {
        // `run` only shares its param's name with the forwarding `h`;
        // the object-literal method receives no closure at all
        let ast = pass_output(
            "function up(s: string): string { return s; }\nfunction g3(cb: (s: string) => string): void { cb(\"x\"); }\nfunction h(cb: (s: string) => string): void { g3(cb); }\nfunction run(cb: (s: string) => string): void { cb(\"y\"); }\nconst o = { k(cb: (s: string) => string) { cb(\"z\"); } };\ng3((s: string): string => s);\nh(up);\nrun(up);\no.k(up);\n",
        );
        assert!(fn_param_ann(&ast, "h", 0).unwrap().starts_with("__cls("));
        assert!(fn_param_ann(&ast, "run", 0).unwrap().starts_with("__fn("));
        let ks: Vec<String> = ast
            .exprs
            .iter()
            .filter_map(|e| match e {
                Expr::ObjectLit { fields } => {
                    fields.iter().find_map(|(_, v)| match ast.get_expr(*v) {
                        Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
                        _ => None,
                    })
                }
                _ => None,
            })
            .collect();
        assert_eq!(ks.len(), 1);
        assert!(fn_param_ann(&ast, &ks[0], 1).unwrap().starts_with("__fn("));
    }

    #[test]
    fn objlit_method_param_marked_through_member_calls() {
        // an arrow at `o.k(...)` marks the method's param, and the
        // param a function forwards there follows it
        let ast = pass_output(
            "const o = { k(cb: (s: string) => string) { cb(\"z\"); } };\nfunction fwd(cb: (s: string) => string): void { o.k(cb); }\no.k((s: string): string => s);\n",
        );
        assert!(fn_param_ann(&ast, "fwd", 0).unwrap().starts_with("__cls("));
    }

    #[test]
    fn pure_named_fn_chain_stays_direct_dispatch() {
        // No marked slot anywhere — the backward round must not touch
        // a named-fn-only forwarding chain.
        let ast = pass_output(
            "function up(s: string): string { return s; }\nfunction g3(cb: (s: string) => string): void { cb(\"x\"); }\nfunction h(cb2: (s: string) => string): void { g3(cb2); }\nh(up);\n",
        );
        assert!(fn_param_ann(&ast, "g3", 0).unwrap().starts_with("__fn("));
        assert!(fn_param_ann(&ast, "h", 0).unwrap().starts_with("__fn("));
    }

    #[test]
    fn ns_static_member_arg_retags_param() {
        // bug-327 third value shape: `Math.sin` outside a call
        // position is the interned dispatcher cell (Type::Closure),
        // so the receiving param needs the env-first ABI.
        let ast = pass_output(
            "function apply1(f: (n: number) => number, x: number): number { return f(x); }\napply1(Math.sin, 0);\n",
        );
        let ann = fn_param_ann(&ast, "apply1", 0).unwrap();
        assert!(ann.starts_with("__cls("), "expected __cls retag, got {ann}");
        // …and the member arg is wrapped: the cell's fn_addr is the
        // RFC B4 loud reject, so the typed lane needs a real closure.
        assert!(
            ast.stmts.iter().any(|s| matches!(
                s,
                Stmt::FnDecl { name, .. } if name == "__forward_ns_Math_sin"
            )),
            "expected __forward_ns_Math_sin decl"
        );
        assert!(
            ast.exprs.iter().any(|e| matches!(
                e,
                Expr::Closure { fn_name, .. } if fn_name == "__forward_ns_Math_sin"
            )),
            "apply1(Math.sin, ...) arg should be rewritten"
        );
    }

    #[test]
    fn member_arg_retags_closure() {
        // A member read IS closure-shaped: a fn value living in a
        // slot (struct field / array element / namespace static) is
        // Closure repr across the board, so the receiving param must
        // leave the bare-pointer lane. The old expectation (`no
        // retag`) preserved exactly the bug-327 fourth-shape SIGBUS —
        // `o.m` here is an objlit fn-expr field, a Closure cell.
        let ast = pass_output(
            "function f(thunk: () => void): void { thunk(); }\nlet o = { m: function() {} };\nf(o.m);\n",
        );
        let ann = fn_param_ann(&ast, "f", 0).unwrap();
        assert!(ann.starts_with("__cls("), "member arg retags, got {ann}");
    }

    #[test]
    fn marked_param_propagates_transitively() {
        let ast = pass_output(
            "function inner(cb: () => void): void { cb(); }\nfunction outer(t: () => void): void { inner(t); }\nouter(function() {});\n",
        );
        let outer_ann = fn_param_ann(&ast, "outer", 0).unwrap();
        let inner_ann = fn_param_ann(&ast, "inner", 0).unwrap();
        assert!(outer_ann.starts_with("__cls("), "outer: {outer_ann}");
        assert!(inner_ann.starts_with("__cls("), "inner: {inner_ann}");
    }

    const RUN: &str = "function run(cb: (x: number) => number): number { return cb(3); }\n";
    const NESTED: &str = "(f: (g: (x: number) => number) => number";

    #[test]
    fn fn_passed_to_a_nested_fn_typed_slot_takes_closure_params() {
        let src =
            format!("{RUN}function hof{NESTED}): number {{ return f((x) => x); }}\nhof(run);\n");
        let ast = pass_output(&src);
        let run_ann = fn_param_ann(&ast, "run", 0).unwrap();
        let hof_ann = fn_param_ann(&ast, "hof", 0).unwrap();
        assert!(run_ann.starts_with("__cls("), "run: {run_ann}");
        assert!(hof_ann.starts_with("__fn("), "hof keeps its own: {hof_ann}");
    }

    #[test]
    fn fn_typed_param_fed_to_a_nested_slot_takes_closure_repr() {
        let src = format!(
            "{RUN}function pass{NESTED}, g2: (x: number) => number): number {{ return f(g2); }}\n"
        );
        let ast = pass_output(&src);
        let g2_ann = fn_param_ann(&ast, "pass", 1).unwrap();
        assert!(g2_ann.starts_with("__cls("), "g2: {g2_ann}");
    }

    #[test]
    fn forwarder_of_a_retagged_fn_takes_its_closure_params() {
        // `lam`'s params are closure repr, so `run` reaches it as its
        // forwarder, which hands `cb` on to `run` unchanged
        let src = format!("{RUN}const lam = {NESTED}): number => f((x) => x);\nlam(run);\n");
        let ast = pass_output(&src);
        let fwd = ast
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::FnDecl { name, params, .. } if name == "__forward_run" => Some(params),
                _ => None,
            })
            .expect("forwarder for run");
        let cb = fwd.iter().find(|p| p.name == "cb").expect("cb param");
        let ann = cb.type_ann.as_deref().unwrap();
        assert!(ann.starts_with("__cls("), "forwarder cb: {ann}");
    }

    #[test]
    fn unannotated_fn_returning_a_closure_retags_the_receiving_param() {
        let ast = pass_output(
            "function mk(k: number) { return (y: number) => y + k; }\n\
             function caller(h: (y: number) => number) { return h(1); }\n\
             caller(mk(1));\n",
        );
        let ann = fn_param_ann(&ast, "caller", 0).unwrap();
        assert!(ann.starts_with("__cls("), "expected __cls retag, got {ann}");
    }

    #[test]
    fn unannotated_fn_returning_a_slot_read_is_not_a_closure_source() {
        // `inc` below is a local number; the top-level `inc` keeps the
        // direct lane
        let ast = pass_output(
            "function first(a: number[]) { return a[0]; }\n\
             function inc(x: number) { return x + 1; }\n\
             function apply(f: (x: number) => number) { return f(1); }\n\
             function other() { const inc = first([5]); return inc; }\n\
             console.log(other(), apply(inc));\n",
        );
        let ann = fn_param_ann(&ast, "apply", 0).unwrap();
        assert!(ann.starts_with("__fn("), "expected __fn kept, got {ann}");
    }
}
