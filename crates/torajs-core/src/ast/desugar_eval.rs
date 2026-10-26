//! `eval` of a string literal, resolved at compile time — direct calls
//! inlined at the call site, indirect calls attached to the global
//! scope.
//!
//! ## Why this is a desugar and not a runtime
//!
//! The reflex is that `eval` means shipping an interpreter, and that an
//! AOT compiler therefore cannot have it. Measurement says otherwise:
//! across the 1473 test262 cases tr rejects on the `eval` identifier,
//! **79.5% pass a string or template literal** — the program eval is
//! asked to run is already sitting in the source being compiled, it
//! merely happens to be written between quotes. For those, parsing the
//! text at compile time and lowering it at the call site is not a trick;
//! it is the same path every other statement in the file takes.
//!
//! ## The environment split, and why tr sits on the strict side of it
//!
//! §19.2.1.1 PerformEval splits the environment in two, and which half
//! `var` lands in depends on strictness:
//!
//! - **sloppy** direct eval shares the caller's *VariableEnvironment*,
//!   so `var` and function declarations leak into the enclosing
//!   function and outlive the eval;
//! - **strict** direct eval gets its own VariableEnvironment too, so
//!   nothing it declares escapes.
//!
//! tr compiles a strict TS subset, and bun — the reference — treats a
//! `.ts` file as an ES module, which is strict. Measured, not assumed:
//! `function f() { eval("var a = 1"); console.log(a) }` raises
//! `ReferenceError: a is not defined` under bun. An earlier draft of
//! this pass inlined a bare `Stmt::Block`, which let `var` through via
//! `desugar_var_hoist` and printed `1` — correct for sloppy mode,
//! wrong for the mode tr is in.
//!
//! So a DIRECT eval's inlined statements are *sealed*: every `var` in
//! eval's own variable scope is re-tagged block-scoped, which makes the
//! enclosing `Stmt::Block` the whole environment for `var`. Sealing
//! stops at a nested `FnDecl` body: that body is its own
//! VariableEnvironment and its `var`s belong to it, not to the eval.
//!
//! ## Indirect eval — `(0, eval)("…")` — and the script framing
//!
//! An INDIRECT call evaluates its source as *global* code
//! (§19.2.1.1: no calling-context environment is passed), and the
//! source's strictness is its own: without a `"use strict"` prologue
//! the eval code is sloppy and its `var`s land on the global —
//! measured against bun, `(0, eval)("var q = 7")` makes `typeof q`
//! answer `"number"` afterwards while `let` stays contained.
//!
//! tr has no separate global object; its top level IS its global. This
//! pass therefore treats the program under **script framing** — the
//! framing test262 itself is written in, where top-level `var`s are
//! global and indirect eval shares one scope with the top level. One
//! direction deliberately rides on this framing and diverges from
//! bun-as-module: a top-level-position source that reads the caller's
//! top-level bindings collapses (script semantics — the global env
//! holds them), while bun's module bindings are invisible to indirect
//! eval and it would throw. That is the spec's answer for the code
//! test262 actually writes; conformance fixtures stay on convergent
//! shapes (names touched only through eval reach the same binding
//! both ways).
//!
//! Concretely, an indirect literal eval is resolved when one of these
//! holds, and left alone (honest reject) otherwise:
//!
//! - **the source is closed** — a single expression with no free
//!   variables. Scope cannot matter, so it collapses to the expression
//!   in ANY position, including inside functions (`assert.throws(…,
//!   function () { (0, eval)("…") })` is how test262 wraps most sites);
//! - **the source is a single expression at a top-level position** —
//!   free variables allowed, provided none collides with a lexical
//!   name declared below the top level (`walk::nested_lexical_names` —
//!   such a name would shadow the collapse site while real global code
//!   cannot see it);
//! - **the source completes empty without effects** — `";"`, `"{}"`,
//!   control flow over literals — and collapses to `undefined`;
//! - **the call is a top-level statement** — inlined as a `Stmt::Block`
//!   WITHOUT sealing: `var` hoists out (sloppy global semantics),
//!   `let` / `const` / `class` stay in the block (their environment is
//!   the eval's own lexical env, distinct per §19.2.1.2);
//! - **the source does not parse** — replaced by a `throw new
//!   SyntaxError` in statement position, at any depth.
//!
//! A statement-position indirect eval INSIDE a function is not inlined:
//! its source belongs to the global scope, and inlining it locally
//! would let the source see the function's bindings.
//!
//! **A function declaration still escapes a direct eval — a defect
//! this pass inherits rather than introduces.** A plain `{ function
//! f() {} }` with no eval anywhere leaks `f` the same way (§14.2 makes
//! it block-scoped; Annex B B.3.3 hoists it only in sloppy mode, and
//! tr is not in sloppy mode), so `var` here follows the strict rule
//! while `function` follows the sloppy one. AnnexB eval-code cases
//! passing off the back of it are water under no-metric-inflation and
//! fall back out as the family tightens (7 did in r333).
//!
//! This is why the sloppy-only corpus is out of reach here rather than
//! merely unimplemented: 697 of the 1473 eval-blocked cases (47.3%)
//! carry `flags: [noStrict]`, and 404 of the 405 in
//! `annexB/language/eval-code` do — that directory tests the Annex B
//! web-compat extension whose own description says it is "not honored
//! in strict mode". A strict runtime cannot satisfy them by
//! implementing more; they need a second language mode.
//!
//! ## What this pass deliberately does NOT do
//!
//! - **Only statically-placeable completion values.** A single
//!   ExpressionStatement collapses to its expression; a multi-statement
//!   source whose FINAL statement is one becomes an IIFE
//!   (`completion.rs`). Shapes whose completion needs the runtime
//!   completion machinery — `eval("if (true) { }")`, `eval("1; ;")` —
//!   keep the honest reject rather than evaluating to something wrong.
//! - **Only literal source.** A runtime string needs the compiler in
//!   the artifact; that is a separate layer with its own cost to
//!   measure (artifact size is a headline property of tr).
//! - **Only the comma spelling of indirect.** Alias forms (`var e =
//!   eval; e("…")`) need value tracking, and `eval` as a first-class
//!   value (`arr.every(cb, eval)`) needs a runtime eval object — both
//!   are the runtime layer's problem, not this pass's.
//! - **Only when the program has not rebound `eval`.** If any binding
//!   named `eval` exists anywhere, the pass declines wholesale — a
//!   local `eval` is not this `eval`, and proving which one a given
//!   call site sees needs scope resolution this pass runs before.
//! - **A parse failure becomes a throw**, per §19.2.1.1 step 12 —
//!   raised when the eval is reached (`if (false) { eval("((("); }`
//!   completes normally): a statement rewrite in statement position,
//!   a throw-IIFE in value position (`completion.rs`).

mod bound_fns;
mod class_field_arguments;
mod collapse;
mod completion;
mod completion_stmt;
mod const_prop;
mod foreign_spans;
mod function_ctor;
mod instantiate;
mod param_default;
mod param_default_arrow;
mod recognize;
mod rewrite;
mod scope;
mod script_fn_position;
mod script_form;
mod source;
mod walk;
// fnexpr-bind promotion shares the fn-ownership walk (shadow-use aware)
pub(crate) use walk::{body_owned_exprs, fn_owned_exprs};

use super::{Ast, Expr, Stmt, free_vars};
use collapse::{completes_empty_effect_free, decl_names, is_effect_free_decl, name_mentioned};
use recognize::{has_use_strict_prologue, literal_eval_call, nonstring_literal_eval_arg};
use rewrite::rewrite_list;
use scope::binds_eval;
use source::{CallForm, EvalStrictness, eval_strictness, parse_eval_source};

/// Resolve every literal `eval` call this pass can resolve exactly.
/// See the module doc for the boundaries. The dynamic-function desugar
/// rides along, gated on ITS name only — a program that rebinds `eval`
/// still gets its `Function("…")` calls resolved, and vice versa.
pub fn desugar_eval(ast: &mut Ast) {
    if !binds_eval(ast) {
        // Named constant sources first — `var s = '…'; eval(s)`
        // becomes a literal call, so every rewrite below sees it.
        const_prop::propagate_eval_const_args(ast);
        // §19.2.1.3 special case BEFORE the value collapse: a direct
        // eval in non-arrow default-parameter position whose source
        // var-declares `arguments` must throw a SyntaxError when the
        // function is called, while the collapse would fold the
        // declarations-only source to `undefined` and swallow it.
        param_default::rewrite_param_default_arguments_evals(ast);
        // The arrow half of the same special case — legal in sloppy
        // code (the binding lands in the arrow's parameter scope), a
        // SyntaxError throw when a parameter is itself named
        // `arguments`. Must also run before the value collapse, for
        // the same swallowed-throw reason.
        param_default_arrow::rewrite_arrow_param_default_arguments_evals(ast);
        // §19.2.1.1 step 12's other half, and before the collapse for
        // the same swallowed-throw reason: a direct eval called from a
        // class field initializer may not bring `arguments` in — the
        // §15.7.1 prohibition follows the text into the eval.
        class_field_arguments::rewrite_class_field_arguments_evals(ast);
        // §16.1.7 steps 3-5 for `$262.evalScript`: a Script whose
        // declarations collide with the bindings in force throws when
        // evaluated — decided on the program as written, before any
        // inlining or collapse could move a declaration.
        script_form::throw_colliding_scripts(ast);
        // Value-position collapses first: a collapsed call is no
        // longer an eval call, so the statement walks below see only
        // the sources that need inlining.
        rewrite_value_position_evals(ast);
        // Before the take: the map is built by walking `ast.stmts`.
        let caller_strict = walk::caller_strict_exprs(ast);
        let mut stmts = std::mem::take(&mut ast.stmts);
        rewrite_list(&mut stmts, ast, false, false, &caller_strict);
        // The eval / Script inlines were spliced flat by `rewrite_list`;
        // under the Script goal the last of several same-named
        // top-level functions is the one instantiated.
        if ast.sloppy_script_goal {
            script_form::keep_last_fn_decls(&mut stmts);
        }
        ast.stmts = stmts;
        rewrite_arrow_bodies(ast);
        completion::rewrite_completion_value_evals(ast);
        bound_fns::route_global_reads(ast);
    }
    function_ctor::rewrite_function_ctors(ast);
}

/// Collapse the eval calls whose value is exact without any scope
/// machinery, wherever they sit in the expression arena.
///
/// For a DIRECT call the caller's scope IS the eval's scope, so a
/// single-expression source collapses unconditionally (§14.5.1: the
/// completion of an ExpressionStatement is its value), and a source
/// that is nothing but effect-free declarations collapses to
/// `undefined` — in a STRICT direct eval the bindings die with the
/// eval's own environment, so nothing can observe them.
///
/// For an INDIRECT call the source belongs to the global scope. A
/// single expression with **no free variables** collapses anywhere —
/// scope cannot touch it. One that DOES read identifiers collapses
/// only when its slot is outside every function body (`walk::
/// fn_owned_exprs`): there the surrounding scope IS the global scope
/// under script framing, so the source resolves its names exactly as
/// spec'd. A source that completes empty without effects collapses to
/// `undefined` anywhere. Indirect declaration sources do NOT collapse:
/// a sloppy eval's `var` lands on the global and is observable
/// afterwards.
///
/// Both forms collapse a non-string literal argument to itself
/// (§19.2.1.1 step 2: eval returns a non-String argument unchanged).
///
/// The replacement writes over the Call node in place rather than
/// re-pointing the parent at a new id, so every existing reference to
/// this ExprId keeps working without a remap. The parsed expression's
/// own node stays in the arena unreferenced, which costs a slot and
/// nothing else.
fn rewrite_value_position_evals(ast: &mut Ast) {
    // Snapshot which slots sit under a function body BEFORE any parse
    // appends to the arena. Appended slots (eval sources) read `false`
    // through the bounds check, which is right — they take the
    // position of the call they replace.
    let fn_owned = walk::fn_owned_exprs(ast);
    let class_owned = walk::class_owned_exprs(ast);
    let caller_strict = walk::caller_strict_exprs(ast);
    let nested_lexical = walk::nested_lexical_names(ast);
    let mut i = 0;
    while i < ast.exprs.len() {
        let eid = super::ExprId(i as u32);
        if let Some(lit) = nonstring_literal_eval_arg(eid, ast) {
            ast.exprs[i] = lit;
            i += 1;
            continue;
        }
        let Some((src, form)) = literal_eval_call(eid, ast) else {
            i += 1;
            continue;
        };
        // §19.2.1.1 steps 4-6 — SuperProperty in the text is legal only
        // for a DIRECT eval whose call site has a [[HomeObject]] on its
        // this-environment. Anywhere else the parse fails (the parser's
        // own position gate), the collapse is skipped, and the throw
        // carriers downstream raise the step-12 SyntaxError.
        let super_ok = form == CallForm::Direct && class_owned.get(i).copied().unwrap_or(false);
        let arena_before = ast.exprs.len();
        let strict = eval_strictness(form, i, &caller_strict);
        if let Ok(parsed) = parse_eval_source(&src, ast, super_ok, strict) {
            if let [Stmt::Expr(inner)] = parsed[..] {
                let at_toplevel = !fn_owned.get(i).copied().unwrap_or(false);
                let closed = form == CallForm::Direct || {
                    let fv = free_vars::free_vars_of_body(ast, &[], &parsed);
                    fv.is_empty() || (at_toplevel && fv.iter().all(|n| !nested_lexical.contains(n)))
                };
                if closed && let Some(e) = ast.exprs.get(inner.0 as usize).cloned() {
                    ast.exprs[i] = e;
                    // the copy is the async function value the parse
                    // registered under `inner`
                    if ast.async_fn_value_exprs.contains(&inner) {
                        ast.async_fn_value_exprs.insert(eid);
                    }
                    // Do not advance: the expression just written
                    // in may itself be an `eval("…")` that came out
                    // of a nested literal, and it now occupies this
                    // slot.
                    continue;
                }
            }
            // A "use strict" prologue makes the eval code strict
            // regardless of call form (§19.2.1.1 steps 3-5): its
            // declarations die with the eval's own environment, and
            // the directive — an ordinary expression statement — is
            // the completion value when nothing after it produces one.
            let prologue = has_use_strict_prologue(&parsed, ast);
            let tail = if prologue { &parsed[1..] } else { &parsed[..] };
            let strict_ctx = strict == EvalStrictness::Strict || prologue;
            // Declarations complete with *empty* and, in a strict
            // eval, bind nothing anyone can see — so a tail of
            // effect-free declarations and empty statements leaves
            // only the prologue's value (or undefined without one).
            let tail_dead = strict_ctx
                && !tail.is_empty()
                && tail
                    .iter()
                    .all(|s| is_effect_free_decl(s, ast) || completes_empty_effect_free(s, ast));
            let tail_empty = tail.iter().all(|s| completes_empty_effect_free(s, ast));
            // A SLOPPY indirect declaration source does put its names
            // on the global — but a binding the rest of the program
            // never so much as names (no identifier, no member/string
            // spelling that could reach it through `this`) is
            // unobservable, and the call collapses to the empty
            // completion. `mentions` scans only the arena as it was
            // before this parse appended the source's own expressions.
            let tail_dead_sloppy = !strict_ctx
                && !tail.is_empty()
                && tail
                    .iter()
                    .all(|s| is_effect_free_decl(s, ast) || completes_empty_effect_free(s, ast))
                && {
                    let mut names = Vec::new();
                    decl_names(tail, &mut names);
                    !names.is_empty()
                        && names
                            .iter()
                            .all(|n| !name_mentioned(ast, n, arena_before, eid))
                };
            if tail_dead || tail_empty || tail_dead_sloppy {
                ast.exprs[i] = if prologue {
                    Expr::String("use strict".to_string().into())
                } else {
                    Expr::Ident("undefined".to_string())
                };
            }
        }
        i += 1;
    }
}

/// The statement walk above reaches a `FnDecl` body because that body
/// hangs off the statement tree. A function written in *expression*
/// position does not: `() => { … }`, `function () { … }` and a method
/// shorthand all park their statements inside an `Expr::ArrowFn` in the
/// expression arena, which no statement walk visits. Missing them is
/// not a corner: `closure __closure_N references unknown identifier
/// eval` accounts for ~390 of the eval-blocked cases on its own, and
/// `assert.throws(…, function () { eval("…") })` is how test262 spells
/// most of its eval assertions.
///
/// The loop re-reads `len()` each turn rather than snapshotting it,
/// because inlining appends to the arena — an arrow written inside an
/// eval'd literal lands past the original end and still has to be
/// visited. Each body is taken out before the rewrite so `ast` can be
/// handed to `parse_into`, then put back. An arrow body is a function
/// scope, so the walk enters it with `in_fn` set.
fn rewrite_arrow_bodies(ast: &mut Ast) {
    // Home verdict per arrow (r334 blade 6): an arrow inherits its
    // enclosure's this-environment, so a direct eval inside one may
    // contain `super` exactly when the arrow itself sits in a class
    // member body. Arrows appended by inlining read `false` through
    // the bounds check — conservative (their eval supers decline to a
    // runtime SyntaxError rather than resolving).
    let class_owned = walk::class_owned_exprs(ast);
    let caller_strict = walk::caller_strict_exprs(ast);
    let mut i = 0;
    while i < ast.exprs.len() {
        let Some(Expr::ArrowFn { body, .. }) = ast.exprs.get_mut(i) else {
            i += 1;
            continue;
        };
        let mut taken = std::mem::take(body);
        let home = class_owned.get(i).copied().unwrap_or(false);
        rewrite_list(&mut taken, ast, true, home, &caller_strict);
        if let Some(Expr::ArrowFn { body, .. }) = ast.exprs.get_mut(i) {
            *body = taken;
        }
        i += 1;
    }
}
