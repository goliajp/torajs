//! Which parameters of the function being lowered may hold their
//! width's `undefined` sentinel — seeded once per body, right after
//! the params are materialized, into the same registries a let-decl
//! of that shape fills (`ssa_lower_fn` step 5b).

use crate::ast;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// Record the parameters that a call site hands an answer meaning
/// `undefined`, so the consumers in this body know to check.
pub(crate) fn seed_undef_sentinel_params(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    params: &[ast::Param],
) {
    // The replace lane's view twin takes a capture group's view, and
    // a group that did not participate is the Substr `undefined`
    // sentinel (§22.2.6.11 step 14.g); no call site in the program
    // spells that, so the twin names its view parameters itself.
    let view_twin = ctx.ast.is_replace_view_fn(name);
    // A parameter some call site hands an out-of-range read (or a
    // `find` miss, or a `pop` off an empty array) carries that
    // answer's sentinel, exactly like a binding initialized from the
    // same shape. A binding gets recorded at its let-decl; a
    // parameter's value arrives from a caller lowered separately, so
    // without this the consumers in this body read the sentinel as a
    // plain number: `h(xs[7])` printed NaN where `console.log(xs[7])`
    // printed `undefined`.
    //
    // Which set to record it in follows the slot, because each family
    // spells the answer its own way and each has its own consumers
    // reading its own set. The collector itself is shape-only, so it
    // has always named these parameters; only `number` was being
    // told. `ts(ss[7])` answered "string" and `td(ds[7])` "object"
    // for want of these three lines.
    for p in params {
        // A WRITTEN `T | null` / `T | undefined` says the same thing
        // the call-site collector infers, and says it directly — the
        // annotation is the one piece of evidence neither this gate
        // nor the let-decl twin was reading. Without it `typeof s` on
        // `function k(s: string | null)` answered "string" for a null.
        let annotated_nullable = p
            .type_ann
            .as_deref()
            .is_some_and(|a| a.starts_with("__nullable("));
        let slot_ty = ctx.locals.get(&p.name).map(|l| l.ty);
        if view_twin && slot_ty == Some(Type::Substr) {
            ctx.undefable_substr_lets.insert(p.name.clone());
            continue;
        }
        if !annotated_nullable && !ctx.num_f64_slots.param_takes_undef_sentinel(name, &p.name) {
            continue;
        }
        match slot_ty {
            Some(Type::Str) => {
                ctx.nullable_str_lets.insert(p.name.clone());
            }
            Some(Type::Substr) => {
                ctx.undefable_substr_lets.insert(p.name.clone());
            }
            Some(t) if t.spells_undef_with_generic_cell() => {
                ctx.undefable_heap_lets.insert(p.name.clone());
            }
            // F64 stays the default rather than an arm of its own: a
            // slot type that is not on this list costs nothing by
            // being in the number set, since every consumer of it
            // gates on an F64 operand as well.
            _ => {
                ctx.undefable_f64_lets.insert(p.name.clone());
            }
        }
    }
}
