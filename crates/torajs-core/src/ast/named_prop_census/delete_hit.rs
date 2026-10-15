//! What one property delete of the program reaches.
//!
//! A declared field has no slot to give up, so `delete` (§10.1.10)
//! records the removal in the field's sidecar behind
//! `FLAG_OBJ_EXOTIC_FIELD`, and a later [[Get]] answers `undefined`
//! (or the prototype's value) where the slot still holds the old one.
//! For the reads that is a write of a value the slot may not spell: a
//! delete reaches its names as a write of `Unknown` kind does, so the
//! typed reads of a deleted field test the bit and the checker reads
//! the member as `any`.

use super::write_hit::{Hit, Kind, write};
use crate::ast::{Ast, Expr, ExprId};

/// The names `delete target` reaches.
pub(super) fn hit(facts: &super::write_kind::Facts<'_, '_>, target: ExprId) -> Hit {
    match facts.ast.get_expr(target) {
        Expr::Member { name, .. } => Hit::Name(name.clone(), Kind::Unknown),
        Expr::Index { .. } => match write(facts, target, None) {
            Hit::Name(n, _) => Hit::Name(n, Kind::Unknown),
            Hit::All(_) => Hit::All(Kind::Unknown),
            h => h,
        },
        _ => Hit::None,
    }
}

/// The names `Reflect.deleteProperty(o, key)` reaches.
pub(super) fn reflect(ast: &Ast, args: &[ExprId]) -> Hit {
    match args.get(1).map(|k| ast.get_expr(*k)) {
        Some(Expr::String(s)) => match s.as_str() {
            Some(s) => Hit::Name(s.to_string(), Kind::Unknown),
            None => Hit::All(Kind::Unknown),
        },
        _ => Hit::All(Kind::Unknown),
    }
}

/// A callee spelled `Reflect.set` or `Reflect.deleteProperty`, whose
/// first argument is the receiver it writes.
pub(super) fn reflect_write(ast: &Ast, callee: ExprId) -> bool {
    matches!(ast.get_expr(callee),
        Expr::Member { obj, name } if matches!(name.as_str(), "set" | "deleteProperty")
            && matches!(ast.get_expr(*obj), Expr::Ident(r) if r == "Reflect"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::shadow_names::{ShadowedNames, SlotKind};
    use crate::ast::{Expr, ExprId};
    use crate::check::Type;

    /// What the program's deletes leave mistyped, with `o` typed `t`.
    fn mistyped(src: &str, t: Type) -> ShadowedNames {
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        let ast = crate::parser::parse(src, &tokens).expect("parse");
        let types: HashMap<ExprId, Type> = (ast.exprs.iter().enumerate())
            .filter(|(_, e)| matches!(e, Expr::Ident(n) if n == "o"))
            .map(|(i, _)| (ExprId(i as u32), t.clone()))
            .collect();
        ShadowedNames::of(&ast, &types)
    }

    #[test]
    fn a_delete_retypes_the_reads_of_the_name_it_spells() {
        let n = mistyped(r#"delete o.b; Reflect.deleteProperty(o, "c");"#, Type::Any);
        assert!(n.may_mistype("b", SlotKind::Number) && n.may_mistype("b", SlotKind::String));
        assert!(n.may_mistype("c", SlotKind::Number));
        assert!(!n.may_mistype("a", SlotKind::Number));
        let n = mistyped(r#"delete o["d"];"#, Type::Any);
        assert!(n.may_mistype("d", SlotKind::Boolean));
        let fs = Type::struct_of(vec![(crate::ast::PropKey::from("e"), Type::Number)]);
        assert!(mistyped("delete o.e;", fs).may_mistype("e", SlotKind::Number));
    }
}
