//! Whether a slot call's target is settled statically.

use crate::ast::ExprId;
use crate::ssa_lower::LowerCtx;

/// The receiver's statically-settled slot target, or `None` to keep
/// the polymorphic vtable path. Settled means: the receiver's static
/// type is a ClassRef whose class has NO method-declaring descendant
/// (an instance can only wear that class or a descendant, and with
/// none of them re-declaring the method, every dynamic type resolves
/// the slot to the same impl). Resolution mirrors `populate_vtables`:
/// the mono spelling (the call's retarget suffix) first, then bare,
/// up the ancestor chain.
pub(super) fn devirt_target(
    ctx: &LowerCtx<'_>,
    recv: ExprId,
    method_name: &str,
    suffix: &str,
    owners: &[String],
) -> Option<crate::ssa::FuncId> {
    let crate::check::Type::ClassRef(key) = ctx.expr_types.get(&recv)? else {
        return None;
    };
    let base = key.split('<').next().unwrap_or(key.as_str());
    if !ctx.ast.class_parents.contains_key(base) {
        return None;
    }
    // Any declarer below the static class keeps the vtable path.
    if overridden_below(ctx, base, owners) {
        return None;
    }
    let mut cur = Some(base.to_string());
    let mut depth = 0u32;
    while let Some(name) = cur {
        if depth > 64 {
            break;
        }
        let suffixed = format!("__cm_{name}__{method_name}{suffix}");
        let bare = format!("__cm_{name}__{method_name}");
        if let Some(fid) = ctx
            .fn_table
            .get(&suffixed)
            .or_else(|| ctx.fn_table.get(&bare))
        {
            return Some(*fid);
        }
        if owners.contains(&name) {
            // This level DECLARES the method yet neither spelling
            // resolves — a generic declarer whose specialization
            // this call's suffix can't name (the dispatcher's
            // retarget suffix is the BASE's, not the receiver's).
            // Falling through to an ancestor would silently drop
            // the override; the vtable row (whose resolution rides
            // the factory's own suffix) knows better.
            return None;
        }
        cur = ctx.ast.class_parents.get(&name).and_then(|p| p.clone());
        depth += 1;
    }
    None
}

/// True iff some declarer in `owners` is a STRICT descendant of
/// `base` — an instance statically typed `base` may then be wearing
/// an override, so the call must read the slot.
pub(crate) fn overridden_below(ctx: &LowerCtx<'_>, base: &str, owners: &[String]) -> bool {
    owners.iter().any(|c| {
        if c == base {
            return false;
        }
        let mut cur = ctx.ast.class_parents.get(c).and_then(|p| p.clone());
        let mut depth = 0u32;
        while let Some(p) = cur {
            if depth > 64 {
                break;
            }
            if p == base {
                return true;
            }
            cur = ctx.ast.class_parents.get(&p).and_then(|q| q.clone());
            depth += 1;
        }
        false
    })
}
