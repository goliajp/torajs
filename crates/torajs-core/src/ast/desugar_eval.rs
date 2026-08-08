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
//! (Decomposition: `.claude/rfcs/20260807-eval/decomposition.md`.)
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
//! source's strictness is its own — it does not inherit the caller's,
//! so without a `"use strict"` prologue the eval code is sloppy and its
//! `var`s land on the global. Measured against bun: `(0,
//! eval)("var q = 7")` makes `typeof q` answer `"number"` afterwards,
//! `let` stays contained, and the completion value comes back exactly
//! as for direct.
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
//! **A function declaration still escapes a direct eval, and that is a
//! defect this pass inherits rather than introduces.** `eval("{
//! function f() {} }")` leaves `f` visible afterwards under tr and does
//! not under bun — but so does a plain `{ function f() {} }` with no
//! eval anywhere, which is where the difference actually lives (§14.2
//! says a block-level function declaration is block-scoped; Annex B
//! B.3.3 hoists it only in sloppy mode, and tr is not in sloppy mode).
//! Until that is fixed, `var` here follows the strict rule and
//! `function` follows the sloppy one, which is not a coherent mode.
//! Twenty-six `annexB/language/eval-code` cases currently report `pass`
//! off the back of it — they are asking for the sloppy behaviour and
//! getting it by accident, so they are counted as water under the
//! no-metric-inflation rule and will fall back out when the block
//! scoping is corrected.
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
//! - **A parse failure in statement position becomes a throw**, per
//!   §19.2.1.1 step 12 — raised when the eval is reached, so
//!   `if (false) { eval("((("); }` still runs to completion. In value
//!   position it is still left alone: JavaScript has no throw
//!   expression, so that shape needs a statement-level rewrite this
//!   pass does not do yet.

mod completion;
mod scope;
mod source;
mod walk;
// The fnexpr-bind promotion shares the fn-ownership walk (its use
// profile must not count a harness param's shadow uses).
pub(super) use walk::{body_owned_exprs, fn_owned_exprs};

use super::{Ast, Expr, Stmt, free_vars};
use scope::{binds_eval, seal_var_scope};
use source::{
    CallForm, first_line, has_use_strict_prologue, literal_eval_call, nonstring_literal_eval_arg,
    parse_eval_source, syntax_error_throw,
};

/// Resolve every literal `eval` call this pass can resolve exactly.
/// See the module doc for the boundaries.
pub fn desugar_eval(ast: &mut Ast) {
    if binds_eval(ast) {
        return;
    }
    // Value-position collapses first: a collapsed call is no longer an
    // eval call, so the statement walks below see only the sources that
    // need inlining.
    rewrite_value_position_evals(ast);
    let mut stmts = std::mem::take(&mut ast.stmts);
    rewrite_list(&mut stmts, ast, false);
    ast.stmts = stmts;
    rewrite_arrow_bodies(ast);
    completion::rewrite_completion_value_evals(ast);
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
        let arena_before = ast.exprs.len();
        if let Some(parsed) = parse_eval_source(&src, ast) {
            if let [Stmt::Expr(inner)] = parsed[..] {
                let at_toplevel = !fn_owned.get(i).copied().unwrap_or(false);
                let closed = form == CallForm::Direct || {
                    let fv = free_vars::free_vars_of_body(ast, &[], &parsed);
                    fv.is_empty() || (at_toplevel && fv.iter().all(|n| !nested_lexical.contains(n)))
                };
                if closed {
                    if let Some(e) = ast.exprs.get(inner.0 as usize).cloned() {
                        ast.exprs[i] = e;
                        // Do not advance: the expression just written
                        // in may itself be an `eval("…")` that came out
                        // of a nested literal, and it now occupies this
                        // slot.
                        continue;
                    }
                }
            }
            // A "use strict" prologue makes the eval code strict
            // regardless of call form (§19.2.1.1 steps 3-5): its
            // declarations die with the eval's own environment, and
            // the directive — an ordinary expression statement — is
            // the completion value when nothing after it produces one.
            let prologue = has_use_strict_prologue(&parsed, ast);
            let tail = if prologue { &parsed[1..] } else { &parsed[..] };
            let strict_ctx = form == CallForm::Direct || prologue;
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
                    !names.is_empty() && names.iter().all(|n| !name_mentioned(ast, n, arena_before))
                };
            if tail_dead || tail_empty || tail_dead_sloppy {
                ast.exprs[i] = if prologue {
                    Expr::String("use strict".to_string())
                } else {
                    Expr::Ident("undefined".to_string())
                };
            }
        }
        i += 1;
    }
}

/// A declaration whose evaluation nothing can observe **in a strict
/// direct eval**: a `let` / `var` whose initializer is an identifier
/// read or a literal, or a function declaration. `Stmt::Multi` of such
/// declarations counts too — the parser expands `var x, y` into one.
/// The initializer bound is what keeps the collapse exact; a call, a
/// member access, anything that could run code disqualifies the whole
/// source.
fn is_effect_free_decl(s: &Stmt, ast: &Ast) -> bool {
    match s {
        // `Uninit` is the parser's sentinel for `var x;` with no
        // initializer at all — the most effect-free init there is.
        Stmt::LetDecl { init, .. } => matches!(
            ast.exprs.get(init.0 as usize),
            Some(Expr::Uninit | Expr::Ident(_) | Expr::String(_) | Expr::Number(_))
        ),
        Stmt::FnDecl { .. } => true,
        Stmt::Multi(list) => list.iter().all(|s| is_effect_free_decl(s, ast)),
        _ => false,
    }
}

/// A statement whose completion is *empty*, whose evaluation has no
/// observable effect in ANY scope, and which provably terminates:
/// empty blocks, control flow whose paths are decided by literal
/// conditions and reach only empty-and-effect-free statements. `(0,
/// eval)("{}")` and `(0, eval)("for(false;false;false);")` are
/// `undefined` by §14.5.1, and test262's cptn-nrml-empty family spells
/// exactly these.
///
/// Three boundaries are load-bearing:
/// - a literal EXPRESSION statement is value-producing (`eval("1;
///   2;")` is `2`, not undefined), so `Stmt::Expr` never qualifies;
/// - a loop qualifies only when its literal condition is FALSY — a
///   truthy `while (true);` never terminates, and collapsing it to
///   `undefined` would change that;
/// - an `if` over a truthy literal completes with its taken branch's
///   completion, so the branch actually reached must itself qualify.
fn completes_empty_effect_free(s: &Stmt, ast: &Ast) -> bool {
    match s {
        Stmt::Block(b) | Stmt::Multi(b) => b.iter().all(|s| completes_empty_effect_free(s, ast)),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => match lit_truth(*cond, ast) {
            Some(true) => completes_empty_effect_free(then_branch, ast),
            Some(false) => else_branch
                .as_deref()
                .is_none_or(|e| completes_empty_effect_free(e, ast)),
            None => false,
        },
        Stmt::While { cond, .. } => lit_truth(*cond, ast) == Some(false),
        Stmt::DoWhile { body, cond } => {
            lit_truth(*cond, ast) == Some(false) && completes_empty_effect_free(body, ast)
        }
        Stmt::For {
            init, cond, body, ..
        } => {
            // The step never runs under a falsy condition; a missing
            // condition is an infinite loop and disqualifies. The
            // init is NOT a completion position, so a literal
            // expression statement there is fine — its value is
            // discarded, only its effects (none) matter.
            init.as_deref().is_none_or(|i| match i {
                Stmt::Expr(e) => lit_truth(*e, ast).is_some(),
                other => completes_empty_effect_free(other, ast),
            }) && cond.is_some_and(|c| lit_truth(c, ast) == Some(false))
                && completes_empty_effect_free(body, ast)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            // Only the trivially-empty switch: any case body might be
            // the one dispatched to, and a value-producing body would
            // become the completion.
            lit_truth(*scrutinee, ast).is_some() && cases.is_empty() && default.is_none()
        }
        _ => false,
    }
}

/// The boolean a literal coerces to, or `None` for anything that is
/// not a literal. Mirrors §7.1.2 ToBoolean over the literal shapes the
/// parser can produce directly.
fn lit_truth(e: super::ExprId, ast: &Ast) -> Option<bool> {
    match ast.exprs.get(e.0 as usize)? {
        Expr::Bool(b) => Some(*b),
        Expr::Number(n) => Some(*n != 0.0 && !n.is_nan()),
        Expr::String(s) => Some(!s.is_empty()),
        Expr::Null => Some(false),
        _ => None,
    }
}

/// The names a declaration source binds — what a sloppy indirect eval
/// would put on the global.
fn decl_names(stmts: &[Stmt], out: &mut Vec<String>) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, .. } => out.push(name.clone()),
            Stmt::FnDecl { name, .. } => out.push(name.clone()),
            Stmt::Multi(list) => decl_names(list, out),
            _ => {}
        }
    }
}

/// Could the program reach a global binding of this name? Any
/// identifier spelling it, any member access naming it (`this.x`), or
/// any string literal equal to it (`'x' in this`,
/// `getOwnPropertyDescriptor(this, "x")`) counts. The scan stops at
/// `limit` — the arena length before the current eval source was
/// parsed in — so the source's own uses of its own names do not veto
/// the collapse. Garbage expressions left by earlier collapses are
/// scanned too, which can only over-decline: the safe direction.
fn name_mentioned(ast: &Ast, name: &str, limit: usize) -> bool {
    ast.exprs[..limit.min(ast.exprs.len())]
        .iter()
        .any(|e| match e {
            Expr::Ident(n) => n == name,
            Expr::Member { name: n, .. } | Expr::OptChain { name: n, .. } => n == name,
            Expr::String(s) => s == name,
            _ => false,
        })
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
    let mut i = 0;
    while i < ast.exprs.len() {
        let Some(Expr::ArrowFn { body, .. }) = ast.exprs.get_mut(i) else {
            i += 1;
            continue;
        };
        let mut taken = std::mem::take(body);
        rewrite_list(&mut taken, ast, true);
        if let Some(Expr::ArrowFn { body, .. }) = ast.exprs.get_mut(i) {
            *body = taken;
        }
        i += 1;
    }
}

fn rewrite_list(stmts: &mut Vec<Stmt>, ast: &mut Ast, in_fn: bool) {
    for s in stmts.iter_mut() {
        rewrite_stmt(s, ast, in_fn);
    }
}

fn rewrite_stmt(s: &mut Stmt, ast: &mut Ast, in_fn: bool) {
    // The rewrite itself: a statement that is nothing but a call to
    // `eval` with one literal argument becomes the block that literal
    // parses to. A direct eval's block is sealed (strict — nothing
    // escapes); an indirect eval's block is not (sloppy global code —
    // `var` hoists out, `let` stays), and is only built at the top
    // level, where the surrounding scope IS the global scope. An
    // indirect statement inside a function is left alone rather than
    // wrongly attached to the function's scope.
    if let Stmt::Expr(eid) = s {
        if let Some((src, form)) = literal_eval_call(*eid, ast) {
            let inline_here = form == CallForm::Direct || !in_fn;
            match parse_eval_source(&src, ast) {
                Some(mut inlined) if inline_here => {
                    // An eval inside the inlined text is an eval like
                    // any other; the nesting is finite because each
                    // level is a literal written in the level above.
                    rewrite_list(&mut inlined, ast, in_fn);
                    // A "use strict" prologue makes even an indirect
                    // eval's code strict — its `var`s die with the
                    // eval (§19.2.1.1 steps 3-5), so it seals exactly
                    // like a direct one.
                    if form == CallForm::Direct || has_use_strict_prologue(&inlined, ast) {
                        seal_var_scope(&mut inlined);
                    }
                    *s = Stmt::Block(inlined);
                    return;
                }
                Some(_) => {}
                None => {
                    // The text does not parse. §19.2.1.1 step 12 wants
                    // a SyntaxError at evaluation time — see
                    // `syntax_error_throw` on why this is a throw and
                    // not a compile error. Scope-independent, so it
                    // applies at any depth for both call forms.
                    *s = syntax_error_throw(format!("eval: {}", first_line(&src)), ast);
                    return;
                }
            }
        }
        // `v = eval("(((")` — an assignment statement whose rhs is an
        // eval of a non-parsing source. §13.15.2 evaluates the target
        // reference first and the rhs second, so with a bare-identifier
        // target (no effects) the statement's entire behaviour is the
        // rhs's SyntaxError; the assignment never completes and the
        // statement becomes the throw. A member target could run a
        // getter on its object expression first, so only the identifier
        // shape rewrites.
        if let Some(Expr::Assign { target, value }) = ast.exprs.get(eid.0 as usize) {
            let (target, value) = (*target, *value);
            if matches!(ast.exprs.get(target.0 as usize), Some(Expr::Ident(_))) {
                if let Some((src, _)) = literal_eval_call(value, ast) {
                    if parse_eval_source(&src, ast).is_none() {
                        *s = syntax_error_throw(format!("eval: {}", first_line(&src)), ast);
                        return;
                    }
                }
            }
        }
    }
    match s {
        Stmt::Block(b) | Stmt::Multi(b) => rewrite_list(b, ast, in_fn),
        Stmt::FnDecl { body, .. } => rewrite_list(body, ast, true),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_stmt(then_branch, ast, in_fn);
            if let Some(e) = else_branch.as_deref_mut() {
                rewrite_stmt(e, ast, in_fn);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => rewrite_stmt(body, ast, in_fn),
        Stmt::Labeled { body, .. } => rewrite_stmt(body, ast, in_fn),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_stmt(i, ast, in_fn);
            }
            rewrite_stmt(body, ast, in_fn);
        }
        Stmt::ForOf { body, .. } => rewrite_stmt(body, ast, in_fn),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_list(body, ast, in_fn);
            rewrite_list(catch_body, ast, in_fn);
            if let Some(f) = finally_body.as_mut() {
                rewrite_list(f, ast, in_fn);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                rewrite_list(&mut c.body, ast, in_fn);
            }
            if let Some(d) = default.as_mut() {
                rewrite_list(d, ast, in_fn);
            }
        }
        _ => {}
    }
}
