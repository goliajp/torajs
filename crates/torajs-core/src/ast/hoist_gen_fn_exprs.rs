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
//! Known scoped losses (RFC 残面): NamedEvaluation `.name` rows don't
//! mint for `__genexpr_*` (pass 2B filters on the `__closure_` prefix)
//! and parse-time `generator_fns` for-of elem typing can't see hoisted
//! names.

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

    let mut counter: usize = 0;
    for i in 0..ast.exprs.len() {
        let eid = ExprId(i as u32);
        let Some(info) = ast.gen_fn_exprs.get(&eid).copied() else {
            continue;
        };
        let name = format!("__genexpr_{counter}");
        counter += 1;
        let arrow = std::mem::replace(&mut ast.exprs[i], Expr::Ident(name.clone()));
        let Expr::ArrowFn {
            params,
            return_type,
            body,
        } = arrow
        else {
            panic!("gen_fn_exprs marker on a non-ArrowFn ExprId {eid:?} (parser contract)");
        };
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
        });
    }
}
