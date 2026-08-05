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
//!    boundaries. Same-name lets in different scopes both map to
//!    the same field and would clobber; we panic on collision so
//!    the user has to rename (Phase J.2.b limitation).
//! 3. **params + locals rewrite** — every Ident matching a
//!    generator param or lifted local rewrites to `this.<name>` so
//!    the iterator class's field path picks it up.
//!
//! Returns `(prepared_body, lifted_locals)` so the caller can pass
//! both into the class-assembly step.

use super::desugar_generators_walkers::LiftCtx;
use super::{Ast, Param, Stmt, expand_yield_into_in_stmt, lift_lets_in_stmt};

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
    let mut lift_ctx = LiftCtx {
        params: gen_params,
        fn_sigs,
        binds: std::collections::HashMap::new(),
    };
    for s in &mut gen_body {
        lift_lets_in_stmt(ast, s, &mut lifted_locals, &mut lift_ctx);
    }
    for i in 0..lifted_locals.len() {
        for j in (i + 1)..lifted_locals.len() {
            if lifted_locals[i].0 == lifted_locals[j].0 {
                panic!(
                    "function* {gen_name}: duplicate `let {}` declarations across \
                     scopes — both lift to `this.{}` and would collide. Rename \
                     one (Phase J.2.b limitation).",
                    lifted_locals[i].0, lifted_locals[i].0
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

    (gen_body, lifted_locals)
}
