//! A `function` declared directly in an ASYNC function's body becomes
//! a function value bound at the top of that body, before the body is
//! moved into its state machine (RFC 20260805 blades 1-2).
//!
//! ```text
//! async function outer() {
//!   let n = 1;
//!   function g() { return n; }   // bun: 2, tr: unknown identifier `g`
//!   await null; n = 2;
//!   console.log(g());
//! }
//! ```
//!
//! Inside a moved body every local is a field of the machine, and a
//! nested declaration reading one has no binding for it: the
//! declaration lane (`nested_fns`) lifts it to top level, where the
//! field is not a name. A function VALUE in the same place works — the
//! closure lane captures the machine's frame (`desugar_generators`'
//! closure capture; `const g = function () { return n; }` answers 2) —
//! so the declaration becomes that value, `let g = function (…) { … }`,
//! in the slot the declaration occupied — the same placement
//! `nested_fns_capture` chose, for the same reason: the closure lane
//! binds what is in scope at its site, so a value hoisted to the top
//! could not name a local declared below it (`let values = []; async
//! function inner() { values.push(1) }` — hoisted, `values` was an
//! unknown identifier). What in-place gives up is calling the function
//! before its text (§10.2.11); a call that early is a type error, as it
//! is on that lane. An `async function` declaration becomes an async
//! function value (`async_fn_value_exprs`), which blade 2 then moves
//! like any other.
//!
//! Async function VALUES declaring a nested function were refused by
//! blade 2 (`declares_fn_or_class`) and kept the synchronous lowering,
//! where an await of an already-settled promise continues in the same
//! job — one tick early against every ordering test
//! (`async-disposable-stack-dispose-sync-calls`). Rewritten, those
//! bodies declare nothing and move.
//!
//! An `async function*` declaration becomes an async generator
//! EXPRESSION (`gen_fn_exprs`, kind AsyncGenerator): it had no lane —
//! `lift_nested_generators` links a sync generator's captures, a nested
//! async generator declaration reading an enclosing local was an
//! unknown identifier — while the expression form takes the frame link
//! the hoist gives an async value's nested generator (rotation 643).
//!
//! Not rewritten, keeping today's lane: a sync generator declaration
//! (`lift_nested_generators` owns it), a generic one (an arrow has no
//! type parameters), one whose body names `arguments` (a declaration
//! owns that binding), and an async generator with a parser-synthesized
//! (`__`-spelled) parameter. Declarations deeper than the body's
//! own statement list (inside a block or a loop) are Annex B territory
//! and stay with `nested_fns`.

use super::{Ast, Expr, ExprId, Stmt};

/// Rewrite every async function body — declarations on the parser's
/// `async_fns` roll and values in `async_fn_value_exprs`.
pub fn desugar_async_nested_fns(ast: &mut Ast) {
    for idx in 0..ast.stmts.len() {
        let Stmt::FnDecl { name, .. } = &ast.stmts[idx] else {
            continue;
        };
        if !ast.async_fns.contains(name) {
            continue;
        }
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[idx] else {
            unreachable!()
        };
        let mut taken = std::mem::take(body);
        rewrite_declarations(ast, &mut taken);
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[idx] else {
            unreachable!()
        };
        *body = taken;
    }
    let mut ids: Vec<ExprId> = ast.async_fn_value_exprs.iter().copied().collect();
    ids.sort_by_key(|e| e.0);
    for eid in ids {
        let Expr::ArrowFn { body, .. } = &ast.exprs[eid.0 as usize] else {
            continue;
        };
        if !body.iter().any(|s| is_rewritable(ast, s)) {
            continue;
        }
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] else {
            unreachable!()
        };
        let mut taken = std::mem::take(body);
        rewrite_declarations(ast, &mut taken);
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] else {
            unreachable!()
        };
        *body = taken;
    }
}

/// The body's direct function declarations become `let` bindings of
/// function values, each in its own slot.
fn rewrite_declarations(ast: &mut Ast, body: &mut [Stmt]) {
    for slot in body.iter_mut() {
        if is_rewritable(ast, slot) {
            let taken = std::mem::replace(slot, Stmt::Block(Vec::new()));
            *slot = to_value_binding(ast, taken);
        }
    }
}

fn is_rewritable(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::FnDecl {
        name,
        type_params,
        params,
        is_generator,
        body,
        ..
    } = s
    else {
        return false;
    };
    // A sync generator declaration keeps `lift_nested_generators`
    // (which links its captures); an ASYNC one had no lane at all —
    // as a generator EXPRESSION it takes the frame link the hoist gives
    // an async value's nested generator (rotation 643). A parameter the
    // parser synthesized (`__`-spelled destructuring prefix) is counted
    // by the hoist for declarations only, so such a declaration stays.
    let generator_ok = !*is_generator
        || (ast.async_generator_fns.contains(name)
            && !params.iter().any(|p| p.name.starts_with("__")));
    generator_ok && type_params.is_empty() && !names_arguments(ast, body)
}

/// `function f(p): R { … }` → `let f = function (p): R { … }`; an async
/// declaration's value is marked async.
fn to_value_binding(ast: &mut Ast, s: Stmt) -> Stmt {
    let Stmt::FnDecl {
        name,
        params,
        return_type,
        body,
        span,
        is_generator,
        ..
    } = s
    else {
        unreachable!("caller checked");
    };
    let eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type,
        body,
    });
    ast.set_expr_span(eid, span);
    if is_generator {
        // An async generator expression, the parser's own marks.
        ast.gen_fn_exprs.insert(
            eid,
            super::GenFnExprInfo {
                kind: super::GenFnExprKind::AsyncGenerator,
                destr_prefix: 0,
            },
        );
    } else {
        ast.fn_expr_exprs.insert(eid);
        if ast.async_fns.contains(&name) {
            ast.async_fn_value_exprs.insert(eid);
        }
    }
    Stmt::LetDecl {
        mutable: false,
        name,
        type_ann: None,
        init: eid,
        is_var: false,
    }
}

/// Does the declaration's body read `arguments` (its own binding, which
/// a function value spelled as an arrow node would not own)? Arrows
/// inside it share the binding, so their bodies count; a nested
/// declaration has its own and is not entered.
fn names_arguments(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_names_arguments(ast, s))
}

fn stmt_names_arguments(ast: &Ast, s: &Stmt) -> bool {
    if matches!(s, Stmt::FnDecl { .. }) {
        return false;
    }
    super::desugar_with::walk::stmt_exprs(s)
        .into_iter()
        .any(|e| expr_names_arguments(ast, e))
        || super::desugar_with::walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_names_arguments(ast, c))
}

fn expr_names_arguments(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n == "arguments",
        Expr::ArrowFn { body, .. } => names_arguments(ast, body),
        _ => super::desugar_with::walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_names_arguments(ast, c)),
    }
}
