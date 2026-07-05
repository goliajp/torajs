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
//! return type; `return g` named-fn sites in those fns get the same
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

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::{HashMap, HashSet};

/// Does this annotation parse to `Type::FnSig` at the SSA layer?
fn is_fnsig_ann(ann: &Option<String>) -> bool {
    matches!(ann, Some(s) if s.trim_start().starts_with("__fn("))
}

pub fn tag_closure_arg_params(ast: &mut Ast) {
    // ── Snapshot FnDecls with at least one __fn(-annotated param.
    // fn name → [(param idx, param name)] of fn-typed params.
    let mut fn_params: HashMap<String, Vec<(usize, String)>> = HashMap::new();
    // Signatures of plain (non-closure-shaped, non-forwarder) decls
    // for forwarder synthesis.
    let mut fn_sigs: HashMap<String, (Vec<Param>, Option<String>)> = HashMap::new();
    let mut existing_forwarders: HashSet<String> = HashSet::new();
    collect_fn_decls(
        &ast.stmts,
        &mut fn_params,
        &mut fn_sigs,
        &mut existing_forwarders,
    );
    // bug-553 — FnDecls with an `__fn(`-annotated return type: their
    // return-site ExprIds, for the return-lane marking round below.
    let mut fn_returns: HashMap<String, Vec<ExprId>> = HashMap::new();
    collect_fn_returns(&ast.stmts, &mut fn_returns);
    if fn_params.is_empty() && fn_returns.is_empty() {
        return;
    }

    // ── Closure-holding let bindings, program-wide by name.
    let mut closure_idents: HashSet<String> = HashSet::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::LetDecl { name, init, .. } = s
            && matches!(ast.get_expr(*init), Expr::Closure { .. })
        {
            closure_idents.insert(name.clone());
        }
        push_child_stmts(s, &mut stack);
    }

    // ── Mark fn-typed params that receive a closure-shaped argument
    // and `__fn(`-annotated returns that yield one. Fixpoint: a marked
    // param's name itself becomes closure-holding (it may be forwarded
    // verbatim into another fn's fn-typed param or returned), and a
    // ret-marked fn's call results are closure-shaped in turn.
    let mut marked: HashSet<(String, usize)> = HashSet::new();
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
                if is_closure_shaped(ast, *arg, &closure_idents, &ret_marked)
                    && marked.insert((fname.clone(), *idx))
                {
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
        for (fname, rets) in &fn_returns {
            if !ret_marked.contains(fname)
                && rets
                    .iter()
                    .any(|r| is_closure_shaped(ast, *r, &closure_idents, &ret_marked))
            {
                ret_marked.insert(fname.clone());
                changed = true;
            }
        }
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
        if !changed {
            break;
        }
    }
    if marked.is_empty() && ret_marked.is_empty() {
        return;
    }

    // ── Retag the marked params' / returns' annotations in place.
    retag_fn_decls(&mut ast.stmts, &marked);
    retag_fn_return_types(&mut ast.stmts, &ret_marked);

    // ── Named-fn values reaching a retagged lane need the env-first
    // shape too: `f(g)` args at marked params and `return g` sites in
    // ret-marked fns flip to `__forward_<g>` closure values, synthesizing
    // the forwarder decl (same shape as synthesize_fn_to_closure_forwarders).
    // A closure-holding name shadowing a global fn stays unwrapped —
    // the value already carries the env-first shape.
    let mut wrap_sites: Vec<(ExprId, String)> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(fname) = ast.get_expr(*callee) else {
            continue;
        };
        for (idx, _) in fn_params.get(fname).into_iter().flatten() {
            if !marked.contains(&(fname.clone(), *idx)) {
                continue;
            }
            if let Some(arg) = args.get(*idx)
                && let Expr::Ident(g) = ast.get_expr(*arg)
                && fn_sigs.contains_key(g)
                && !closure_idents.contains(g)
            {
                wrap_sites.push((*arg, g.clone()));
            }
        }
    }
    for (fname, rets) in &fn_returns {
        if !ret_marked.contains(fname) {
            continue;
        }
        for r in rets {
            if let Expr::Ident(g) = ast.get_expr(*r)
                && fn_sigs.contains_key(g)
                && !closure_idents.contains(g)
            {
                wrap_sites.push((*r, g.clone()));
            }
        }
    }
    if wrap_sites.is_empty() {
        return;
    }
    let mut renames: HashMap<String, String> = HashMap::new();
    let mut new_decls: Vec<Stmt> = Vec::new();
    for (_, target) in &wrap_sites {
        if renames.contains_key(target) {
            continue;
        }
        let forward_name = format!("__forward_{target}");
        if !existing_forwarders.contains(&forward_name) {
            let (params, return_type) = fn_sigs.get(target).unwrap().clone();
            let mut fwd_params: Vec<Param> = Vec::with_capacity(params.len() + 1);
            fwd_params.push(Param {
                name: "__env".into(),
                type_ann: Some("__env()".to_string()),
                default: None,
                is_rest: false,
            });
            fwd_params.extend(params.iter().cloned());
            let arg_eids: Vec<ExprId> = params
                .iter()
                .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
                .collect();
            let callee_id = ast.add_expr(Expr::Ident(target.clone()));
            let call_id = ast.add_expr(Expr::Call {
                callee: callee_id,
                args: arg_eids,
            });
            new_decls.push(Stmt::FnDecl {
                name: forward_name.clone(),
                type_params: Vec::new(),
                params: fwd_params,
                return_type,
                body: vec![Stmt::Return(Some(call_id))],
                is_generator: false,
            });
            existing_forwarders.insert(forward_name.clone());
        }
        renames.insert(target.clone(), forward_name);
    }
    for (eid, target) in wrap_sites {
        let forward_name = renames.get(&target).unwrap();
        ast.exprs[eid.0 as usize] = Expr::Closure {
            fn_name: forward_name.clone(),
            captures: Vec::new(),
        };
    }
    ast.stmts.extend(new_decls);
}

/// Closure-shaped value predicate shared by the param-arg and
/// return-site marking rounds: a closure literal, a closure-holding
/// ident, or a call to a fn whose return lane is closure-marked.
fn is_closure_shaped(
    ast: &Ast,
    eid: ExprId,
    closure_idents: &HashSet<String>,
    ret_marked: &HashSet<String>,
) -> bool {
    match ast.get_expr(eid) {
        Expr::Closure { .. } => true,
        Expr::Ident(n) => closure_idents.contains(n),
        Expr::Call { callee, .. } => {
            matches!(ast.get_expr(*callee), Expr::Ident(f) if ret_marked.contains(f))
        }
        _ => false,
    }
}

/// Snapshot, per FnDecl with an `__fn(`-annotated return type, the
/// ExprIds of every `return <e>` in its own body. A nested FnDecl
/// switches context — its returns attribute to itself, never to the
/// enclosing fn.
fn collect_fn_returns(stmts: &[Stmt], out: &mut HashMap<String, Vec<ExprId>>) {
    let mut stack: Vec<(&Stmt, Option<String>)> = stmts.iter().map(|s| (s, None)).collect();
    while let Some((s, cur)) = stack.pop() {
        if let Stmt::FnDecl {
            name,
            return_type,
            body,
            ..
        } = s
        {
            let ctx = if is_fnsig_ann(return_type) {
                Some(name.clone())
            } else {
                None
            };
            for b in body {
                stack.push((b, ctx.clone()));
            }
            continue;
        }
        if let Stmt::Return(Some(eid)) = s
            && let Some(f) = &cur
        {
            out.entry(f.clone()).or_default().push(*eid);
        }
        let mut kids: Vec<&Stmt> = Vec::new();
        push_child_stmts(s, &mut kids);
        for k in kids {
            stack.push((k, cur.clone()));
        }
    }
}

/// Apply `__fn(` → `__cls(` on every ret-marked FnDecl's return type.
fn retag_fn_return_types(stmts: &mut [Stmt], ret_marked: &HashSet<String>) {
    let mut stack: Vec<&mut Stmt> = stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name, return_type, ..
        } = s
            && ret_marked.contains(name)
            && let Some(ann) = return_type
            && let Some(rest) = ann.strip_prefix("__fn(")
        {
            *ann = format!("__cls({rest}");
        }
        push_child_stmts_mut(s, &mut stack);
    }
}

/// Recursively snapshot FnDecls (top-level and nested) into the
/// param-index / signature maps.
fn collect_fn_decls(
    stmts: &[Stmt],
    fn_params: &mut HashMap<String, Vec<(usize, String)>>,
    fn_sigs: &mut HashMap<String, (Vec<Param>, Option<String>)>,
    existing_forwarders: &mut HashSet<String>,
) {
    let mut stack: Vec<&Stmt> = stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } = s
        {
            if name.starts_with("__forward_") {
                existing_forwarders.insert(name.clone());
            } else {
                let is_closure_shaped = params.first().is_some_and(|p| p.name == "__env");
                let fnsig_params: Vec<(usize, String)> = params
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| is_fnsig_ann(&p.type_ann))
                    .map(|(i, p)| (i, p.name.clone()))
                    .collect();
                if !fnsig_params.is_empty() {
                    fn_params.insert(name.clone(), fnsig_params);
                }
                if !is_closure_shaped {
                    fn_sigs.insert(name.clone(), (params.clone(), return_type.clone()));
                }
            }
        }
        push_child_stmts(s, &mut stack);
    }
}

/// Apply `__fn(` → `__cls(` on every marked (fn, param idx).
fn retag_fn_decls(stmts: &mut [Stmt], marked: &HashSet<(String, usize)>) {
    let mut stack: Vec<&mut Stmt> = stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl { name, params, .. } = s {
            for (i, p) in params.iter_mut().enumerate() {
                if marked.contains(&(name.clone(), i))
                    && let Some(ann) = &mut p.type_ann
                    && let Some(rest) = ann.strip_prefix("__fn(")
                {
                    *ann = format!("__cls({rest}");
                }
            }
        }
        push_child_stmts_mut(s, &mut stack);
    }
}

/// Push every Stmt nested inside `s` (one level) onto the walk stack.
fn push_child_stmts<'a>(s: &'a Stmt, stack: &mut Vec<&'a Stmt>) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stack.push(then_branch);
            if let Some(e) = else_branch {
                stack.push(e);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stack.push(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                stack.push(i);
            }
            stack.push(body);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => stack.push(body),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                stack.extend(c.body.iter());
            }
            if let Some(d) = default {
                stack.extend(d.iter());
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            stack.extend(body.iter());
            stack.extend(catch_body.iter());
            if let Some(f) = finally_body {
                stack.extend(f.iter());
            }
        }
        Stmt::Block(inner) | Stmt::Multi(inner) => stack.extend(inner.iter()),
        Stmt::FnDecl { body, .. } => stack.extend(body.iter()),
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner {
                stack.push(i);
            }
        }
        _ => {}
    }
}

/// Mutable twin of [`push_child_stmts`].
fn push_child_stmts_mut<'a>(s: &'a mut Stmt, stack: &mut Vec<&'a mut Stmt>) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stack.push(then_branch);
            if let Some(e) = else_branch {
                stack.push(e);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stack.push(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                stack.push(i);
            }
            stack.push(body);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => stack.push(body),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                stack.extend(c.body.iter_mut());
            }
            if let Some(d) = default {
                stack.extend(d.iter_mut());
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            stack.extend(body.iter_mut());
            stack.extend(catch_body.iter_mut());
            if let Some(f) = finally_body {
                stack.extend(f.iter_mut());
            }
        }
        Stmt::Block(inner) | Stmt::Multi(inner) => stack.extend(inner.iter_mut()),
        Stmt::FnDecl { body, .. } => stack.extend(body.iter_mut()),
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner {
                stack.push(i);
            }
        }
        _ => {}
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
        let mut ast = parse(&toks).expect("parse");
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
    fn anon_fn_arg_retags_param() {
        let ast =
            pass_output("function f(thunk: () => void): void { thunk(); }\nf(function() {});\n");
        let ann = fn_param_ann(&ast, "f", 0).unwrap();
        assert!(ann.starts_with("__cls("), "expected __cls retag, got {ann}");
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
    fn marked_param_propagates_transitively() {
        let ast = pass_output(
            "function inner(cb: () => void): void { cb(); }\nfunction outer(t: () => void): void { inner(t); }\nouter(function() {});\n",
        );
        let outer_ann = fn_param_ann(&ast, "outer", 0).unwrap();
        let inner_ann = fn_param_ann(&ast, "inner", 0).unwrap();
        assert!(outer_ann.starts_with("__cls("), "outer: {outer_ann}");
        assert!(inner_ann.starts_with("__cls("), "inner: {inner_ann}");
    }
}
