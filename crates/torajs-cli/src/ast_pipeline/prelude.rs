//! The passes and raw-AST early-error gates that run between the
//! module resolver and the desugar chain, split from `ast_pipeline`
//! under the file-size limit.

use torajs_core::ast;

pub(crate) fn run_ast_prelude(ast: &mut ast::Ast) -> Result<(), ()> {
    // First: every later pass reads type names through these tables.
    if let Err(msg) = ast::resolve_type_decls(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
    // Block/CaseBlock redeclaration early errors — must see the RAW
    // AST before the generator / async / var-hoist desugars move one
    // side of a conflict away.
    ast::early_redecl_errors(ast);
    // A Script's duplicate `function` declarations: the last one wins
    // (§16.1.7), dropped here so the checker registers one binding.
    ast::dedupe_fn_decls(ast);
    ast::flatten_interface_extends(ast);
    // §14.8.1 / §14.9.1 `break` / `continue` early errors — same RAW-AST
    // reason as the redeclaration gate above: the generator / async /
    // for-of desugars rewrite loops, and a rewritten loop is not the
    // shape the spec asks about. ssa_lower already decided every one of
    // these and said "a syntax error upstream"; this is upstream.
    if let Some(msg) = ast::early_label_errors(ast) {
        eprintln!("parse error: {msg}");
        return Err(());
    }
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
    // AFTER `desugar_eval`, not before: a `with` can arrive from the
    // eval'd source itself (`Function("var o = {}; with (o) {}")`),
    // and its marker block is minted after a pass that ran first had
    // decided there was no `with`. Running after also rewrites
    // eval-inlined statements INSIDE a `with` body, as §14.11 wants.
    // The reject carries its own prefix: a `with` in strict code is a
    // §14.11.1 SyntaxError (`parse error:`), an uncovered shape is
    // `not yet supported:`. Printing both as a bare `error:` made every
    // refusal read to a stderr-classifying harness like an uncaught
    // runtime throw — so a program tr DECLINED counted as one tr ran.
    if let Some(reject) = ast::desugar_with(ast) {
        eprintln!("{}", reject.message());
        return Err(());
    }
    // §10.2.11's parameter environment: a body declaration a default
    // would otherwise reach takes a name of its own, before any pass
    // moves a default (the body's names are final once eval and `with`
    // have been inlined).
    ast::split_param_scope(ast);
    // §14.11 — an object literal used as a `with` scope object is
    // dynamic by contract (per-reference HasBinding, body-side
    // `delete`, fall-through misses); widen its declaration to `any`
    // so it lowers through the dynobj lane (doc on the pass).
    ast::widen_with_object_bindings(ast);
    // A promise binding whose `then` the program stores keeps that
    // store in the cell's expando bag — an any-lane surface, so the
    // binding widens to `any` (doc on the pass).
    ast::widen_promise_then_store_bindings(ast);
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
    // …and the array half of the `delete` family: a binding the program
    // deletes indices out of was never a `number[]`, so its declaration
    // widens to `any[]` — the only element storage that can say
    // no-longer-here. Beside the triage above because it answers the
    // same question about the same operator, and before anything reads
    // element types.
    ast::widen_deleted_array_bindings(ast);
    // Writes to the non-writable global value properties (NaN /
    // Infinity / undefined) — §6.2.5.6 runtime semantics per goal:
    // strict throws TypeError at the site, sloppy folds to the rhs.
    // Same placement rationale as the delete triage above.
    ast::demote_assigned_builtin_globals(ast);
    ast::resolve_readonly_global_writes(ast);
    // …and their member-write mirror: the non-writable
    // builtin-namespace properties (`Number.NaN = 1`), same goal
    // split, same placement.
    ast::resolve_readonly_ns_prop_writes(ast);
    // …and the call-face member of the family: calls to builtin
    // values the spec makes uncallable (`Map()` / `JSON()`) become
    // their §13.3.6.2 runtime TypeError — no goal split, both goals
    // throw. Same placement rationale.
    ast::resolve_uncallable_builtin_calls(ast);
    // A `const` written from inside a nested function: the
    // runtime TypeError §9.1.1.1.5 gives, minted here while every
    // function is still in its own scope (`const_write_throw`).
    ast::rewrite_const_writes_in_functions(ast);
    // Sloppy-goal implicit globals (§9.1.1.4.6) — synthesize a hoisted
    // `var <name>;` per never-declared assignment target, so the write
    // creates the binding instead of the checker rejecting the program.
    // After the readonly sibling (its folds already consumed the §19.1
    // names), before the checker.
    ast::synthesize_sloppy_implicit_globals(ast);
    ast::unwrap_exports(ast);
    ast::rename_user_main(ast);
    ast::census_program_reparents(ast);
    ast::desugar_using(ast);
    // RFC 20260809 B5 — after `desugar_using` (the classes contain no
    // `using`), before `desugar_async` (their walk helpers are async
    // fns the ordinary async desugar state-machines).
    ast::inject_disposable_stack(ast);
    // RFC 20260914 — `Array.fromAsync` self-hosted: an async helper the
    // same state-machine run moves, plus its startup bind.
    ast::inject_array_from_async(ast);
    // RFC 20260915 — `finally` via `then`: the §27.2.5.3 closures as a
    // TypeScript helper the runtime's Invoke lane calls.
    ast::inject_promise_finally(ast);
    // RFC 20260805-async-fn-state-machine blades 1-2: an async function
    // suspends at its first await — the body becomes a generator driven
    // by a promise. Before `hoist_gen_fn_exprs`, which lifts the
    // generator EXPRESSIONS blade 2 mints for async function values,
    // and before `desugar_generators`, which lowers every `function*`.
    // D4's driver is an async declaration too, injected first so this
    // one run moves it (a second run from inside `desugar_generators`
    // moved async values whose nested generators had just been lifted
    // out, past the hoist — `desugar_generators_async`).
    // A `function` declared directly in an async body becomes a function
    // value bound at the body's top, so the moved body declares nothing
    // (a nested declaration cannot see the machine's fields; a value
    // captures them) — `desugar_async_nested_fns`.
    ast::desugar_async_nested_fns(ast);
    ast::inject_async_generator_driver(ast);
    // the async functions whose awaits suspend over an SSA frame; the
    // state machine leaves them alone
    ast::mark_async_ssa_fns(ast);
    ast::desugar_async_state_machine(ast);
    ast::hoist_gen_fn_exprs(ast);
    ast::desugar_generators(ast);
    ast::desugar_async(ast);
    // §8.1.1.1 — a closure that may run before the `let` / `const` it
    // reads has been initialised gets the machine bodies' runtime flag,
    // in every ordinary list. After the machines, whose own bodies
    // `desugar_generators_tdz` has already settled.
    ast::flag_closure_tdz(ast);
    // §10.4.6.8 — the entry's own namespace over bindings declared
    // after it: the reflective faces need the binding's dead zone
    // at run time, off a flag the cell is handed (`ns_tdz_flag`).
    ast::flag_ns_tdz(ast);
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
