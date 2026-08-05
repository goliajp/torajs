//! What a lifted generator local's field annotation is, when the
//! shared sniff cannot answer at this point in the pipeline.
//!
//! Split from `desugar_generators_walkers` when the RFC
//! 20260805-async-fn-state-machine D0 knives took that file to the
//! 500-line HARD limit exactly. The walkers keep the lift; this file
//! keeps the question "what IS this initializer", which is the half
//! that grows every time another shape turns out to have been pinned
//! to `number`.

use super::desugar_generators_walkers::LiftCtx;
use super::{Ast, Expr};

/// What a call answers, for the three call shapes the shared sniff
/// declines: a class's generator method, a local holding a closure,
/// and a `Promise` static. None for every other call, which leaves
/// the fallback exactly where it was.
///
/// The parser hoists `class C { *m() {} }` into a top-level
/// `function* __cm_gen_C__m(__genrecv, ..)` plus an ordinary
/// forwarder method (`parse_class_decl_generator`), so by the time
/// this pass runs `fn_sigs` already carries that hoisted name and the
/// `__Gen_*` class this pass is about to mint for it. The receiver's
/// class was the missing half, and knife 3's `new C()` arm now
/// supplies it: `const b = new Box(); const it = b.each()` had `it`
/// take the `number` fallback, and the checker — which types the call
/// correctly — rejected the store outright ("field is Number, value
/// is ClassRef(`__Gen___cm_gen_Box__each`)").
///
/// Answered here rather than in the shared sniff for the same reason
/// `New` is: the shared sniff has no notion of the hoisted spelling,
/// and teaching it one reaches all of its callers.
fn call_result_ann(
    ast: &Ast,
    callee: super::ExprId,
    args: &[super::ExprId],
    ctx: &LiftCtx,
) -> Option<String> {
    match ast.get_expr(callee) {
        Expr::Member { obj, name } => {
            // `Promise.resolve(x)` — the shared sniff wants the
            // receiver's annotation and `Promise` is a namespace, not
            // a value it can type, so the whole call declined and
            // `const p = Promise.resolve([1, 2, 3])` took the `number`
            // fallback ("field is Number, value is
            // Promise(Array(Number))"). Handled here rather than in
            // the sniff for the same reason `New` is.
            if let Expr::Ident(ns) = ast.get_expr(*obj)
                && ns == "Promise"
                && !ctx.binds.contains_key(ns)
                && !ctx.params.iter().any(|p| p.name == *ns)
            {
                return promise_static_ann(ast, name, args, ctx);
            }
            let recv =
                super::infer_expr_ann_with(&ast.exprs, *obj, ctx.params, &ctx.binds, ctx.fn_sigs)?;
            let hoisted = format!("{}{recv}__{name}", super::GEN_METHOD_PREFIX);
            ctx.fn_sigs.get(&hoisted).cloned()
        }
        // Calling a local that holds a closure. The shared sniff reads
        // `fn_sigs`, which is keyed on top-level function names, so a
        // local was not in it and `const add3 = add(3)` fell to
        // `number` — then `add3(4)` said "not callable: type Number".
        // The local's own field annotation is fn-shaped and says what
        // the call answers.
        Expr::Ident(n) => ctx
            .binds
            .get(n)
            .or_else(|| ctx.params.iter().find(|p| p.name == *n)?.type_ann.as_ref())
            .and_then(|ann| fn_ann_return(ann))
            .map(str::to_string),
        _ => None,
    }
}

/// What a `Promise` static answers, or None to leave the call alone.
///
/// `resolve(x)` is the one that matters here and the one §27.2.4.7
/// makes precise: handing it a promise passes that promise's value
/// through, so the annotation is the argument's own; handing it a
/// plain value makes that value the promise's. `reject` and the
/// combinators are left to decline — each has its own value rule
/// (`allSettled` answers an array of outcome objects, not of values),
/// and a wrong one here pins the field just as badly as the fallback
/// it replaces.
fn promise_static_ann(
    ast: &Ast,
    name: &str,
    args: &[super::ExprId],
    ctx: &LiftCtx,
) -> Option<String> {
    if name != "resolve" {
        return None;
    }
    let Some(arg) = args.first() else {
        // `Promise.resolve()` fulfils with `undefined`.
        return Some("Promise<any>".into());
    };
    let inner = super::infer_expr_ann_with(&ast.exprs, *arg, ctx.params, &ctx.binds, ctx.fn_sigs)?;
    if inner.starts_with("Promise<") {
        return Some(inner);
    }
    Some(format!("Promise<{inner}>"))
}

/// The `R` of a `__cls(P|..)->R` / `__fn(P|..)->R` annotation.
///
/// Depth-aware, because a parameter can itself be fn-shaped —
/// `__cls(__fn(number)->number)->any` closes its own paren before the
/// one that matters.
fn fn_ann_return(ann: &str) -> Option<&str> {
    let rest = ann
        .strip_prefix("__cls(")
        .or_else(|| ann.strip_prefix("__fn("))?;
    let mut depth = 1usize;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return rest[i + 1..].strip_prefix("->");
                }
            }
            _ => {}
        }
    }
    None
}

/// The field annotation for initializers the shared sniff cannot
/// answer at this point in the pipeline, or None to let it try.
///
/// Three shapes, each of which pinned a lifted local to `number` and
/// took every later use of it down:
///
/// * **an arrow** — `infer_expr_ann_with`'s `Expr::Closure` arm reads
///   a signature published under a lifted `__closure_*` name, and this
///   pass runs before `lift_arrow_fns`, so the node is still an
///   `Expr::ArrowFn` and no such name exists yet. See below for why
///   the answer is `__cls(` and not `__fn(`.
/// * **`new C()`** — the constructed class IS the annotation. The
///   shared sniff has no `New` arm and adding one reaches all of its
///   callers, so it is answered here, where the field being typed is.
///   `const s = new Set()` did not compile inside a `function*`.
/// * **`undefined`** — JS's untyped slot is `any`, and `number` made
///   the difference observable: a local holding `undefined` printed
///   `0`.
/// * **a call** — see `call_result_ann`: a class's generator method,
///   a local holding a closure, or a `Promise` static. The shared
///   sniff's method table is keyed on `string` / `T[]` receivers and
///   answers None for all three.
///
/// `infer_expr_ann_with` cannot answer this one: its `Expr::Closure`
/// arm reads a signature `preinfer_closure_sigs` publishes under the
/// lifted `__closure_*` name, and this pass runs before
/// `lift_arrow_fns` — the arrow is still an `Expr::ArrowFn` and no
/// such name exists yet. The shape is right there in the node, so
/// read it.
///
/// `__cls(`, not `__fn(`: the lifted local becomes a *field* of the
/// `__Gen_*` class, and a field slot is a mutable position that can
/// hold a capturing closure — the same retag
/// `lift_arrow_fns::retag_field_fn_ann` performs for object-literal
/// fields, for the same reason.
///
/// Declines unless every parameter carries a written annotation. An
/// unannotated one is filled in later by
/// `infer_anonymous_closure_params`, which knows things this pass
/// does not; guessing `any` here would pin the field against whatever
/// that pass concludes, which is the failure this whole change exists
/// to stop repeating.
pub(super) fn direct_field_ann(ast: &Ast, init: super::ExprId, ctx: &LiftCtx) -> Option<String> {
    let (params, return_type, body) = match ast.get_expr(init) {
        Expr::ArrowFn {
            params,
            return_type,
            body,
        } => (params, return_type, body),
        Expr::New { class_name, .. } => return Some(class_name.clone()),
        Expr::Ident(n) if n == "undefined" => return Some("any".into()),
        Expr::Call { callee, args } => return call_result_ann(ast, *callee, args, ctx),
        _ => return None,
    };
    let mut param_anns: Vec<String> = Vec::with_capacity(params.len());
    for p in params {
        param_anns.push(p.type_ann.clone()?);
    }
    let ret = match return_type {
        Some(rt) => rt.clone(),
        None if !super::body_has_value_return(body) => "void".into(),
        // Seeded, not bare: an arrow in a generator body routinely
        // reads the generator's params and the locals lifted before
        // it (`const f = (n: number) => n + base`), and the bare
        // sniff has no entry for either — it bailed, and the field
        // fell back to `number` while the closure itself came out
        // `Function([Number], Number)`.
        // `any` when the seeded sniff cannot read the body either —
        // the honest answer for a return nothing here can see, and the
        // one an explicit annotation proves works. Declining instead
        // handed the field back to the `number` fallback, which is a
        // claim about the return, not an absence of one: `const add =
        // (n: number) => (m: number) => n + m` (the sniff has no arm
        // for an arrow in return position) came out `number` and every
        // later `add(3)` said "not callable: type Number".
        None => super::infer_return_ann_seeded(&ast.exprs, body, params, &ctx.binds, ctx.fn_sigs)
            .unwrap_or_else(|| "any".into()),
    };
    Some(format!("__cls({})->{}", param_anns.join("|"), ret))
}
