//! The shared post-parse AST desugar sequence — `tr run` (main.rs)
//! and `tr build` (cmd_build.rs) ran byte-identical copies of this
//! 31-pass chain; one home keeps the two from drifting when a pass
//! lands (the REPL and LSP keep their own reduced pipelines).
//!
//! Ordering notes live where they bind:
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

pub(crate) fn run_ast_desugar_pipeline(ast: &mut ast::Ast) {
    ast::desugar_prototype_call(ast);
    ast::inject_builtin_classes(ast);
    ast::desugar_classes(ast);
    ast::desugar_dflt_param_tdz(ast);
    ast::materialize_expr_defaults(ast);
    ast::bind_this_param(ast);
    ast::rewrite_toplevel_this(ast);
    ast::synthesize_fn_constructors(ast);
    ast::route_non_class_new(ast);
    ast::fill_optional_fields(ast);
    ast::synthesize_class_globals(ast);
    ast::tag_struct_field_closure_types(ast);
    ast::desugar_capturing_nested_fns(ast);
    ast::lift_arrow_fns(ast);
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
