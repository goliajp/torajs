//! The shared post-parse AST desugar sequence — `tr run` (main.rs)
//! and `tr build` (cmd_build.rs) ran byte-identical copies of this
//! 31-pass chain; one home keeps the two from drifting when a pass
//! lands (the REPL and LSP keep their own reduced pipelines).
//!
//! Ordering notes live where they bind:
//! - `lift_arrow_fns` (M2 Phase A) — lifts arrow fns to top-level
//!   FnDecls so check.rs's global-fn machinery resolves them.
//!   Non-capturing closures only; captures land in Phase B.
//! - `desugar_dflt_param_tdz` — after `desugar_classes` (methods are
//!   flat `__cm_` FnDecls, the `__new_*` error factories exist),
//!   before `materialize_expr_defaults` (which would otherwise move
//!   a bare self/later-param read into the body where it reads the
//!   undefined-bound parameter instead of throwing) — §8.6.2.
//! - `bind_this_param` — after `desugar_classes` (that pass turns
//!   `this` into the name `__this`; this one gives plain functions
//!   somewhere to bind it — RFC 20260726 blade 1).
//! - `synthesize_fn_constructors` — reads the receiver parameter the
//!   pass above may have added (blade 2).
//! - `route_non_class_new` — every factory that will exist exists by
//!   now; a `new <name>()` still holding a name nobody claimed is
//!   constructing a value (S-NEW 刀 4).
//! - `desugar_capturing_nested_fns` — a nested `function` reading an
//!   outer local wants the closure lane, not the top-level lift;
//!   must precede `lift_arrow_fns` (its rewrite emits a function
//!   expression, and that is the pass which gives one its env).
//! - `desugar_nested_fns` — before the fn-to-closure collector (RFC
//!   20260717 O3): lifted nested names are top-level by then.
//! - `synthesize_recv_cb_forwarders` — claims its HOF callback sites
//!   BEFORE the plain fn-to-closure wrap (the rewrite turns the
//!   Ident into `Expr::Closure`, which cluster #1 then skips).

use torajs_core::{ast, ast_closure_param_tag};

/// The passes that run BEFORE the chain below, plus the two raw-AST
/// early-error gates that bracket them.
///
/// These stayed copy-pasted in `tr run` and `tr build` when the
/// 31-pass chain got a home, for one reason: the gates return
/// `ExitCode` from one caller and `Err(ExitCode)` from the other. So
/// this reports the failure as `Err(())` — having already printed the
/// diagnostics, which the two spelled identically — and each caller
/// wraps it in its own shape. The pass list itself, the thing a new
/// pass would have to be added to twice, is now written once.
///
/// (The REPL and LSP keep their own reduced pipelines.)
pub(crate) fn run_ast_prelude(ast: &mut ast::Ast) -> Result<(), ()> {
    // Block/CaseBlock redeclaration early errors — must see the RAW
    // AST before the generator / async / var-hoist desugars move one
    // side of a conflict away.
    ast::early_redecl_errors(ast);
    if !ast.redecl_parse_errors.is_empty() {
        for msg in &ast.redecl_parse_errors {
            eprintln!("parse error: {msg}");
        }
        return Err(());
    }
    // After the redeclaration early errors, which want the raw AST —
    // and a declaration inlined out of an eval is not an early error
    // anyway (§19.2.1.1 makes an eval-introduced conflict a runtime
    // SyntaxError). Before everything else, so the inlined statements
    // reach every desugar below exactly as if they had been written at
    // the call site, which is what direct eval means.
    ast::desugar_eval(ast);
    ast::unwrap_exports(ast);
    ast::rename_user_main(ast);
    ast::desugar_using(ast);
    // RFC 20260809 B5 — after `desugar_using` (the classes contain no
    // `using`), before `desugar_async` (their walk helpers are async
    // fns the ordinary async desugar state-machines).
    ast::inject_disposable_stack(ast);
    ast::hoist_gen_fn_exprs(ast);
    ast::desugar_generators(ast);
    ast::desugar_async(ast);
    ast::desugar_builtin_imports(ast);
    ast::desugar_builtin_new(ast);
    ast::desugar_regex_syntax_error(ast);
    ast::desugar_promise_try(ast);
    if !ast.regex_parse_errors.is_empty() {
        for msg in ast.regex_parse_errors.values() {
            eprintln!("parse error: regex literal {msg}");
        }
        return Err(());
    }
    Ok(())
}

pub(crate) fn run_ast_desugar_pipeline(ast: &mut ast::Ast) {
    // RFC 20260807-global-object G1 — `globalThis.<builtin>` member
    // reads rewrite to the bare name before anything consumes member
    // shapes; eval-inlined stmts (prelude) are already in place.
    ast::desugar_globalthis_members(ast);
    ast::desugar_prototype_call(ast);
    ast::inject_builtin_classes(ast);
    ast::desugar_classes(ast);
    ast::desugar_dflt_param_tdz(ast);
    ast::materialize_expr_defaults(ast);
    ast::bind_this_param(ast);
    ast::rewrite_toplevel_this(ast);
    ast::normalize_function_bind_call(ast);
    ast::promote_bind_receiver_this(ast);
    ast::synthesize_fn_constructors(ast);
    ast::route_non_class_new(ast);
    ast::fill_optional_fields(ast);
    ast::synthesize_class_globals(ast);
    ast::tag_struct_field_closure_types(ast);
    ast::desugar_capturing_nested_fns(ast);
    // Once BEFORE the arrows lift and once after (below). The rename a
    // lift performs has to reach the arrows that call the nested
    // function, and they are only still inside the parent's body at
    // this point; the later call is what catches a nested function
    // declared inside an arrow, which is not top-level until
    // `lift_arrow_fns` has made it one. The pass is a fixpoint and
    // idempotent, so running it twice costs a walk that lifts nothing.
    ast::desugar_nested_fns(ast);
    ast::lift_arrow_fns(ast);
    ast::register_bind_receiver_recv_fns(ast);
    ast::infer_anonymous_closure_params(ast);
    ast_closure_param_tag::tag_closure_arg_params(ast);
    ast::synthesize_forwarders(ast);
    ast::desugar_nested_fns(ast);
    ast::synthesize_recv_cb_forwarders(ast);
    ast::synthesize_fn_to_closure_forwarders(ast);
    ast::desugar_function_prototype_methods(ast);
    ast::desugar_uninit_let(ast);
    ast::desugar_var_hoist(ast);
    ast::desugar_variadic_push(ast);
    ast::desugar_arguments_object(ast);
    ast::rewrite_split_for_i_to_iter(ast);
    ast::escape_analyze_array_literals(ast);
    ast::desugar_implicit_generics(ast);
    ast::apply_default_args(ast);
    ast::apply_rest_args(ast);
    ast::apply_spread_args(ast);
    ast::fold_fromentries(ast);
}
