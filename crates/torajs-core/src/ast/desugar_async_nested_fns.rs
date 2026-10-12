//! A `function` declared directly in the body of an ASYNC function or a
//! GENERATOR becomes a function value bound in that body, before the
//! body is moved into its state machine (RFC 20260805 blades 1-2).
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
//! in the slot the declaration occupied, or just above the first
//! statement that reaches it when one comes earlier (§10.2.11 initializes
//! the declaration before the body runs;
//! `nested_fns_capture::hoist_above_first_use`). A local the move leaves
//! below the value is a forward capture, which the machine's closure
//! lane serves. An `async function` declaration becomes an async
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
//! Its binding rises toward the top of the body as far as the names it
//! reads allow, so a call written above the declaration finds it live.
//!
//! Not rewritten, keeping today's lane: a sync generator declaration
//! (`lift_nested_generators` owns it), a generic one (an arrow has no
//! type parameters), one whose body names `arguments` (a declaration
//! owns that binding), and an async generator with a parser-synthesized
//! (`__`-spelled) parameter — nor any peer that names one of those or
//! is named by one, since a lifted declaration cannot name a value. In
//! a generator body, a group of declarations that reads nothing around
//! it stays a declaration too, and a direct call (`select`).
//! Declarations deeper than the body's own statement list (inside a
//! block or a loop) are Annex B territory and stay with `nested_fns`.

use super::{Ast, Expr, ExprId, Stmt};

mod lift;
mod select;
use select::Route;

/// Rewrite every async function and generator body — declarations on
/// the parser's `async_fns` roll or marked `is_generator`, and values in
/// `async_fn_value_exprs` / `gen_fn_exprs`.
pub fn desugar_async_nested_fns(ast: &mut Ast) {
    let mut lifts = lift::Lifts::new(ast);
    for idx in 0..ast.stmts.len() {
        let Stmt::FnDecl {
            name,
            is_generator,
            params,
            body,
            ..
        } = &ast.stmts[idx]
        else {
            continue;
        };
        if !*is_generator && !ast.async_fns.contains(name) {
            continue;
        }
        // a top-level generator: its own frame is all there is around
        // a declaration in its body
        let frames = (!ast.async_fns.contains(name)).then(|| {
            let mut out = Vec::new();
            super::scope_chain::fn_names(params, body, &mut out);
            super::scope_chain::list_names(body, &mut out);
            out
        });
        let parent = name.clone();
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[idx] else {
            unreachable!()
        };
        let mut taken = std::mem::take(body);
        rewrite_declarations(ast, &mut taken, &mut lifts, &parent, frames.as_deref());
        let Stmt::FnDecl { body, .. } = &mut ast.stmts[idx] else {
            unreachable!()
        };
        *body = taken;
    }
    let mut ids: Vec<ExprId> = ast.async_fn_value_exprs.iter().copied().collect();
    ids.extend(ast.gen_fn_exprs.keys().copied());
    ids.sort_by_key(|e| e.0);
    ids.dedup();
    let chains = super::scope_chain::chains_by_arrow(ast, &[&ast.stmts]);
    for eid in ids {
        let Expr::ArrowFn { body, .. } = &ast.exprs[eid.0 as usize] else {
            continue;
        };
        if !body.iter().any(|s| matches!(s, Stmt::FnDecl { .. })) {
            continue;
        }
        let frames = (!ast.async_fn_value_exprs.contains(&eid)).then(|| {
            let mut out = super::scope_chain::arrow_frames(ast, chains.get(&eid), eid, body);
            super::scope_chain::list_names(body, &mut out);
            out
        });
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] else {
            unreachable!()
        };
        let mut taken = std::mem::take(body);
        rewrite_declarations(ast, &mut taken, &mut lifts, "__genexpr", frames.as_deref());
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] else {
            unreachable!()
        };
        *body = taken;
    }
    lifts.finish(ast);
}

/// The body's direct function declarations `select` routes to values
/// become `let` bindings of function values, each in its own slot — an
/// async generator's rises as far as what it reads allows, so a call
/// above it finds it live (`hoist_gen_fn_exprs_decls::raise_binding`).
/// Those it routes to a lift go to top level (`lift`).
/// `capture_frames` is `None` for an async body (see `select`).
fn rewrite_declarations(
    ast: &mut Ast,
    body: &mut [Stmt],
    lifts: &mut lift::Lifts,
    parent: &str,
    capture_frames: Option<&[String]>,
) {
    let routes = select::select(ast, body, &lifts.globals, capture_frames);
    let decls: Vec<usize> = (0..body.len())
        .filter(|i| matches!(body[*i], Stmt::FnDecl { .. }))
        .collect();
    let of = |r: Route| -> Vec<usize> {
        (0..decls.len())
            .filter(|k| routes[*k] == r)
            .map(|k| decls[k])
            .collect()
    };
    // first, while every index still holds: a lifted slot empties in place
    lifts.lift(ast, body, &of(Route::Lift), parent);
    let mut values: Vec<ExprId> = Vec::new();
    // ascending, so a raise (which rotates only slots above `i`) leaves
    // the later ones where they were
    for i in of(Route::Value) {
        let taken = std::mem::replace(&mut body[i], Stmt::Block(Vec::new()));
        let reads = match &taken {
            Stmt::FnDecl {
                name,
                params,
                body: fn_body,
                is_generator: true,
                ..
            } => Some(super::free_vars::free_vars_of_arrow(
                ast,
                params,
                fn_body,
                &[],
                &[],
                Some(name),
            )),
            _ => None,
        };
        body[i] = to_value_binding(ast, taken);
        if let Some(reads) = reads {
            super::hoist_gen_fn_exprs_decls::raise_binding(body, i, &reads);
        } else if let Stmt::LetDecl { init, .. } = &body[i] {
            values.push(*init);
        }
    }
    // a call written above a declaration finds the function live
    // (§10.2.11), as in an ordinary body
    let sites: Vec<usize> = body
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, Stmt::LetDecl { init, .. } if values.contains(init)))
        .map(|(i, _)| i)
        .collect();
    super::nested_fns_capture::hoist_above_first_use(ast, body, &sites);
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

pub(super) fn expr_names_arguments(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n == "arguments",
        Expr::ArrowFn { body, .. } => names_arguments(ast, body),
        _ => super::desugar_with::walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_names_arguments(ast, c)),
    }
}
