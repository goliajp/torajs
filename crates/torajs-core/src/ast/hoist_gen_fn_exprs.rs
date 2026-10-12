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
//! Capture policy (rotation 348, RFC 20260912): a body capturing
//! enclosing locals compiles through the WRAPPER route — the
//! expression slot becomes an ordinary arrow
//! `(orig params) => __genexpr_N(<cell>, <caps>, orig params)`, which
//! `lift_arrow_fns` turns into a closure carrying the real env. A
//! captured name that is never reassigned (Assign / PostIncr /
//! for-of head, anywhere in the program — conservative by name) rides
//! a call-time SNAPSHOT: it is prepended as a factory param, which the
//! generator prep moves onto `this.<name>`, so the body compiles
//! untouched. A name that IS reassigned somewhere is read BY
//! REFERENCE instead: the wrapper captures it like any closure (its
//! env holds the capture box) and hands its own cell to the factory,
//! and the iterator methods bind the name from that env
//! (`GenEnvInfo`; the wrapper file has the mechanics). Captures
//! combined with a rest param or a `__genrecv` receiver keep the loud
//! panic instead of silently mis-binding. A destructuring-param
//! prefix composes fine: the prefix counts BODY stmts, which the
//! prepended params don't touch.
//!
//! NamedEvaluation: resolved in this pass and parked in
//! `ast.genexpr_names` for the fn-name registry (V4 刀 2). It cannot
//! wait for pass 2B — that walk reads syntactic positions, and this
//! pass is what erases them.
//!
//! Known scoped loss (RFC 残面): parse-time `generator_fns` for-of
//! elem typing can't see hoisted names.

use super::{Ast, Expr, ExprId, Stmt};

/// Module-level value bindings pre-bind as globals for the
/// free-variable check: fn / let / var / class names. (Classes are
/// still Stmt::ClassDecl here — this pass runs before
/// desugar_classes.)
/// `var a, b, c;` parses to a Stmt::Multi of LetDecls — without
/// unwrapping it, the trailing names read as free variables and a
/// genexpr writing one gets refused as a reassigned capture (the
/// test262 dstr-yield cases all declare `var iter, x;` this way).
fn module_global_names(ast: &Ast) -> Vec<String> {
    let mut global_names: Vec<String> = Vec::new();
    let collect = |s: &Stmt, names: &mut Vec<String>| match s {
        Stmt::FnDecl { name, .. } | Stmt::LetDecl { name, .. } | Stmt::ClassDecl { name, .. } => {
            names.push(name.clone())
        }
        _ => {}
    };
    for s in &ast.stmts {
        if let Stmt::Multi(inner) = s {
            for i in inner {
                collect(i, &mut global_names);
            }
        } else {
            collect(s, &mut global_names);
        }
    }
    // A `var` declared inside a top-level block / loop / try is a
    // global too (§8.2 VarScopedDeclarations hoist to the function or
    // script scope); `desugar_var_hoist` moves it there, but that pass
    // runs after this one, so the walk that reads the spine must
    // descend the same way. The test262 `unscopables-with` family
    // declares `var callCount` inside a block and writes it from the
    // generator — read from the spine alone it looked like a
    // reassigned capture (rotation 635).
    super::free_vars_hoisted_names::hoist_var_names(&ast.stmts, &mut global_names);
    global_names
}

pub fn hoist_gen_fn_exprs(ast: &mut Ast) {
    let mut global_names = module_global_names(ast);
    // A nested generator DECLARATION that captures joins the marked
    // set here, so the loop below treats it like a written expression
    // (rotation 634; rationale in the `_decls` sibling).
    super::hoist_gen_fn_exprs_decls::route_capturing_nested_gen_decls(ast, &global_names);
    if ast.gen_fn_exprs.is_empty() {
        return;
    }

    // RFC 20260729-fn-value-any V4 刀 2 — NamedEvaluation has to be
    // resolved HERE: replacing the expression with an Ident erases the
    // syntactic position, so pass-2B's walk (which the closure path
    // still uses) would find nothing left to name. Collected once over
    // the pre-hoist tree.
    let positions = crate::ast::collect_named_eval_positions(ast);
    let bound = super::hoist_gen_fn_exprs_names::bound_names_anywhere(ast);
    // A top-level name a frame around the expression rebinds is that
    // frame's, and the body captures it (`scope_chain`).
    let chains = super::scope_chain::chains_by_arrow(ast, &[&ast.stmts]);

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
            mut body,
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
        let shadow: &[String] = chains.get(&eid).map_or(&[], |c| c.as_slice());
        let narrowed: Vec<String>;
        let globals: &[String] = if shadow.is_empty() {
            &global_names
        } else {
            narrowed = super::scope_chain::not_rebound(&global_names, shadow);
            &narrowed
        };
        let captures = super::hoist_gen_fn_exprs_wrapper::route_wrapper(
            ast,
            i,
            eid,
            &name,
            has_recv,
            &mut params,
            &body,
            (globals, shadow),
            &bound,
        );
        // A captured name reaches the body as a param holding a
        // function value, which mints no state-machine class: the
        // parser's `yield* g()` annotations naming it widen to `any`.
        super::hoist_gen_fn_exprs_decls::widen_parser_gen_anns(&mut body, &captures);
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
