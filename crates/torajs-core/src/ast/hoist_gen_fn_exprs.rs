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
//! Capture policy: capture-free bodies only. The generator desugar has
//! no env channel (prep rewrites params + lifted lets to `this.<name>`;
//! anything else must resolve as a module-level global), so a body
//! referencing an enclosing FUNCTION local cannot be compiled — we
//! panic loudly naming the captured idents instead of silently
//! mis-binding. Module-level bindings (let / var / fn / class names)
//! are globals in tr's model and pre-bind as non-captures, matching
//! what an equivalent decl-form generator at top level sees.
//!
//! NamedEvaluation: resolved in this pass and parked in
//! `ast.genexpr_names` for the fn-name registry (V4 刀 2). It cannot
//! wait for pass 2B — that walk reads syntactic positions, and this
//! pass is what erases them.
//!
//! Known scoped loss (RFC 残面): parse-time `generator_fns` for-of
//! elem typing can't see hoisted names.

use super::{Ast, Expr, ExprId, Stmt};

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
            params,
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
        if params
            .first()
            .is_some_and(|p| p.name == crate::ast::GEN_RECV_PARAM)
        {
            ast.fnexpr_recv_fns.insert(format!("__forward_{name}"));
        }
        let captures =
            crate::ast::free_vars::free_vars_of_arrow(ast, &params, &body, &global_names);
        if !captures.is_empty() {
            panic!(
                "generator expression capturing outer binding(s) {captures:?} is not yet \
                 supported (RFC 20260713-generator-fn-value-substrate: hoisted generator \
                 bodies have no closure-env channel — move the generator to a declaration \
                 or pass the value as a parameter)"
            );
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
