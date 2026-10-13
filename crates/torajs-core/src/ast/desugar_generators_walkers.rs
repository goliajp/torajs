//! Generator body walker: let-lift. Extracted from
//! `ast/desugar_generators.rs` when the RFC 20260713 blade work pushed
//! it past the 500-line HARD limit; the yield-into expansion that
//! shared this file lives in `desugar_generators_yield_into`.

use super::{Ast, Expr, Param, Stmt};

mod for_let_cell;
mod local_ann;
mod var_lift;

pub(crate) use var_lift::var_slots;

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
    /// Names the generator's own fields already own before any `let`
    /// lifts — its params and its own name (rotation 636). A block
    /// `let` of one of these is a distinct binding (§14.3.1), and
    /// lifting it under the same field name made every read of the
    /// param / self-name outside the block read the block's value;
    /// the duplicate-let rename (J.2.b) now sees them as taken.
    pub(crate) reserved: Vec<(String, String)>,
    pub(crate) fn_sigs: &'a std::collections::HashMap<String, String>,
    pub(crate) binds: std::collections::HashMap<String, String>,
    /// Classes DECLARED inside this generator's body. A `new C()`
    /// init whose class is one of these cannot annotate the lifted
    /// field as `C`: the class never reaches the top-level class
    /// index (the capturing lane α-renames it to `__cc<N>_C` and its
    /// name stops being a type name), so the annotation would be an
    /// `unknown type` at check and an `__tvdefault__C` seed in the
    /// factory. `any` is the honest slot type — it matches the
    /// `const __cc<N>_C: any` value the ES5 lane itself produces.
    pub(crate) local_classes: std::collections::HashSet<String>,
    /// The `__Gen_<name>` class the lifted fields land on — the key
    /// `Ast::const_lifted_fields` records a `const` / `using` lift
    /// under (RFC 20260915 #8).
    pub(crate) machine_class: String,
    /// Every `var` name of the body — the function-scoped bindings
    /// `var_lift` gives one field each — and whether that field may
    /// take the initializer's type (`var_lift::var_slots`).
    pub(crate) vars: std::collections::HashMap<String, bool>,
    /// Locals `for_let_cell` placed right before the statement that
    /// reads them: they live within one state arm and stay locals.
    pub(crate) arm_locals: std::collections::HashSet<String>,
    /// How many loop bodies the lift is inside: a statement list there
    /// may run more than once, each time with fresh block bindings.
    pub(crate) loop_depth: usize,
    /// Block bindings `for_let_cell` gave a per-pass local so far,
    /// which keeps the names of those locals apart.
    pub(crate) cells: usize,
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
        let taken = taken_names(ctx, lifted);
        super::desugar_generators_alpha::resolve_duplicate_let(ast, &mut list[i..], &taken);
        lift_lets_in_stmt(ast, &mut list[i], lifted, ctx);
    }
    if ctx.loop_depth > 0 {
        for_let_cell::box_captured_block_lets(ast, list, lifted, ctx);
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
    let taken = taken_names(ctx, lifted);
    super::desugar_generators_alpha::resolve_duplicate_for_let(ast, s, &taken);
    let taken = taken_names(ctx, lifted);
    for_let_cell::box_captured_for_let(ast, s, &taken, ctx);
    match s {
        Stmt::LetDecl { is_var: true, .. } => var_lift::lift_var(ast, s, lifted, ctx),
        Stmt::LetDecl { name, .. } if ctx.arm_locals.contains(name) => {}
        Stmt::LetDecl {
            mutable,
            name,
            type_ann,
            init,
            ..
        } => {
            let n = name.clone();
            let t = local_ann::local_ann(ast, &n, type_ann.as_ref(), *init, ctx);
            ctx.binds.insert(n.clone(), t.clone());
            lifted.push((n.clone(), t));
            let this_id = ast.add_expr(Expr::This);
            let m = ast.add_expr(Expr::Member {
                obj: this_id,
                name: n.clone(),
            });
            let assign = ast.add_expr(Expr::Assign {
                target: m,
                value: *init,
            });
            // RFC 20260915 #8 — the slot is writable; the binding was
            // not. Record the field and admit exactly this store.
            if !*mutable {
                ast.const_lifted_fields
                    .entry(ctx.machine_class.clone())
                    .or_default()
                    .insert(n);
            }
            ast.lifted_init_assigns.insert(assign);
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
            ctx.loop_depth += 1;
            lift_lets_in_stmt(ast, body, lifted, ctx);
            ctx.loop_depth -= 1;
        }
        Stmt::Labeled { body, .. } => {
            lift_lets_in_stmt(ast, body, lifted, ctx);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                lift_lets_in_stmt(ast, i, lifted, ctx);
            }
            ctx.loop_depth += 1;
            lift_lets_in_stmt(ast, body, lifted, ctx);
            ctx.loop_depth -= 1;
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
            } else {
                for s in body
                    .iter_mut()
                    .chain(catch_body.iter_mut())
                    .chain(finally_body.iter_mut().flatten())
                {
                    var_lift::lift_vars_only(ast, s, lifted, ctx);
                }
            }
        }
        Stmt::Switch {
            cases,
            default,
            default_at,
            ..
        } => lift_lets_in_switch(ast, cases, default, *default_at, lifted, ctx),
        // An inline loop (the yield-bearing ones were rewritten to the
        // manual protocol before the lift): its `let`s stay locals.
        Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => {
            var_lift::lift_vars_only(ast, body, lifted, ctx);
        }
        _ => {}
    }
}

/// A switch's case block is ONE scope (§14.12.2 — a `let` in one
/// clause is visible, in its TDZ, from the clauses after it), so a
/// yield-bearing switch lifts its clauses as one list in source order,
/// then splits them back. A yield-free switch keeps its lets as plain
/// locals, as a yield-free try does.
fn lift_lets_in_switch(
    ast: &mut Ast,
    cases: &mut [super::SwitchCase],
    default: &mut Option<Vec<Stmt>>,
    default_at: usize,
    lifted: &mut Vec<(String, String)>,
    ctx: &mut LiftCtx,
) {
    let has_yield = cases
        .iter()
        .flat_map(|c| c.body.iter())
        .chain(default.iter().flatten())
        .any(super::desugar_generators_sm_rewrite::stmt_contains_yield);
    if !has_yield {
        for s in cases
            .iter_mut()
            .flat_map(|c| c.body.iter_mut())
            .chain(default.iter_mut().flatten())
        {
            var_lift::lift_vars_only(ast, s, lifted, ctx);
        }
        return;
    }
    let (before, after) = cases.split_at_mut(default_at);
    let mut clauses: Vec<&mut Vec<Stmt>> = before.iter_mut().map(|c| &mut c.body).collect();
    clauses.extend(default.as_mut());
    clauses.extend(after.iter_mut().map(|c| &mut c.body));
    let lens: Vec<usize> = clauses.iter().map(|b| b.len()).collect();
    let mut list: Vec<Stmt> = clauses.iter_mut().flat_map(|b| b.drain(..)).collect();
    lift_lets_in_list(ast, &mut list, lifted, ctx);
    let mut rest = list.into_iter();
    for (body, n) in clauses.into_iter().zip(lens) {
        body.extend(rest.by_ref().take(n));
    }
}

/// What a `let` about to lift may not be called: the fields the
/// generator owns from the start (`LiftCtx::reserved`), the body's
/// `var` names, and what the lift has claimed so far.
fn taken_names(ctx: &LiftCtx, lifted: &[(String, String)]) -> Vec<(String, String)> {
    ctx.reserved
        .iter()
        .cloned()
        .chain(ctx.vars.keys().map(|n| (n.clone(), String::new())))
        .chain(lifted.iter().cloned())
        .collect()
}
