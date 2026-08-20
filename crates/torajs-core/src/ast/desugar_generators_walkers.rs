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

/// Lift the lets of one statement list — every scope that owns one
/// goes through here, so a name a lifted field already claims gets
/// resolved (J.2.b, see `desugar_generators_alpha`) while the
/// declaration is still a declaration and its visibility range is
/// still `list[i..]`.
pub(crate) fn lift_lets_in_list(
    ast: &mut Ast,
    list: &mut [Stmt],
    lifted: &mut Vec<(String, String)>,
    ctx: &mut LiftCtx,
) {
    for i in 0..list.len() {
        super::desugar_generators_alpha::resolve_duplicate_let(ast, &mut list[i..], lifted);
        lift_lets_in_stmt(ast, &mut list[i], lifted, ctx);
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
    // J.2.b — a `for (let i = ..)` counter whose name a lifted field
    // already claims is renamed across the whole loop first; the
    // declaration is about to stop being one. A no-op for every other
    // statement, and for a `for` whose name is still free.
    super::desugar_generators_alpha::resolve_duplicate_for_let(ast, s, lifted);
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
                } else if matches!(ast.get_expr(*init), Expr::Uninit) {
                    // 422-01 — `let v;` (no init, no ann): the binding
                    // starts undefined and takes whatever a later
                    // assignment sends (`v = yield ...` most often).
                    // Pinning number cannot even hold the initial
                    // undefined; the field is an any slot by shape.
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
                    super::desugar_generators_field_ann::field_ann(ast, *init, ctx).unwrap_or_else(
                        || {
                            // r454 — a dstr-assignment source temp the
                            // sniff can't read (an Ident source, an
                            // element load) holds whatever the RHS
                            // was; the number fallback pinned it
                            // against its own init ("field is Number,
                            // value is Array"). Sniffable sources
                            // (array literals) stay on their typed
                            // lane above.
                            if n.starts_with("__dstra_src_") {
                                "any".into()
                            } else {
                                "number".into()
                            }
                        },
                    )
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
        Stmt::Block(stmts) => lift_lets_in_list(ast, stmts, lifted, ctx),
        // Not a scope of its own: `Stmt::Multi` is the wrapper the
        // yield-into expansion puts around a `[yield e; let v = ..]`
        // pair, so a `let` inside it belongs to the list that holds
        // the Multi — which has already run the J.2.b hook over a
        // range that covers it.
        Stmt::Multi(stmts) => {
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
                // Three separate scopes — a `let` in the body and one
                // in the catch are the J.2.b collision this pass is
                // most often asked about.
                lift_lets_in_list(ast, body, lifted, ctx);
                lift_lets_in_list(ast, catch_body, lifted, ctx);
                if let Some(fs) = finally_body {
                    lift_lets_in_list(ast, fs, lifted, ctx);
                }
            }
        }
        // Switch cases don't yet support yields (J.2.b scope)
        // so their inner lets stay as plain locals — no lift needed.
        _ => {}
    }
}
