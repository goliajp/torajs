//! Which names a store can rebind without releasing anything but the
//! old value itself.
//!
//! The checker types reads, not assignment targets, and a read's type
//! may be narrowed (`x = null; f(x)` reads `null`), so a store is judged
//! by the binding's DECLARED type: every `let` of the name in the frame,
//! the parameter slot, the module global. A name none of them declares,
//! or one declaration of which can hold a cell that reaches others,
//! counts as releasing.

use std::collections::{HashMap, HashSet};

use crate::ast::Stmt;
use crate::ast::stmt_nested_lists::for_each_nested_list;
use crate::check::Type as CT;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

pub(super) fn leaf_names(ctx: &LowerCtx<'_>, frame: &[&Stmt]) -> HashSet<String> {
    let mut seen: HashMap<String, bool> = HashMap::new();
    for s in frame {
        walk(ctx, s, &mut seen);
    }
    for (name, info) in ctx.locals.iter() {
        *seen.entry(name.clone()).or_insert(true) &= leaf_slot(info.ty);
    }
    for (name, ty) in ctx.globals.iter() {
        *seen.entry(name.clone()).or_insert(true) &= leaf_slot(*ty);
    }
    seen.into_iter()
        .filter(|(_, ok)| *ok)
        .map(|(n, _)| n)
        .collect()
}

fn leaf_slot(t: Type) -> bool {
    t.is_copy() || matches!(t, Type::Str | Type::Substr)
}

fn walk(ctx: &LowerCtx<'_>, s: &Stmt, seen: &mut HashMap<String, bool>) {
    let decl: Option<(&String, bool)> = match s {
        Stmt::FnDecl { name, .. } => {
            *seen.entry(name.clone()).or_insert(true) = false;
            return;
        }
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => Some((
            name,
            match type_ann {
                Some(a) => matches!(a.trim(), "number" | "boolean" | "string" | "bigint"),
                // a cross-family rebind widens the slot past its init
                None => {
                    !ctx.scans.cross_type_widened.contains(init)
                        && matches!(
                            ctx.expr_types.get(init),
                            Some(CT::Number | CT::Boolean | CT::String | CT::BigInt)
                        )
                }
            },
        )),
        Stmt::UsingDecl { name, .. } | Stmt::ForOfSplitIter { var_name: name, .. } => {
            Some((name, false))
        }
        Stmt::ForOf { var_name, .. } => Some((var_name, false)),
        Stmt::Try {
            catch_param: Some(p),
            ..
        } => Some((p, false)),
        _ => None,
    };
    if let Some((name, ok)) = decl {
        *seen.entry(name.clone()).or_insert(true) &= ok;
    }
    for_each_nested_list(s, &mut |l| {
        for st in l {
            walk(ctx, st, seen);
        }
    });
}
