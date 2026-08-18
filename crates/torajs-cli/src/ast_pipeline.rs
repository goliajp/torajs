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
/// Goal-triage gates that must fire BEFORE `modules::resolve_imports`:
/// a strict-goal SyntaxError must precede any resolver diagnostics —
/// `import('./missing.js', yield)` expects the parse-phase reject, not
/// "import path not found" (the same ordering the parse-time yield
/// gate used to guarantee before the goal bit moved post-parse).
/// The delete triage has no resolver-facing face and stays in the
/// prelude with the other raw-AST gates.
pub(crate) fn run_pre_resolve_gates(ast: &mut ast::Ast) -> Result<(), ()> {
    // `yield`-as-identifier goal triage (§12.7.2) — the parser
    // admitted the sites, strict raises the SyntaxError here, sloppy
    // keeps the identifiers as parsed.
    if let Some(msg) = ast::triage_yield_idents(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // §12.7.2 strict-only future reserved words, goal half — same
    // reason it sits with the yield gate rather than in the prelude.
    if let Some(msg) = ast::triage_strict_reserved_idents(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // §16.2.3 duplicate ExportedNames — same reason it sits here: the
    // duplicate-export cases point their `from` clause at the file
    // itself, so "import path not found" / "import error" would
    // otherwise answer before the SyntaxError does.
    if let Some(msg) = ast::triage_duplicate_exports(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    Ok(())
}

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
    // RFC 20260814 — `with` right after the eval inline, and before
    // everything else: the parser leaves a marker Block that no other
    // pass knows, and this turns it into ordinary code before anything
    // else looks.
    //
    // AFTER `desugar_eval`, not before. A `with` can arrive from the
    // eval'd source itself (`Function("var o = {}; with (o) {}")`),
    // and running first meant the marker block was minted after this
    // pass had already decided the program had no `with` in it — so
    // the helpers were never injected and the program died on
    // `unknown identifier __torajs_with_obj` (t262
    // statements/with/12.10.1-5-s and -10-s). Running after also makes
    // eval-inlined statements INSIDE a `with` body get rewritten,
    // which is what §14.11 wants of them anyway.
    // The reject carries its own prefix: a `with` in strict code is a
    // §14.11.1 SyntaxError (`parse error:`), an uncovered shape is
    // `not yet supported:`. Printing both as a bare `error:` made every
    // refusal read to a stderr-classifying harness like an uncaught
    // runtime throw — so a program tr DECLINED counted as one tr ran.
    if let Some(reject) = ast::desugar_with(ast) {
        eprintln!("{}", reject.message());
        return Err(());
    }
    // §14.11 — an object literal used as a `with` scope object is
    // dynamic by contract (per-reference HasBinding, body-side
    // `delete`, fall-through misses); widen its declaration to `any`
    // so it lowers through the dynobj lane (doc on the pass).
    ast::widen_with_object_bindings(ast);
    // `delete <bare name>` goal triage (rotation 372) — strict is the
    // §13.5.1.1 SyntaxError, sloppy resolves §13.5.1.2 statically.
    //
    // AFTER the eval inline and the `with` desugar, not before. It
    // folds each site to a constant from what the program declares,
    // and both of those passes change what a site MEANS:
    //   - a `delete` inlined out of an eval never reached the triage
    //     at all and died downstream on "delete target must be a
    //     property reference";
    //   - inside a `with` body the reference resolves through the
    //     object, so `with (o) { delete x }` has to delete `o.x` —
    //     folding it first answered `true` and removed nothing.
    // What reaches it now is only the fall-through arm of the `with`
    // rewrite plus every ordinary site, which is exactly the set
    // §13.5.1.2 decides statically.
    if let Some(msg) = ast::triage_delete_bare_names(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
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

/// `Err(())` = a gate inside the chain refused the program and has
/// already printed its diagnostic (same contract as the prelude).
pub(crate) fn run_ast_desugar_pipeline(ast: &mut ast::Ast) -> Result<(), ()> {
    // T-12 tagged templates — renumber the parser's `-1` site
    // placeholders program-wide (arena order, deterministic); must
    // run after modules splice so ids are unique across files.
    ast::number_template_sites(ast);
    // RFC 20260807-global-object G1 — `globalThis.<builtin>` member
    // reads rewrite to the bare name before anything consumes member
    // shapes; eval-inlined stmts (prelude) are already in place.
    ast::desugar_globalthis_members(ast);
    // Namespace-alias member rewrite — right after G1 (whose rewrite
    // can mint the `const m = Math` alias shape from
    // `const m = globalThis.Math`), before anything consumes member
    // shapes.
    ast::desugar_ns_alias_members(ast);
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
    ast::alias_arrow_arguments(ast);
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
    // After it, because the receiver-promoting knives live inside it —
    // what still carries a `__this` capture by now is a fn-expr nobody
    // supplies a receiver for, and gets the plain-call answer.
    ast::bind_fnexpr_this_default(ast);
    // C1 honest-reject gate — right after the LAST receiver rule: a
    // `__this` capture still standing is a fn-expr `this` nobody
    // claims, and the checker's spelling of the same reject leaks two
    // internal names (`closure __closure_N references unknown
    // identifier __this`). Same accept/reject line, honest words —
    // see `fnexpr_this_unclaimed`'s module doc.
    if let Some(msg) = ast::unclaimed_fnexpr_this(ast) {
        eprintln!("not yet supported: {msg}");
        return Err(());
    }
    ast::apply_default_args(ast);
    // After the arguments/default passes (their side-tables gate the
    // wrap), before the static expanders (whose declined spread
    // shapes it takes) — see the pass's module doc.
    ast::wrap_dynamic_spread_callees(ast);
    ast::apply_rest_args(ast);
    ast::apply_spread_args(ast);
    ast::fold_fromentries(ast);
    // RFC 20260817-fnsig-reabstraction-thunk — last: it rewrites
    // call-ARGUMENT positions only (the forwarder axes own the
    // store sites), reads the targets' post-materialize param
    // spellings, and its thunk bodies pass every argument
    // explicitly, so nothing after it needs to run again.
    ast::synthesize_sig_thunks(ast);
    Ok(())
}
