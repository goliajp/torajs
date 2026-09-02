//! What a lifted generator local's field annotation is, when the
//! shared sniff cannot answer at this point in the pipeline.
//!
//! Split from `desugar_generators_walkers` when the RFC
//! 20260805-async-fn-state-machine D0 knives took that file to the
//! 500-line HARD limit exactly. The walkers keep the lift; this file
//! keeps the question "what IS this initializer", which is the half
//! that grows every time another shape turns out to have been pinned
//! to `number`.

use super::desugar_generators_field_ann_builtin::{
    global_ctor_ann, object_static_ann, promise_static_ann,
};
use super::desugar_generators_walkers::LiftCtx;
use super::{Ast, BinOp, Expr, Stmt};
use crate::ast::PropKey;

/// This file's answer for `eid`, or the shared sniff's.
///
/// The lift's caller composes the two exactly this way for the
/// initializer as a whole, but until now nothing composed them
/// *inside* an expression — so a shape this file knows stopped being
/// known the moment it appeared as somebody's sub-expression. A
/// generator holding `const ps = [Promise.resolve(2.5)]` fell back to
/// `number` ("field is Number, value is Array(Promise(Number))")
/// because the shared sniff's array arm recursed into its own arms
/// only, and `Promise` is a namespace it cannot type. Same for
/// `[new C()]`, and for `Promise.resolve(new C())`.
///
/// Composing this way rather than teaching the shared sniff keeps the
/// reach unchanged: every arm below declines where it has nothing to
/// say, so the shared sniff still answers everything it answered
/// before.
pub(super) fn field_ann(ast: &Ast, eid: super::ExprId, ctx: &LiftCtx) -> Option<String> {
    direct_field_ann(ast, eid, ctx).or_else(|| {
        super::infer_expr_ann_with(&ast.exprs, eid, ctx.params, &ctx.binds, ctx.fn_sigs)
    })
}

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
                && !ctx.binds.contains_key(ns)
                && !ctx.params.iter().any(|p| p.name == *ns)
            {
                if ns == "Promise" {
                    return promise_static_ann(ast, name, args, ctx);
                }
                if ns == "Object" {
                    return object_static_ann(name);
                }
            }
            let recv = field_ann(ast, *obj, ctx)?;
            // A method on a regex receiver. Nothing else answers for
            // one: the shared sniff's method table is keyed on
            // `string` / `T[]` receivers, so `const m = r.exec(s)` in
            // a generator fell back to `number` even once `r` itself
            // read as a RegExp.
            if recv == "RegExp" {
                return match name.as_str() {
                    "exec" => Some("__nullable(string[])".into()),
                    "test" => Some("boolean".into()),
                    _ => None,
                };
            }
            let hoisted = format!("{}{recv}__{name}", super::GEN_METHOD_PREFIX);
            ctx.fn_sigs.get(&hoisted).cloned()
        }
        // Calling a local that holds a closure. The shared sniff reads
        // `fn_sigs`, which is keyed on top-level function names, so a
        // local was not in it and `const add3 = add(3)` fell to
        // `number` — then `add3(4)` said "not callable: type Number".
        // The local's own field annotation is fn-shaped and says what
        // the call answers.
        Expr::Ident(n) => {
            let local = ctx
                .binds
                .get(n)
                .or_else(|| ctx.params.iter().find(|p| p.name == *n)?.type_ann.as_ref());
            match local {
                Some(ann) => fn_ann_return(ann).map(str::to_string),
                // Not a local — one of the global constructors whose
                // answer the spec fixes. `fn_sigs` is keyed on
                // top-level USER functions, so these were absent from
                // it and the call declined: a template literal is the
                // one that bites, since the parser lowers each
                // substitution through a synthesized `String(..)`
                // wrapper, and one unreadable operand takes the whole
                // concatenation down — `const t = ` + "`a${1}b`" + `
                // took the `number` fallback.
                None => global_ctor_ann(n),
            }
        }
        _ => None,
    }
}

/// An array literal's element type, read through `field_ann` so the
/// elements get this file's arms as well as the shared sniff's.
///
/// The widening rule is the shared sniff's array arm verbatim: the
/// first element anchors, and a literal whose remaining elements do
/// not all agree with it — including any element that cannot be read
/// at all — widens to `any[]` rather than being stamped with the
/// anchor's type. An empty literal declines, as it does there.
fn array_ann(ast: &Ast, elems: &[super::ExprId], ctx: &LiftCtx) -> Option<String> {
    // `[]` — nothing to sniff, and declining here reached the
    // `number` fallback, which rejected the very store that
    // initializes the field ("field is Number, value is Array(Any)"
    // — the dstr-assign src temp of `[x = yield] = []`). The empty
    // array's honest answer is `any[]`.
    if elems.is_empty() {
        return Some("any[]".into());
    }
    let first = field_ann(ast, *elems.first()?, ctx)?;
    let uniform = elems
        .iter()
        .skip(1)
        .all(|e| field_ann(ast, *e, ctx).as_deref() == Some(first.as_str()));
    Some(format!("{}[]", if uniform { first } else { "any".into() }))
}

/// An object literal's `__inlobj(` annotation, read through
/// `field_ann` for the same reason `array_ann` is.
///
/// The shared sniff has this arm too, and it declines the moment one
/// field is a shape only this file knows — an arrow being the one that
/// matters, since `Expr::Closure` is what its own arm reads and this
/// pass runs before `lift_arrow_fns` mints those. So
/// `const t = { n: 1, f: (x: number) => x * 2 }` in a generator took
/// the `number` fallback, and `t.f(t.n)` after it reported "no member
/// `.f` on type Number".
///
/// Fields keep the same field-position retag the shared arm applies: a
/// fn-valued field slot is a mutable position that can hold a
/// capturing closure, so `__fn(` becomes `__cls(`.
fn objlit_ann(ast: &Ast, fields: &[(PropKey, super::ExprId)], ctx: &LiftCtx) -> Option<String> {
    let parts: Vec<String> = fields
        .iter()
        .map(|(n, eid)| {
            field_ann(ast, *eid, ctx).map(|t| {
                format!(
                    "{}:{}",
                    n.lossy(),
                    super::lift_arrow_fns::retag_field_fn_ann(&t)
                )
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!("__inlobj({})", parts.join("|")))
}

/// The `R` of a `__cls(P|..)->(R)` / `__fn(P|..)->(R)` annotation.
///
/// Depth-aware, because a parameter can itself be fn-shaped —
/// `__cls(__fn(number)->(number))->any` closes its own paren before the
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
                    return crate::type_ann_fnsig::ret_of_tail(&rest[i + 1..]);
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
/// * **an array literal** — the shared sniff has this arm already, but
///   it recurses into its own arms, so an array OF any shape above was
///   still unreadable: `const ps = [Promise.resolve(2.5)]` and
///   `const cs = [new C()]` both took the `number` fallback. Repeated
///   here over `field_ann` so the elements get both halves. The
///   widening rule is the shared arm's, unchanged — the first element
///   anchors, anything that disagrees or cannot be read widens the
///   whole literal to `any[]` (the checker's T-10.c anchor widening;
///   answering the first element's type for a mixed literal stamps an
///   annotation its `Arr<Any>` lowering cannot satisfy).
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
fn direct_field_ann(ast: &Ast, init: super::ExprId, ctx: &LiftCtx) -> Option<String> {
    let (params, return_type, body) = match ast.get_expr(init) {
        Expr::ArrowFn {
            params,
            return_type,
            body,
        } => (params, return_type, body),
        // A class declared IN THIS GENERATOR BODY never reaches the
        // top-level class index — the capturing lane α-renames it to
        // `__cc<N>_C` and its name stops being a type name — so the
        // class-name annotation would be an `unknown type` at check
        // and an `__tvdefault__C` factory seed. `any` is the honest
        // slot type (it matches the `const __cc<N>_C: any` value the
        // ES5 lane itself produces).
        Expr::New { class_name, .. } if ctx.local_classes.contains(class_name) => {
            return Some("any".into());
        }
        Expr::New { class_name, .. } => return Some(class_name.clone()),
        Expr::Ident(n) if n == "undefined" => return Some("any".into()),
        // `null` is the same case as `undefined` one line up: JS's
        // untyped slot is `any`, and `number` made the difference
        // observable — a local holding `null` did not compile at all
        // ("field is Number, value is Null").
        Expr::Null => return Some("any".into()),
        // A bigint literal is its own type, like the regex literal
        // below; nothing else answered for it.
        Expr::BigInt { .. } => return Some("bigint".into()),
        Expr::Call { callee, args } => return call_result_ann(ast, *callee, args, ctx),
        Expr::Array(elems) => return array_ann(ast, elems, ctx),
        // `+` is the third place the two halves had to meet. The
        // shared sniff propagates string-ness through it, but again
        // over its own arms only — so one operand this file alone can
        // read took the whole concatenation down. A template literal
        // is the everyday case: the parser lowers each substitution
        // through a synthesized `String(..)` wrapper, so
        // `const t = `a${1}b`` had an unreadable operand and fell to
        // the `number` fallback. Only the two unambiguous shapes are
        // answered here; anything else declines and the shared arm
        // takes it, typevars included.
        Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } => {
            let (l, r) = (field_ann(ast, *left, ctx)?, field_ann(ast, *right, ctx)?);
            return match (l.as_str(), r.as_str()) {
                ("string", _) | (_, "string") => Some("string".into()),
                ("number", "number") => Some("number".into()),
                _ => None,
            };
        }
        Expr::ObjectLit { fields } => return objlit_ann(ast, fields, ctx),
        // A regex literal is its own type, and nothing else answers
        // for it: `const r = /a(b)c/` in a generator took the `number`
        // fallback and every use of it after ("no member `.exec` on
        // type Number") went down with it.
        Expr::Regex { .. } => return Some("RegExp".into()),
        // A load off an `any` — either spelling. The shared sniff has
        // arms for both, and both answer by taking the base's
        // annotation APART: `Index` strips a `[]` suffix, `Member`
        // looks the name up in a receiver table keyed on `string` /
        // `T[]`. An `any` base has nothing to take apart, so both
        // DECLINE — and declining here is not "no assertion", it is
        // the `number` fallback, which the r454 note above says in as
        // many words for `Expr::Ident`.
        //
        // Both spellings are load-bearing and both are in injected
        // code: `desugar_using`'s `__torajs_using_dispose_async`
        // opens with `const st = env.stack` and then indexes it
        // (`const r = st[i]`), so the whole `using` / DisposableStack
        // family reads "no member `.length` on type Number" the
        // moment those helpers become generators.
        //
        // A base with a real annotation falls through to the shared
        // arms untouched — this one only speaks where they cannot,
        // which is also why it does NOT try to answer for a typed
        // receiver whose member type it would have to go and look up.
        Expr::Index { obj, .. } | Expr::Member { obj, .. } => {
            return (field_ann(ast, *obj, ctx)? == "any").then(|| "any".into());
        }
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
    Some(crate::type_ann_fnsig::fn_type_ann(
        "__cls",
        &param_anns.join("|"),
        &ret,
    ))
}

/// Every top-level function's *declared* return type, keyed by name —
/// the `fn_sigs` map D0's let-lift consults when a generator local is
/// initialized by a call.
///
/// Declared, not inferred: this pass runs well before
/// `desugar_implicit_generics`, so an inferred signature does not
/// exist yet. A call to a function that never wrote its return type
/// is simply absent from the map, and the lift falls back exactly as
/// it did before.
///
/// A `function*` is the exception, and the reason is that its
/// declared return type describes what it YIELDS, not what calling it
/// answers. `function* ag(): any` called as `ag()` answers the
/// iterator object — which this very pass is about to mint as
/// `__Gen_ag` — so that is what the map says. Reading the annotation
/// instead pinned `const it = ag()` to the yield type and every
/// `it.next()` after it failed ("no member `.next` on type Number").
pub(super) fn collect_declared_fn_return_types(
    ast: &Ast,
) -> std::collections::HashMap<String, String> {
    ast.stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name,
                is_generator: true,
                ..
            } => Some((name.clone(), format!("__Gen_{name}"))),
            Stmt::FnDecl {
                name,
                return_type: Some(rt),
                ..
            } => Some((name.clone(), normalize_void(rt))),
            // A function with no value return answers `undefined`
            // (§14.10) whether or not it wrote `: void` — the same
            // rule `preinfer_closure_sigs` applies to a lifted
            // closure. Without it `const a = sideEffectOnly()` took
            // the `number` fallback and would not compile ("field is
            // Number, value is Undefined").
            Stmt::FnDecl { name, body, .. } if !super::body_has_value_return(body) => {
                Some((name.clone(), "any".to_string()))
            }
            // An UNANNOTATED function with a value return: JS's
            // untyped slot is `any` — the same answer `undefined` /
            // `null` initializers get in `direct_field_ann`, and for
            // the same reason. Absent from the map, the call declined
            // and the field took the `number` fallback, which is a
            // CLAIM about the value: a function answering a symbol
            // (the t262 forbidden-ext `inner()` shape) had its result
            // stored through a number slot and surfaced as a raw
            // pointer error.
            Stmt::FnDecl { name, .. } => Some((name.clone(), "any".to_string())),
            // 551-02 — a top-level `const s = (n): T => …` binding is
            // called exactly like a declared fn, and its arrow's
            // DECLARED return type answers the same question (this
            // pass runs before lift_arrow_fns, so the init is still
            // `Expr::ArrowFn`). Same declared-not-inferred rule: an
            // unannotated arrow answers `any`. Absent from the map,
            // the call declined and `const t = s(k)` across a yield
            // took the `number` fallback — a String value through a
            // Number field.
            Stmt::LetDecl { name, init, .. } => match ast.get_expr(*init) {
                Expr::ArrowFn {
                    return_type: Some(rt),
                    ..
                } => Some((name.clone(), normalize_void(rt))),
                Expr::ArrowFn { .. } => Some((name.clone(), "any".to_string())),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// `void` as a FIELD annotation, which is what a lifted local becomes:
/// `any`. A field has to be able to hold the value, and the value a
/// void call produces is `undefined` — which is what an `any` slot
/// holds and a `void` slot cannot. Left as written, `const b =
/// voidCall()` yielded `null`.
fn normalize_void(rt: &str) -> String {
    if rt == "void" {
        "any".into()
    } else {
        rt.into()
    }
}
