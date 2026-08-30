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
//! Collected against the real predicate rather than a copy of it —
//! run once after the context exists and before its body is
//! lowered, when `is_undef_f64_source` answers on the syntactic
//! shapes alone. A binding written in between (`const u = zs[9];
//! r.v = u`) is therefore not seen; that field reads back as `NaN`,
//! which is the answer it gave before this existed rather than a
//! new wrong one.

use crate::ast::Expr;
use crate::ssa_lower::LowerCtx;
use std::collections::HashSet;

/// Field names that some object literal or member assignment fills
/// with a value the sentinel gate recognises. Arena order, so the
/// result does not depend on how anything is hashed.
pub(crate) fn collect(ctx: &LowerCtx<'_>) -> HashSet<String> {
    let mut out = HashSet::new();
    for id in 0..ctx.ast.exprs.len() {
        let eid = crate::ast::ExprId(id as u32);
        match ctx.ast.get_expr(eid) {
            Expr::ObjectLit { fields } => {
                for (name, value) in fields {
                    if crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, *value) {
                        out.insert(name.clone());
                    }
                }
            }
            Expr::Assign { target, value } => {
                if let Expr::Member { name, .. } = ctx.ast.get_expr(*target) {
                    if crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, *value) {
                        out.insert(name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    out
}
