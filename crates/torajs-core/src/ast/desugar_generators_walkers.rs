//! Generator body walkers: yield-into expansion + let-lift.
//!
//! Extracted from `ast/desugar_generators.rs` when the RFC 20260713
//! blade work pushed it past the 500-line HARD limit. Both walkers
//! are `pub(crate)` re-exported through `ast::desugar_generators` so
//! `ast::desugar_generators_prep`'s existing import path
//! (`super::{expand_yield_into_in_stmt, lift_lets_in_stmt}`) keeps
//! resolving.

use super::{Ast, Expr, Param, Stmt};

/// What the let-lift knows about the generator it is lifting out of,
/// so an unannotated local can be typed by what its initializer says
/// rather than by a constant guessed before anything is read.
///
/// RFC 20260805-async-fn-state-machine D0. `params` are the
/// generator's own parameters (annotated by
/// `desugar_one_generator` before the prep runs, so every one of
/// them answers); `binds` accumulates the locals already lifted in
/// this body, which is what lets `const b = a + 1` follow `const a
/// = 1`; `fn_sigs` maps a top-level function's name to its declared
/// return type. The pass runs before `desugar_implicit_generics`, so
/// only annotations the source actually wrote are available — an
/// inferred one would not exist yet, and guessing at one here would
/// be a second inference disagreeing with the checker's.
pub(crate) struct LiftCtx<'a> {
    pub(crate) params: &'a [Param],
    pub(crate) fn_sigs: &'a std::collections::HashMap<String, String>,
    pub(crate) binds: std::collections::HashMap<String, String>,
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
/// * **a call of a class's generator method** — see
///   `gen_method_call_ann`. The shared sniff's method table is keyed
///   on `string` / `T[]` receivers and answers None for a user class.
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
/// The iterator class `recv.m()` answers when `m` is a generator
/// method of the class `recv` holds, or None for every other call.
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
fn gen_method_call_ann(ast: &Ast, callee: super::ExprId, ctx: &LiftCtx) -> Option<String> {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return None;
    };
    let recv = super::infer_expr_ann_with(&ast.exprs, *obj, ctx.params, &ctx.binds, ctx.fn_sigs)?;
    let hoisted = format!("{}{recv}__{name}", super::GEN_METHOD_PREFIX);
    ctx.fn_sigs.get(&hoisted).cloned()
}

fn direct_field_ann(ast: &Ast, init: super::ExprId, ctx: &LiftCtx) -> Option<String> {
    let (params, return_type, body) = match ast.get_expr(init) {
        Expr::ArrowFn {
            params,
            return_type,
            body,
        } => (params, return_type, body),
        Expr::New { class_name, .. } => return Some(class_name.clone()),
        Expr::Ident(n) if n == "undefined" => return Some("any".into()),
        Expr::Call { callee, .. } => return gen_method_call_ann(ast, *callee, ctx),
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
        None => super::infer_return_ann_seeded(&ast.exprs, body, params, &ctx.binds, ctx.fn_sigs)?,
    };
    Some(format!("__cls({})->{}", param_anns.join("|"), ret))
}

/// J.4 — recursively expand every `Stmt::YieldInto { var, type_ann,
/// value }` in `s` into the pair `[Stmt::Yield(value);
/// Stmt::LetDecl { name: var, type_ann, init: this.__sent }]`. The
/// pair is wrapped in `Stmt::Multi` so it occupies the YieldInto's
/// original slot without disturbing surrounding scope. Walks into
/// nested control-flow.
///
/// `yield_ty` is the surrounding generator's declared yield type; it
/// supplies the let's annotation when the user omitted one (so the
/// J.2.b lift picks the right field type).
pub(crate) fn expand_yield_into_in_stmt(ast: &mut Ast, s: &mut Stmt, yield_ty: &str) {
    match s {
        Stmt::YieldInto {
            var,
            type_ann,
            value,
        } => {
            let var = std::mem::take(var);
            let ty = type_ann.clone().or_else(|| Some(yield_ty.to_string()));
            let value = *value;
            let yield_stmt = Stmt::Yield(value);
            let this_id = ast.add_expr(Expr::This);
            let sent_member = ast.add_expr(Expr::Member {
                obj: this_id,
                name: "__sent".into(),
            });
            let let_stmt = Stmt::LetDecl {
                mutable: true,
                name: var,
                type_ann: ty,
                init: sent_member,
                is_var: false,
            };
            *s = Stmt::Multi(vec![yield_stmt, let_stmt]);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            expand_yield_into_in_stmt(ast, then_branch, yield_ty);
            if let Some(eb) = else_branch.as_deref_mut() {
                expand_yield_into_in_stmt(ast, eb, yield_ty);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::Labeled { body, .. } => {
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                expand_yield_into_in_stmt(ast, i, yield_ty);
            }
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                expand_yield_into_in_stmt(ast, s, yield_ty);
            }
        }
        // RFC 20260802 — try/catch is in yield scope now. Descending
        // into a yield-free try is a no-op (nothing to expand), so no
        // gate is needed here.
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body.iter_mut().chain(catch_body.iter_mut()) {
                expand_yield_into_in_stmt(ast, s, yield_ty);
            }
            if let Some(fs) = finally_body {
                for s in fs {
                    expand_yield_into_in_stmt(ast, s, yield_ty);
                }
            }
        }
        // Switch cases not yet in yield scope (J.2.b).
        _ => {}
    }
}

/// Recursively replace every `let x = init` in `s` (and any nested
/// stmts) with `this.x = init`, recording each lifted `(name, type)`
/// in `lifted`. Used by `desugar_generators` so locals declared in
/// for-init / if-branches / while-bodies survive yield boundaries
/// the same way top-level lets do.
pub(crate) fn lift_lets_in_stmt(
    ast: &mut Ast,
    s: &mut Stmt,
    lifted: &mut Vec<(String, String)>,
    ctx: &mut LiftCtx,
) {
    match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            let n = name.clone();
            // Knife 2a — an untyped alias of `arguments` lifts as
            // `any`: the capture rewrite turns the init into the
            // any-typed `this.arguments` field read, and the
            // historical "number" fallback would pin the lifted
            // field's type wrong (`.length on Number`).
            let t = type_ann.clone().unwrap_or_else(|| {
                if matches!(ast.get_expr(*init), Expr::Ident(nm)
                    if nm == "arguments" || nm == crate::ast::GEN_ARGV_PARAM)
                {
                    "any".into()
                } else if super::free_vars::free_vars_of_body(
                    ast,
                    &[],
                    std::slice::from_ref(&Stmt::Expr(*init)),
                )
                .iter()
                .any(|n| n.starts_with("__yx_"))
                {
                    // RFC 20260802-yield-expr-hoist — an untyped local
                    // whose init reads a hoisted yield-resumption temp
                    // (`__yx_*`, always any) rides the any lane; the
                    // "number" fallback would pin the lifted field's
                    // type against whatever next() actually sends.
                    "any".into()
                } else if n.starts_with("__forof_src_")
                    || n.starts_with("__forof_destr_")
                    || n.starts_with("__ary_src_")
                    || n.starts_with("__nested_destr_")
                {
                    // r283 — the parser's for-of / destructure desugar
                    // temps carry NO annotation (checker-inferred
                    // outside a generator: the hoisted source array,
                    // the destructured loop element, and the pattern-
                    // unpack aliases); lifted to fields the "number"
                    // fallback pinned them against their array/element
                    // inits (t262 for-await-of dstr family, 366-case
                    // cluster "field is Number, value is Array(...)"
                    // plus the plain-generator `for (const [a] of
                    // [[7]])` shape). `__forof_i_N` stays on the
                    // number fallback — it really is the counter.
                    "any".into()
                } else {
                    // D0 (RFC 20260805-async-fn-state-machine) — ask
                    // the initializer. `number` was right for the loop
                    // counters a hand-written generator lifts and
                    // wrong for everything else a body can hold: a
                    // `function*` with `const xs = [1, 2]` in it does
                    // not compile today ("field is Number, value is
                    // Array(Number)"). The carve-outs above outrank
                    // this — each is a deliberate `any` for a slot
                    // whose value the syntax cannot see (a resumption
                    // temp, an `arguments` alias) — and `number`
                    // stays as what is left when the sniff declines,
                    // so every shape it cannot read keeps today's
                    // behaviour.
                    direct_field_ann(ast, *init, ctx)
                        .or_else(|| {
                            super::infer_expr_ann_with(
                                &ast.exprs,
                                *init,
                                ctx.params,
                                &ctx.binds,
                                ctx.fn_sigs,
                            )
                        })
                        .unwrap_or_else(|| "number".into())
                }
            });
            ctx.binds.insert(n.clone(), t.clone());
            lifted.push((n.clone(), t));
            let this_id = ast.add_expr(Expr::This);
            let m = ast.add_expr(Expr::Member {
                obj: this_id,
                name: n,
            });
            let assign = ast.add_expr(Expr::Assign {
                target: m,
                value: *init,
            });
            *s = Stmt::Expr(assign);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            lift_lets_in_stmt(ast, then_branch, lifted, ctx);
            if let Some(eb) = else_branch.as_deref_mut() {
                lift_lets_in_stmt(ast, eb, lifted, ctx);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            lift_lets_in_stmt(ast, body, lifted, ctx);
        }
        Stmt::Labeled { body, .. } => {
            lift_lets_in_stmt(ast, body, lifted, ctx);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                lift_lets_in_stmt(ast, i, lifted, ctx);
            }
            lift_lets_in_stmt(ast, body, lifted, ctx);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                lift_lets_in_stmt(ast, s, lifted, ctx);
            }
        }
        // RFC 20260802 — a yield-bearing try gets region-lowered, so
        // its lets must survive yield boundaries like everyone else's.
        // Yield-FREE trys keep today's inline shape (lets stay plain
        // locals — no lift, no collision-panic surface change).
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            let has_yield = body
                .iter()
                .chain(catch_body.iter())
                .chain(finally_body.iter().flatten())
                .any(super::desugar_generators_sm_rewrite::stmt_contains_yield);
            if has_yield {
                for s in body.iter_mut().chain(catch_body.iter_mut()) {
                    lift_lets_in_stmt(ast, s, lifted, ctx);
                }
                if let Some(fs) = finally_body {
                    for s in fs {
                        lift_lets_in_stmt(ast, s, lifted, ctx);
                    }
                }
            }
        }
        // Switch cases don't yet support yields (J.2.b scope)
        // so their inner lets stay as plain locals — no lift needed.
        _ => {}
    }
}
