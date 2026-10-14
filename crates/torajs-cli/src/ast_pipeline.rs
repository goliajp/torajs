//! The post-parse AST desugar sequence shared by `tr run` (main.rs)
//! and `tr build` (cmd_build.rs), kept in one place so the two do not
//! drift (the REPL and LSP keep their own reduced pipelines).
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
//! - `promote_this_fn_values` — before `tag_closure_arg_params`, so
//!   the parameter a rewritten argument lands in is retagged by the
//!   same fixpoint run; it turns the Ident into `Expr::Closure`,
//!   which the plain fn-to-closure wrap then skips.

use torajs_core::{ast, ast_closure_param_tag, ast_closure_param_tag_ns_values};

mod pre_resolve;
pub(crate) use pre_resolve::run_pre_resolve_gates;

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
    // A top-level `const` written from inside a function: the
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
    // Module-namespace member direct-connect — a static
    // `ns.<export>` read goes straight at the top-level declaration
    // the resolver injected, leaving the synthetic namespace object
    // to the value uses §10.4.6 is about. Same neighbourhood as the
    // alias rewrite above and for the same reason: before anything
    // consumes member shapes.
    ast::desugar_module_ns_members(ast);
    // §13.3.9 — an optional chain that ENDS IN A CALL is an ordinary
    // member call under a nullish guard. The callee flattens to
    // `Member` here, before any pass or the checker reads a callee
    // shape; the guard goes back on at the checker (result widening)
    // and in lowering, keyed off `Ast::optchain_calls`.
    ast::desugar_optchain_calls(ast);
    ast::desugar_prototype_call(ast);
    ast::inject_builtin_classes(ast);
    // Before `desugar_classes`, which binds every `Expr::New` to the
    // `__new_<C>` factory of the class with that name — a lookup with
    // no notion of scope. A `function N` declared in a function body
    // shadows a class `N` for the `new`s inside it, but the rename
    // that says so is `desugar_nested_fns`, twenty passes later. This
    // records the shadowed sites while the scopes are still readable.
    ast::mark_fn_shadowed_new(ast);
    // §15.7.14 step 3 — a named class expression's body refers to the
    // class by a name only that body can see. It has to be resolved
    // before `desugar_classes`, which would otherwise bind it to a
    // same-named class declared outside.
    ast::rewrite_class_expr_self_names(ast);
    ast::desugar_classes(ast);
    // K2b — which member bodies reach a private element, asked while
    // the arrows are still inline in them (`lift_arrow_fns` below
    // hoists every one out).
    ast::collect_private_touch_fns(ast);
    // Object-literal method [[HomeObject]] super sites — right after
    // the class pass has consumed the markers it owns.
    ast::desugar_objlit_super(ast);
    ast::desugar_dflt_param_tdz(ast);
    ast::materialize_expr_defaults(ast);
    ast::bind_this_param(ast);
    ast::bind_new_target_param(ast);
    ast::rewrite_toplevel_this(ast);
    ast::normalize_function_bind_call(ast);
    ast::promote_bind_receiver_this(ast);
    ast::synthesize_fn_constructors(ast);
    ast::route_non_class_new(ast);
    ast::synthesize_class_globals(ast);
    ast::tag_struct_field_closure_types(ast);
    // RFC-less blade — a `function` whose own name is assigned needs a
    // slot to be assigned into. Right before the capturing-nested-fn
    // pass, because it hands its rewrite to the same lane: both mint a
    // function expression that `lift_arrow_fns` then gives an env.
    ast::widen_rebound_fn_decls(ast);
    ast::desugar_capturing_nested_fns(ast);
    // Once BEFORE the arrows lift and once after (below). The rename a
    // lift performs has to reach the arrows that call the nested
    // function, and they are only still inside the parent's body at
    // this point; the later call is what catches a nested function
    // declared inside an arrow, which is not top-level until
    // `lift_arrow_fns` has made it one. The pass is a fixpoint and
    // idempotent, so running it twice costs a walk that lifts nothing.
    ast::desugar_nested_fns(ast);
    // Again, because that pass mints writes of its own: Annex B §B.3.3
    // gives a block-nested `function f` a var-scoped binding written
    // where the declaration sits, and when the scope already declares
    // `function f` at its top that write needs the same slot this pass
    // exists to give. The first call above cannot see those writes —
    // they do not exist until the lift runs.
    ast::widen_rebound_fn_decls(ast);
    ast::alias_arrow_arguments(ast);
    ast::lift_arrow_fns(ast);
    ast::register_bind_receiver_recv_fns(ast);
    ast::infer_anonymous_closure_params(ast);
    // BEFORE the closure-arg fixpoint, not after: giving a promoted
    // declaration's value uses their forwarder turns them into
    // `Expr::Closure`, and the receiving parameters have to be retagged
    // to the env-first repr in the same run — a slot left typed
    // `(x: any) => string` while the argument became a closure cell is
    // the calling-convention mismatch that prints nothing at all.
    // Every value site takes the receiver-first shell, so there is one
    // shell per declaration and no second minting pass below.
    ast::canonicalize_fn_proto_receivers(ast);
    ast::promote_this_fn_values(ast);
    ast::mint_script_global_receiver_shells(ast);
    ast::synthesize_new_target_ctor_faces(ast);
    ast_closure_param_tag::tag_closure_arg_params(ast);
    ast_closure_param_tag_ns_values::wrap_ns_static_value_sites(ast);
    ast::synthesize_forwarders(ast);
    ast::desugar_nested_fns(ast);
    ast::synthesize_fn_to_closure_forwarders(ast);
    ast::wrap_escaping_fn_ident_bindings(ast);
    ast::desugar_function_prototype_methods(ast);
    ast::desugar_uninit_let(ast);
    ast::desugar_var_hoist(ast);
    // Before it — the arguments-object collectors key on arena
    // `Ident`s naming a fn, and this pass removes exactly such a name,
    // so they should read the shape it leaves. Before the static
    // expanders too (it stands aside wherever one of them will take
    // the site), and after `materialize_expr_defaults`, whose output
    // its default gate reads.
    ast::demote_dynamic_spread_method_calls(ast);
    // Before the arguments rewrite: a callback that reads
    // `arguments` can reach its source through it, and the scan asks
    // that of the body as the program wrote it.
    ast::collect_hof_len_shrinks(ast);
    ast::desugar_arguments_object(ast);
    ast::collect_argv_slots(ast);
    // RFC 20260912-global-var-cell knife 3 — a script's top-level functions reach the
    // global object. After the argv injection (see the pass doc), before
    // `desugar_implicit_generics` (its forwarders call generic targets).
    ast::synthesize_script_global_fns(ast);
    ast::rewrite_split_for_i_to_iter(ast);
    ast::escape_analyze_array_literals(ast);
    ast::desugar_implicit_generics(ast);
    // Right after it: that pass is where an unannotated body's return
    // type is decided, so it is the first moment a vtable slot's rows
    // can be seen to disagree — and nothing later rewrites a `__cm_`
    // return annotation.
    ast::join_vtable_slot_returns(ast);
    // The same join at the other end of the signature. It has to run
    // after the slot lanes learned to pad an omitted argument: the
    // rows it pads are entered with that pad, and without it the
    // widened slot answers wrongly instead of refusing loudly.
    // Its refusal is the program's: the rows of a slot that mix a rest
    // parameter with different fixed arities cannot share one, and the
    // ABI check downstream compares MACHINE shapes — a defaulted
    // scalar and a rest array are both one word, so it passes them and
    // the second row reads a number where it expects an array.
    if let Some(msg) = ast::join_vtable_slot_params(ast) {
        eprintln!("not yet supported: {msg}");
        return Err(());
    }
    // After both joins: a witness row is matched against the method
    // signatures they settle.
    torajs_core::iface_witness::synthesize_iface_witnesses(ast);
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
    // Last, for the same reason `escape_analyze_array_literals` runs
    // late: it verifies the final shape of every use of a binding, so
    // every rewrite that could introduce one has to have happened.
    ast::analyze_regex_result_props(ast);
    ast::analyze_let_owned_elems(ast);
    ast::analyze_split_length_only(ast);
    ast::analyze_trim_length_only(ast);
    ast::analyze_regex_match_len_only(ast);
    ast::analyze_regex_exec_record(ast);
    ast::analyze_regex_replace_inline(ast);
    // Truly last: it reads TOP-LEVEL STATEMENT POSITION, so it has to
    // see the order the lowerer will walk. Every pass above may append
    // or lift a declaration, and a mark taken before one of them could
    // name an occurrence that later moved.
    ast::mark_tdz_reads(ast);
    Ok(())
}

#[cfg(test)]
mod tests;
