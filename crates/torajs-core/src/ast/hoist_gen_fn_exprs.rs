//! Generator function-value EXPRESSION hoist pass
//! (RFC 20260713-generator-fn-value-substrate blade 2).
//!
//! The parser emits `function*(params) { body }` expressions as real
//! `Expr::ArrowFn`s and marks each ExprId in `ast.gen_fn_exprs`. This
//! pass — which MUST run before `desugar_generators` — lifts every
//! marked ArrowFn into a top-level `function* __genexpr_N` FnDecl and
//! replaces the expression slot with `Expr::Ident("__genexpr_N")`.
//! Post-desugar the name resolves to an ordinary factory FnDecl, so
//! `(function*(){})()` dispatches through the direct fn_table path and
//! `let f = function*(){}` flows through the M2 fn-addr let →
//! CallIndirect machinery unchanged.
//!
//! Capture policy (rotation 348): a body capturing enclosing locals
//! compiles through the WRAPPER route when a call-time snapshot is
//! provably equivalent to live by-reference capture — the captured
//! names are prepended as factory params (the generator prep already
//! moves params onto `this.<name>`, so the body compiles untouched)
//! and the expression slot becomes an ordinary arrow
//! `(orig params) => __genexpr_N(<caps>, orig params)`, which
//! `lift_arrow_fns` turns into a closure carrying the real env. The
//! wrapper reads each captured name at CALL time, so the only
//! observable divergence from by-reference capture is a reassignment
//! — [`captures_snapshot_safe`] therefore refuses any captured name
//! that is ever reassigned (Assign / PostIncr) or rebound by a
//! for-of/for-in head anywhere in the program (conservative:
//! same-named bindings in unrelated scopes also refuse). Unsafe
//! captures — and captures combined with a rest param or a
//! `__genrecv` receiver — keep the loud panic instead of silently
//! mis-binding. A destructuring-param prefix composes fine: the
//! prefix counts BODY stmts, which the prepended params don't touch.
//!
//! NamedEvaluation: resolved in this pass and parked in
//! `ast.genexpr_names` for the fn-name registry (V4 刀 2). It cannot
//! wait for pass 2B — that walk reads syntactic positions, and this
//! pass is what erases them.
//!
//! Known scoped loss (RFC 残面): parse-time `generator_fns` for-of
//! elem typing can't see hoisted names.

use super::{Ast, Expr, ExprId, Stmt};

/// True when a snapshot of `captures` taken at each wrapper call is
/// observably identical to live by-reference capture: none of the
/// captured names is ever reassigned (`Assign` / `PostIncr`) or
/// rebound by a for-of / for-in loop head anywhere in the program.
/// Conservative by name — a same-named binding in an unrelated scope
/// also refuses (compound assignment already parses to `Assign`, and
/// pre-increment to `x = x + 1`, so the two expr forms cover every
/// write spelling).
fn captures_snapshot_safe(ast: &Ast, captures: &[String]) -> bool {
    let is_cap = |eid: ExprId| matches!(&ast.exprs[eid.0 as usize], Expr::Ident(n) if captures.iter().any(|c| c == n));
    let mut arrow_bodies: Vec<&Stmt> = Vec::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } if is_cap(*target) => {
                return false;
            }
            // Loop heads inside arrow bodies rebind through the same
            // stmt shapes — feed them into the walk below (`for (x
            // of xs)` with no declarator assigns the OUTER x).
            Expr::ArrowFn { body, .. } => arrow_bodies.extend(body.iter()),
            _ => {}
        }
    }
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    stack.extend(arrow_bodies);
    while let Some(s) = stack.pop() {
        match s {
            Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
                if captures.iter().any(|c| c == var_name) {
                    return false;
                }
                stack.push(body);
            }
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
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                stack.push(body)
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    stack.push(i);
                }
                stack.push(body);
            }
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
            Stmt::Block(b) | Stmt::Multi(b) => stack.extend(b.iter()),
            Stmt::FnDecl { body, .. } => stack.extend(body.iter()),
            Stmt::ClassDecl {
                methods,
                static_methods,
                ..
            } => {
                for m in methods.iter().chain(static_methods.iter()) {
                    stack.extend(m.body.iter());
                }
            }
            Stmt::ExportDecl { inner, .. } => {
                if let Some(i) = inner {
                    stack.push(i);
                }
            }
            _ => {}
        }
    }
    true
}

pub fn hoist_gen_fn_exprs(ast: &mut Ast) {
    if ast.gen_fn_exprs.is_empty() {
        return;
    }
    // Module-level value bindings pre-bind as globals for the
    // free-variable check: fn / let / var / class names. (Classes are
    // still Stmt::ClassDecl here — this pass runs before
    // desugar_classes.)
    let mut global_names: Vec<String> = Vec::new();
    for s in &ast.stmts {
        match s {
            Stmt::FnDecl { name, .. }
            | Stmt::LetDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => global_names.push(name.clone()),
            _ => {}
        }
    }

    // RFC 20260729-fn-value-any V4 刀 2 — NamedEvaluation has to be
    // resolved HERE: replacing the expression with an Ident erases the
    // syntactic position, so pass-2B's walk (which the closure path
    // still uses) would find nothing left to name. Collected once over
    // the pre-hoist tree.
    let positions = crate::ast::collect_named_eval_positions(ast);

    let mut counter: usize = 0;
    for i in 0..ast.exprs.len() {
        let eid = ExprId(i as u32);
        let Some(info) = ast.gen_fn_exprs.get(&eid).copied() else {
            continue;
        };
        let name = format!("__genexpr_{counter}");
        counter += 1;
        // §15.5.5 — NamedEvaluation applies only to ANONYMOUS
        // definitions, so a `function* x() {}` self-name wins over
        // every position; a destructuring default (§8.4.5) carries its
        // binder separately because the desugar buries the slot in a
        // ternary. No naming position at all → the empty ES name.
        let visible = ast
            .fn_expr_self_names
            .get(&eid)
            .or_else(|| ast.dstr_default_names.get(&eid))
            .or_else(|| positions.get(&eid))
            .cloned()
            .unwrap_or_default();
        ast.genexpr_names.insert(name.clone(), visible);
        let arrow = std::mem::replace(&mut ast.exprs[i], Expr::Ident(name.clone()));
        let Expr::ArrowFn {
            mut params,
            return_type,
            body,
        } = arrow
        else {
            panic!("gen_fn_exprs marker on a non-ArrowFn ExprId {eid:?} (parser contract)");
        };
        // r295 — a body whose `this` was minted to `__genrecv`
        // (fn_expr.rs prepends the receiver param): register the wrap
        // forwarder's name for FLAG_CLOSURE_RECV_FIRST, so a
        // method-shaped call of the wrapped cell seeds the receiver
        // into argv[0] — which lands exactly on the leading
        // `__genrecv` param the forwarder forwards verbatim.
        let has_recv = params
            .first()
            .is_some_and(|p| p.name == crate::ast::GEN_RECV_PARAM);
        if has_recv {
            ast.fnexpr_recv_fns.insert(format!("__forward_{name}"));
        }
        // §15.5.5 (RFC 20260810 knife 3) — the self-name binds inside
        // the body, never a capture the enclosing scope must supply. A
        // body that READS it takes the wrapper route below: the
        // wrapper ArrowFn reuses this ExprId, so `fn_expr_self_names`
        // already names it and the knife-1 self-slot binds the name
        // inside the wrapper — which then forwards its own cell as a
        // leading argument the prep pass moves onto `this.<name>`. A
        // body that WRITES it stays refused: after the prep rewrite a
        // write would be a silent `this` field store, not the §15.5.5
        // strict TypeError.
        let self_name = ast.fn_expr_self_names.get(&eid).cloned();
        let mut captures =
            crate::ast::free_vars::free_vars_of_arrow(ast, &params, &body, &global_names, None);
        let self_ref = self_name
            .as_ref()
            .is_some_and(|sn| captures.iter().any(|c| c == sn));
        if self_ref {
            let sn = self_name.as_ref().unwrap();
            if !captures_snapshot_safe(ast, std::slice::from_ref(sn)) {
                panic!(
                    "generator expression whose self-name `{sn}` is reassigned in the \
                     body is not yet supported (RFC 20260810: the factory receives the \
                     wrapper cell through a param the generator prep rewrites onto \
                     `this.{sn}`, so a write would be a silent field store instead of \
                     the strict-mode TypeError)"
                );
            }
            captures.retain(|c| c != sn);
        }
        if !captures.is_empty() || self_ref {
            // Wrapper route (module doc "Capture policy") — refuse
            // the combinations whose plumbing is unproven, and any
            // capture where a call-time snapshot is observably
            // different from live capture.
            if has_recv || params.iter().any(|p| p.is_rest) {
                panic!(
                    "generator expression capturing outer binding(s) {captures:?} combined \
                     with a rest param or receiver-threaded `this` is not yet supported \
                     (RFC 20260713-generator-fn-value-substrate residual)"
                );
            }
            if !captures_snapshot_safe(ast, &captures) {
                panic!(
                    "generator expression capturing outer binding(s) {captures:?} where a \
                     captured name is reassigned is not yet supported (RFC \
                     20260713-generator-fn-value-substrate: the hoisted body reads a \
                     call-time snapshot, and a later reassignment would be observable — \
                     move the generator to a declaration or pass the value as a parameter)"
                );
            }
            // The expression slot becomes `(orig params) =>
            // __genexpr_N(<caps>, orig params)`; lift_arrow_fns turns
            // it into a closure carrying the real env. The factory
            // gains the captured names as leading params, which the
            // generator prep moves onto `this.<name>` — the body
            // compiles untouched.
            let callee = ast.add_expr(Expr::Ident(name.clone()));
            let mut args: Vec<ExprId> =
                Vec::with_capacity(usize::from(self_ref) + captures.len() + params.len());
            // The wrapper's own cell rides first — inside the wrapper
            // the self-name resolves through the knife-1 self-slot.
            if let Some(sn) = self_name.as_ref().filter(|_| self_ref) {
                args.push(ast.add_expr(Expr::Ident(sn.clone())));
            }
            for c in &captures {
                args.push(ast.add_expr(Expr::Ident(c.clone())));
            }
            for p in &params {
                args.push(ast.add_expr(Expr::Ident(p.name.clone())));
            }
            let call = ast.add_expr(Expr::Call { callee, args });
            ast.exprs[i] = Expr::ArrowFn {
                params: params.clone(),
                return_type: None,
                body: vec![Stmt::Return(Some(call))],
            };
            let lead_params: Vec<super::Param> = self_name
                .iter()
                .filter(|_| self_ref)
                .chain(captures.iter())
                .map(|c| super::Param {
                    name: c.clone(),
                    type_ann: Some("any".to_string()),
                    default: None,
                    is_rest: false,
                })
                .collect();
            params.splice(0..0, lead_params);
        }
        // Param-destructuring prefix carries over under the hoisted
        // name so desugar_generators moves it into the __Gen ctor
        // (eager binding, same as decl form).
        if info.destr_prefix > 0 {
            ast.gen_param_destr_prefix
                .insert(name.clone(), info.destr_prefix);
        }
        // `async function*` expressions register the hoisted name in
        // async_generator_fns so the decl-form blade 4 machinery
        // applies: desugar_async leaves the factory unwrapped (calling
        // it returns the generator object per §27.6) and
        // desugar_generators gives the step methods their Promise
        // shape.
        if info.kind == crate::ast::GenFnExprKind::AsyncGenerator {
            ast.async_generator_fns.insert(name.clone());
        }
        // Later genexprs may reference earlier hoisted names (nested
        // generator expressions) — treat them as globals too.
        global_names.push(name.clone());
        ast.stmts.push(Stmt::FnDecl {
            name,
            type_params: Vec::new(),
            params,
            return_type,
            body,
            is_generator: true,
            // the hoisted decl IS the user's generator expression --
            // carry its recorded source range (B1b)
            span: ast.expr_spans[i],
        });
    }
}
