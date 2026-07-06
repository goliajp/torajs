//! RC-4 F1c — `Object.defineProperty` receiver collection.
//!
//! `Object.defineProperty(obj, ...)` on a struct-typed binding
//! converts the runtime cell to a DynObj, but the dynobj write-back
//! only fires for `Type::Any` bindings (`emit_any_dynobj_writeback`)
//! — a statically-typed receiver kept pointing at the old struct
//! cell and the defined property landed on an orphan. The fix is to
//! type such bindings as `any` from the start (the P3.2 dynobj-init
//! lane), which needs the receiver names known before type-checking.
//!
//! Flat arena scan — every expression node lives in `ast.exprs`
//! regardless of nesting, so no recursive walker is needed. The
//! collection is name-based and module-flat (same linear-parse-order
//! contract as P8.5 class-value aliases): a shadowing same-named
//! binding elsewhere also degrades — conservative, an over-broad
//! `any` is slower but never wrong.
//!
//! Consumed by both `check_pipeline` (binding type) and
//! `ssa_lower_fn` (init-lane routing) so the two sides can't drift.

use std::collections::HashSet;

use crate::ast::{Ast, Expr};

/// Collect binding names that appear as the receiver (`args[0]`
/// Ident) of an `Object.defineProperty` / `Object.defineProperties` /
/// `Reflect.defineProperty` call anywhere in the module.
pub(crate) fn collect_defineproperty_receivers(ast: &Ast) -> HashSet<String> {
    let mut out = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        if !matches!(name.as_str(), "defineProperty" | "defineProperties") {
            continue;
        }
        let Expr::Ident(ns) = ast.get_expr(*obj) else {
            continue;
        };
        if ns != "Object" && ns != "Reflect" {
            continue;
        }
        let Some(first) = args.first() else {
            continue;
        };
        if let Expr::Ident(recv) = ast.get_expr(*first) {
            out.insert(recv.clone());
        }
    }
    out
}
