//! Which object fields can hold the F64 `undefined` sentinel.
//!
//! A `number[]` read past the end answers the sentinel
//! ([`crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS`]),
//! and every consumer that has to tell `undefined` from a number
//! asks [`crate::ssa_lower_undef_f64_source::is_undef_f64_source`]
//! whether the expression in front of it is one. A field read was
//! not on that list, so storing an out-of-range read into a field
//! and reading it back lost the answer:
//!
//! ```ignore
//! const r = { v: zs[9] };
//! r.v            // tr NaN / bun undefined
//! typeof r.v     // tr number / bun undefined
//! ```
//!
//! Unlike the pointer-shaped families, a `number` field carries no
//! type-level tell: it is seeded with 0, so the declared type says
//! nothing about whether `undefined` ever reached it. Answering
//! "yes" for every `number` field would be sound — the consumers
//! all re-check the bits — but it would arm the ToNumber step in
//! [`crate::ssa_lower_f64_sentinel_canon`] for arithmetic on any
//! field, and that cost belongs only to programs that actually put
//! an out-of-range read in one. So the set is collected instead:
//! the field names some write hands a sentinel-shaped value.
//!
//! Collected against the real predicate rather than a copy of it,
//! before the body is lowered, when `is_undef_f64_source` answers on
//! the syntactic shapes alone.
//!
//! Assignments to a plain binding are collected the same way and for
//! the same reason: `a = zs[9]` records nothing at the let-decl,
//! since there is none, and a read of `a` therefore answered `NaN`.
//! It cannot be recorded at the assignment's own lowering either —
//! inside a loop a read lowers before the assignment that taints it,
//! and would be answered with the previous iteration's ignorance.
//!
//! Declarations are collected here too, even though
//! [`crate::ssa_lower_stmt_let_decl_general`] already records one at
//! its own lowering. That record is in time for a later read of the
//! binding itself and too late for everything the fixpoint below
//! decides, which runs first: `const u = zs[9]; r.v = u` left `v`
//! out of the field set, because when the set was built `u` was not
//! yet known to be a source, and `r.v` therefore answered `NaN`.
//! `let` and `const` are statements, so they are invisible to the
//! two collectors that scan the expression arena — hence the walk.
//!
//! The two feed each other (`const u = zs[9]; r.v = u` needs `u`
//! first; `a = r.v` needs the field first), so they run together to
//! a fixpoint rather than once each. Both sets only grow and the
//! names are finite, so it terminates.
//!
//! Over-broad by name and across bodies, deliberately: a same-named
//! binding elsewhere costs one well-predicted bits compare, while
//! missing one answers `NaN` where the program should see
//! `undefined`.

use crate::ast::Expr;
use crate::ast::PropKey;
use crate::ssa_lower::LowerCtx;
use std::collections::HashSet;

/// The three name sets [`prime`] settles.
pub(crate) struct SentinelSets {
    f64_fields: HashSet<PropKey>,
    str_fields: HashSet<PropKey>,
    f64_lets: HashSet<String>,
}

/// Fill both sentinel sets on `ctx`, to a fixpoint. Run after the
/// context exists and before its body is lowered.
///
/// The fixpoint is settled once per module and copied into every
/// later body. What makes the answer the same for every body: the
/// collectors walk the whole program's arena and statement list, not
/// this body's, and at this point every per-body input they and the
/// two predicates read is still in the state a fresh context starts
/// in. The three sets themselves, `nullable_str_lets`,
/// `nullable_arr_lets`, `locals` and `num_map_slots` are empty — the
/// only priming that runs earlier, `prime_body_binding_sets`, fills
/// none of them — and `proofs` holds nothing but the module's facts.
/// Everything else they read (`ast`, `expr_types`, `globals`,
/// `num_f64_slots`, the facts' live census) is one module-wide value
/// that no body's lowering changes. Equal inputs, equal fixpoint; a
/// new per-body input to either predicate breaks this.
pub(crate) fn prime(ctx: &mut LowerCtx<'_>) {
    let scans = ctx.scans;
    if let Some(sets) = scans.sentinel_sets.get() {
        ctx.undefable_f64_fields = sets.f64_fields.clone();
        ctx.nullable_str_fields = sets.str_fields.clone();
        ctx.undefable_f64_lets = sets.f64_lets.clone();
        return;
    }
    settle(ctx);
    let _ = scans.sentinel_sets.set(SentinelSets {
        f64_fields: ctx.undefable_f64_fields.clone(),
        str_fields: ctx.nullable_str_fields.clone(),
        f64_lets: ctx.undefable_f64_lets.clone(),
    });
}

fn settle(ctx: &mut LowerCtx<'_>) {
    // Seed from the analysis's own answer before walking. The walk
    // below can only see what THIS context's bindings hold, so a
    // field written from a tainted parameter (`this.v = v` in a
    // constructor, `{ v: v }` in a factory) was invisible to it: the
    // parameter belongs to the function being called, and the read
    // is lowered in the caller. The analysis table is by name and
    // program-wide, so it knows; it just had no way to say so.
    let seeded: Vec<_> = ctx
        .num_f64_slots
        .sentinel_fields()
        .iter()
        .cloned()
        .collect();
    ctx.undefable_f64_fields.extend(seeded);
    loop {
        let fields = collect(ctx, crate::ssa_lower_undef_f64_source::is_undef_f64_source);
        // The Str sentinel's field set, collected by the same walk
        // against ITS predicate (rotation 640): a generator step's
        // `value: string` completion is the `__undef_slot__string`
        // marker, and `typeof step.value` answered "string" for it.
        let mut str_fields = collect(ctx, crate::ssa_lower_nullable_guard::is_nullable_str_source);
        str_fields.extend(collect_any_nullish_writes(ctx));
        let mut lets = collect_assigned_idents(ctx);
        lets.extend(collect_declared_idents(ctx));
        let grew = !fields.is_subset(&ctx.undefable_f64_fields)
            || !str_fields.is_subset(&ctx.nullable_str_fields)
            || !lets.is_subset(&ctx.undefable_f64_lets);
        ctx.undefable_f64_fields.extend(fields);
        ctx.nullable_str_fields.extend(str_fields);
        ctx.undefable_f64_lets.extend(lets);
        if !grew {
            return;
        }
    }
}

/// Binding names some assignment hands a value the sentinel gate
/// recognises. Arena order, so the result does not depend on how
/// anything is hashed.
fn collect_assigned_idents(ctx: &LowerCtx<'_>) -> HashSet<String> {
    let mut out = HashSet::new();
    for id in 0..ctx.ast.exprs.len() {
        let eid = crate::ast::ExprId(id as u32);
        if let Expr::Assign { target, value } = ctx.ast.get_expr(eid)
            && let Expr::Ident(name) = ctx.ast.get_expr(*target)
            && crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, *value)
        {
            out.insert(name.clone());
        }
    }
    out
}

/// Binding names some `let` / `const` initialises with a value the
/// sentinel gate recognises. Statement order, so the result does not
/// depend on how anything is hashed.
fn collect_declared_idents(ctx: &LowerCtx<'_>) -> HashSet<String> {
    let mut out = HashSet::new();
    crate::ast::walk_stmts(&ctx.ast.stmts, &mut |s| {
        if let crate::ast::Stmt::LetDecl { name, init, .. } = s
            && crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, *init)
        {
            out.insert(name.clone());
        }
    });
    out
}

/// Field names that some object literal or member assignment fills
/// with a value the sentinel gate recognises. Arena order, so the
/// result does not depend on how anything is hashed.
fn collect(
    ctx: &LowerCtx<'_>,
    is_source: fn(&LowerCtx<'_>, crate::ast::ExprId) -> bool,
) -> HashSet<PropKey> {
    let mut out = HashSet::new();
    for id in 0..ctx.ast.exprs.len() {
        let eid = crate::ast::ExprId(id as u32);
        match ctx.ast.get_expr(eid) {
            Expr::ObjectLit { fields } => {
                for (name, value) in fields {
                    if is_source(ctx, *value) {
                        out.insert(name.clone());
                    }
                }
            }
            Expr::Assign { target, value } => {
                if let Expr::Member { name, .. } = ctx.ast.get_expr(*target)
                    && is_source(ctx, *value)
                {
                    out.insert(PropKey::from(name));
                }
            }
            _ => {}
        }
    }
    out
}

/// Field names an `any` receiver is handed null / undefined (or a
/// value of unknown type) for. The runtime store puts NULL or the Str
/// sentinel straight into a `string` slot of the struct behind it, so
/// a typed read of that field has to stay identity-aware.
fn collect_any_nullish_writes(ctx: &LowerCtx<'_>) -> HashSet<PropKey> {
    use crate::check::Type;
    let mut out = HashSet::new();
    for id in 0..ctx.ast.exprs.len() {
        let eid = crate::ast::ExprId(id as u32);
        if let Expr::Assign { target, value } = ctx.ast.get_expr(eid)
            && let Expr::Member { obj, name } = ctx.ast.get_expr(*target)
            && matches!(ctx.expr_types.get(obj), Some(Type::Any))
            && matches!(
                ctx.expr_types.get(value),
                Some(Type::Null | Type::Undefined | Type::Any) | None
            )
        {
            out.insert(PropKey::from(name));
        }
    }
    out
}
