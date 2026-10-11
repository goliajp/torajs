//! The fields an object literal leaves out of the declared type it is
//! recorded with: the optional members it does not spell.

use crate::ast::ExprId;
use crate::check::Type;

/// Every member of the literal is a plain `name: value` one: no
/// spread, accessor or private name.
pub(super) fn plain_entries(entries: &[(crate::ast::PropKey, ExprId)]) -> bool {
    entries.iter().all(|(n, _)| {
        crate::check_type_of_object_lit::spread_omit_set(n).is_none()
            && crate::check_type_of_object_lit::accessor_slot(n).is_none()
            && !crate::check_type_of_object_lit::private_slot(n)
    })
}

/// The fields of `recorded` (the type the checker recorded for an
/// object literal) the literal's `entries` leave out, with their
/// declared index: what `reorder_object` admitted as absent. Empty
/// for any other literal.
pub(crate) fn literal_absent_fields(
    recorded: &Type,
    entries: &[(crate::ast::PropKey, ExprId)],
) -> Vec<(usize, crate::ast::PropKey)> {
    let Type::Struct(declared) = recorded else {
        return Vec::new();
    };
    // a repeated key (`{ b: 1, b: 2 }`) is one property
    let spelled = entries
        .iter()
        .enumerate()
        .filter(|(i, (n, _))| !entries[..*i].iter().any(|(m, _)| m == n))
        .count();
    if declared.len() <= spelled || !plain_entries(entries) {
        return Vec::new();
    }
    declared
        .iter()
        .enumerate()
        .filter(|(_, (n, _))| !entries.iter().any(|(m, _)| m == n))
        .map(|(i, (n, _))| (i, n.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    fn check_src(src: &str) -> Result<(), String> {
        let tokens = crate::lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
        let mut ast = crate::parser::parse(src, &tokens).map_err(|e| format!("parse: {e}"))?;
        crate::ast::lift_arrow_fns(&mut ast);
        crate::check::check(&ast).map(|_| ())
    }

    fn accepted(src: &str) {
        assert!(check_src(src).is_ok(), "{src}: {:?}", check_src(src));
    }

    fn refused(src: &str) {
        assert!(check_src(src).is_err(), "{src} must be refused");
    }

    #[test]
    fn an_optional_member_may_be_left_out_on_every_spelling() {
        accepted("const p: { a?: number; b: number } = { b: 1 };\n");
        accepted("type O = { a?: number; b: number };\nconst p: O = { b: 1 };\n");
        accepted("interface I { a?: number; b: number }\nconst p: I = { b: 1 };\n");
        accepted("type G<T> = { v?: T; b: number };\nconst g: G<string> = { b: 1 };\n");
        accepted("type N = { i: { s?: string; n: number } };\nconst n: N = { i: { n: 1 } };\n");
        accepted("const z: { z?: number | null; k: number } = { k: 1 };\n");
        accepted(
            "function f(p: { a?: number; b: number }): number { return p.b; }\nf({ b: 1 });\n",
        );
    }

    #[test]
    fn a_required_member_admitting_nullish_may_not_be_left_out() {
        // TS2741: only the `?` makes a member optional
        refused("const p: { a: number | null; b: number } = { b: 1 };\n");
        refused("const p: { a: number | undefined; b: number } = { b: 1 };\n");
        refused("const p: { a: any; b: number } = { b: 1 };\n");
        refused("type R = { a: string | null; b: number };\nconst p: R = { b: 1 };\n");
        refused("interface I { a: number | null; b: number }\nconst p: I = { b: 1 };\n");
        refused("type G<T> = { v: T | null; b: number };\nconst g: G<string> = { b: 1 };\n");
        refused(
            "function f(p: { a: number | null; b: number }): number { return p.b; }\nf({ b: 1 });\n",
        );
    }
}
