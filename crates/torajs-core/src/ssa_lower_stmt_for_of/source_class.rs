//! The class a for-of source names.

use crate::ssa_lower::LowerCtx;

/// The class a for-of SOURCE names, from the checker's own verdict.
///
/// RFC 20260715-nominal-class-identity — a class instance NAMES its
/// class. This used to scan the alias table for any entry whose
/// `Type::Obj` StructId matched the source's, which is structural:
/// two classes with the same field shape share one StructId, so the
/// scan picked whichever the HashMap happened to yield first. That
/// called a DIFFERENT class's `[Symbol.iterator]`, and since HashMap
/// order is not stable the same program compiled to a different
/// iterator between runs. The checker was moved off the structural
/// fallback for exactly this reason; its `ClassRef` verdict is what
/// the emit should have been reading all along.
pub(super) fn src_class_name(ctx: &LowerCtx, src_ref_eid: crate::ast::ExprId) -> Option<String> {
    match ctx.expr_types.get(&src_ref_eid) {
        Some(crate::check::Type::ClassRef(n)) if ctx.ast.class_parents.contains_key(n) => {
            Some(n.clone())
        }
        _ => None,
    }
}
