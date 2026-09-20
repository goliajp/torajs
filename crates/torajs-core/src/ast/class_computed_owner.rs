//! RFC 20260921 — "which ClassDecl declared this computed member".
//!
//! The `class_computed_*` side tables are keyed by class NAME, and a
//! name is not an identity: two functions may each declare `class A`.
//! Asking those tables by name answers for the whole name group, which
//! is how a plain `static plain = 2` on one `class A` went missing
//! because a same-named sibling happened to carry `static [k] = 1` —
//! both the unit lane's admit and the hoist lane's "has observable
//! statics" probe refused the wrong class.
//!
//! Every query that means "does THIS declaration have one" belongs
//! here and takes a `decl_id`. Queries that genuinely mean "anything
//! under this name" (a cross-module rename walking rows to re-key
//! them) keep asking by name and do not belong here.

use super::ast_def::Ast;

/// Does `decl_id` declare the computed member parked under `sentinel`?
/// A `BodyCloner` clone shares its source's sentinels, so a sentinel
/// may have more than one owner.
pub(crate) fn owns(ast: &Ast, sentinel: &str, decl_id: u32) -> bool {
    ast.class_computed_owner
        .get(sentinel)
        .is_some_and(|ids| ids.contains(&decl_id))
}

/// Does `decl_id` declare a computed-key STATIC FIELD?
pub(crate) fn has_static_field(ast: &Ast, decl_id: u32) -> bool {
    ast.class_computed_static_fields
        .iter()
        .any(|(_, sent, _)| owns(ast, sent, decl_id))
}

/// Does `decl_id` declare a computed-key member of any kind — method,
/// accessor, instance field or static field?
pub(crate) fn has_any(ast: &Ast, decl_id: u32) -> bool {
    ast.class_computed_keys
        .keys()
        .any(|(_, sent)| owns(ast, sent, decl_id))
}
