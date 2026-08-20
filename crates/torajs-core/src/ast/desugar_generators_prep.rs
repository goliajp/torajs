//! Generator body preparation: yield-into expansion + let-lift +
//! params/lifted-locals rewrite to `this.<name>`.
//!
//! Chunk 339 — extracted from `ast::desugar_generators` to keep the
//! per-generator loop body close to the 200-LOC HARD limit. Given
//! the raw generator body, runs three contiguous passes before the
//! state machine sees it:
//!
//! 1. **yield-into expand** — every `let v(:T)? = yield <e>;`
//!    becomes the pair `[yield <e>; let v(:T)? = this.__sent;]`
//!    via `expand_yield_into_in_stmt`. The pair is wrapped in
//!    `Stmt::Multi` so it occupies the original slot without
//!    disturbing surrounding scope.
//! 2. **let-lift** — every `let x: T = init` lifts to a class field
//!    (recorded in `lifted_locals`) so the binding survives yield
//!    boundaries. Same-name lets in different scopes would both map
//!    to the same field; the second one is renamed across its own
//!    visibility range instead (`desugar_generators_alpha`), and the
//!    panic below is left for the one range shape a rename cannot
//!    cover.
//! 3. **params + locals rewrite** — every Ident matching a
//!    generator param or lifted local rewrites to `this.<name>` so
//!    the iterator class's field path picks it up.
//!
//! Returns `(prepared_body, lifted_locals)` so the caller can pass
//! both into the class-assembly step.

use super::desugar_generators_walkers::LiftCtx;
use super::desugar_generators_walkers::lift_lets_in_list;
use super::{Ast, Expr, Param, Stmt, expand_yield_into_in_stmt};

pub(super) fn prep_generator_body(
    ast: &mut Ast,
    mut gen_body: Vec<Stmt>,
    gen_name: &str,
    gen_params: &[Param],
    yield_ty: &str,
    fn_sigs: &std::collections::HashMap<String, String>,
) -> (Vec<Stmt>, Vec<(String, String)>) {
    // F1 (RFC 20260728-gen-forof-yieldstar) — pass 0: rewrite every
    // yield-bearing `for…of` into the manual iterator protocol
    // BEFORE the lift, so the iterator / step bindings it mints
    // become class fields that survive yield boundaries.
    super::desugar_generators_forof::rewrite_yield_forof(ast, &mut gen_body);
    // J.4 — expand every `let v(:T)? = yield <e>;` into the pair
    // [Stmt::Yield(e); Stmt::LetDecl { init: this.__sent }] so the
    // rest of the pipeline only sees standard Yield + LetDecl. The
    // `this.__sent` reference picks up whatever was passed to
    // `g.next(arg)` on the resume.
    for s in &mut gen_body {
        expand_yield_into_in_stmt(ast, s, yield_ty);
    }
    // After expansion, gen_body may contain Multi(Vec<Stmt>) holding
    // the [Yield; LetDecl] pair. The recursive let-lift below walks
    // Multi just fine.

    let mut lifted_locals: Vec<(String, String)> = Vec::new();
    // D0 — the lift asks each initializer what the local is (see
    // `LiftCtx`). The generator's own params seed the lookup; each
    // lifted local joins `binds` as it lands, so a local reading an
    // earlier one resolves.
    let mut local_classes = std::collections::HashSet::new();
    collect_local_classes(&gen_body, &mut local_classes);
    // The CLASS BINDING itself must survive yields too: the class
    // value the capturing lane will mint (`const __cc<N>_C`) is born
    // AFTER this lift ran, inside one state arm — a cross-yield read
    // (`C[yield 9]()`) lands in a different arm where that local
    // does not exist. Give each generator-local class a same-named
    // `any` state-machine field: its name joins the rewrite set (a
    // value-position `C` becomes `this.C`), and a bridge assignment
    // `this.C = C` right after the ClassDecl (inserted AFTER the
    // rewrite so its own rhs keeps the bare name for the lane's
    // α-rename) stores the minted value into the field. `new C()`
    // spells the class as a string, not an Ident — the lane's
    // same-list rename owns it, untouched here.
    for c in &local_classes {
        lifted_locals.push((c.clone(), "any".into()));
    }
    let mut lift_ctx = LiftCtx {
        params: gen_params,
        fn_sigs,
        binds: gen_params
            .iter()
            .filter_map(|p| p.type_ann.clone().map(|a| (p.name.clone(), a)))
            .collect(),
        local_classes,
    };
    lift_lets_in_list(ast, &mut gen_body, &mut lifted_locals, &mut lift_ctx);
    for i in 0..lifted_locals.len() {
        for j in (i + 1)..lifted_locals.len() {
            if lifted_locals[i].0 == lifted_locals[j].0 {
                panic!(
                    "function* {gen_name}: duplicate `let {}` declarations across \
                     scopes, and one of the two ranges declares the name again \
                     deeper in — renaming it would have to stop at that inner \
                     declaration. Rename one by hand (J.2.b residue; the plain \
                     sibling-scope case is resolved automatically).",
                    lifted_locals[i].0
                );
            }
        }
    }
    // Names to rewrite to `this.<name>`: generator params + lifted
    // locals. Both share the same identifier-shadowing semantics
    // for our MVP (no shadowing).
    let mut all_names: Vec<Param> = gen_params.to_vec();
    for (n, t) in &lifted_locals {
        all_names.push(Param {
            name: n.clone(),
            type_ann: Some(t.clone()),
            default: None,
            is_rest: false,
        });
    }
    super::desugar_generators_rewrite::rewrite_params_to_this(ast, &gen_body, &all_names);
    insert_class_bridges(ast, &mut gen_body, &lift_ctx.local_classes);

    (gen_body, lifted_locals)
}

/// After the rewrite: right after every generator-local ClassDecl,
/// store the class value into its state-machine field
/// (`this.C = C`). Inserted post-rewrite so the rhs keeps the BARE
/// name — the capturing lane's same-list α-rename picks it up
/// (`__cc<N>_C`), exactly like the `new C()` sites. Only Vec<Stmt>
/// positions can host a ClassDecl (a declaration needs a block), so
/// the single-stmt control-flow bodies need no walk of their own.
fn insert_class_bridges(
    ast: &mut Ast,
    body: &mut Vec<Stmt>,
    local_classes: &std::collections::HashSet<String>,
) {
    let mut i = 0;
    while i < body.len() {
        let cname = match &body[i] {
            Stmt::ClassDecl { name, .. } if local_classes.contains(name) => Some(name.clone()),
            _ => None,
        };
        if let Some(cname) = cname {
            let this_e = ast.add_expr(Expr::This);
            let target = ast.add_expr(Expr::Member {
                obj: this_e,
                name: cname.clone(),
            });
            let value = ast.add_expr(Expr::Ident(cname));
            let assign = ast.add_expr(Expr::Assign { target, value });
            body.insert(i + 1, Stmt::Expr(assign));
            i += 2;
            continue;
        }
        match &mut body[i] {
            Stmt::Multi(list) | Stmt::Block(list) => insert_class_bridges(ast, list, local_classes),
            Stmt::Try {
                body: b,
                catch_body,
                finally_body,
                ..
            } => {
                insert_class_bridges(ast, b, local_classes);
                insert_class_bridges(ast, catch_body, local_classes);
                if let Some(f) = finally_body {
                    insert_class_bridges(ast, f, local_classes);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                bridge_in_boxed(ast, then_branch, local_classes);
                if let Some(e) = else_branch {
                    bridge_in_boxed(ast, e, local_classes);
                }
            }
            Stmt::While { body: b, .. }
            | Stmt::DoWhile { body: b, .. }
            | Stmt::Labeled { body: b, .. }
            | Stmt::For { body: b, .. }
            | Stmt::ForOf { body: b, .. } => bridge_in_boxed(ast, b, local_classes),
            _ => {}
        }
        i += 1;
    }
}

/// Boxed single-stmt control-flow bodies: a declaration needs a
/// block, so only a Block/Multi wrapper can host a ClassDecl there.
fn bridge_in_boxed(ast: &mut Ast, s: &mut Stmt, local_classes: &std::collections::HashSet<String>) {
    if let Stmt::Multi(list) | Stmt::Block(list) = s {
        insert_class_bridges(ast, list, local_classes);
    }
}

/// Collect the names of every class DECLARED inside the generator
/// body (any nesting depth — control flow included). Fed to
/// `LiftCtx::local_classes` so the field-ann sniff can refuse to
/// annotate a lifted local with a class name that will never reach
/// the top-level class index (the capturing lane α-renames it).
fn collect_local_classes(body: &[Stmt], out: &mut std::collections::HashSet<String>) {
    for s in body {
        match s {
            Stmt::ClassDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Multi(list) | Stmt::Block(list) => collect_local_classes(list, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_local_classes(core::slice::from_ref(then_branch.as_ref()), out);
                if let Some(e) = else_branch {
                    collect_local_classes(core::slice::from_ref(e.as_ref()), out);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Labeled { body, .. }
            | Stmt::For { body, .. }
            | Stmt::ForOf { body, .. } => {
                collect_local_classes(core::slice::from_ref(body.as_ref()), out);
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_local_classes(body, out);
                collect_local_classes(catch_body, out);
                if let Some(f) = finally_body {
                    collect_local_classes(f, out);
                }
            }
            _ => {}
        }
    }
}
