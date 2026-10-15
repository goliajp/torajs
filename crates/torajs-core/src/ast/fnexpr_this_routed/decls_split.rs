//! The declaration census of [`super`], split out at the file-size
//! line.

use crate::ast::{ExprId, Stmt};

/// Every `LetDecl` of `name`, split by whether it sits inside a
/// receiver-polymorphic TWIN body.
///
/// `desugar_classes_generic_twin` clones each `this`-using method body
/// into `__cmany_<C>__<m>` (and each static one into `__smany_`). So a
/// `const` a program writes ONCE inside a method is DECLARED TWICE in
/// the statement list this census walks, and the `decls.len() != 1`
/// guard above read the copy as a same-name binding in another scope.
/// That is the whole reason `const g = function () { …this… }; g()`
/// answered the enclosing receiver when written at method scope and
/// `undefined` when written at top level — the same program, and the
/// second answer is the §10.2.1.2 one.
///
/// The two halves are deliberately not interchangeable: the guard
/// counts SOURCES (mono bodies only), while the promotion patches the
/// COPIES too. Promoting only the source would be the silent wrong the
/// zero-alias bar exists to prevent — the direct-call seeding is
/// name-keyed, so the twin's `g()` would push an `undefined` into an
/// argv slot its unpromoted closure never declared.
pub(super) fn collect_decls_split(
    stmts: &[Stmt],
    name: &str,
    in_twin: bool,
    mono: &mut Vec<(bool, ExprId)>,
    twin: &mut Vec<ExprId>,
) {
    for s in stmts {
        if let Stmt::LetDecl {
            mutable,
            name: dn,
            init,
            ..
        } = s
            && dn == name
        {
            if in_twin {
                twin.push(*init);
            } else {
                mono.push((*mutable, *init));
            }
        }
        let nested_twin = in_twin
            || matches!(s, Stmt::FnDecl { name: f, .. }
                    if crate::ast::fnexpr_this_names::is_twin_body_name(f));
        crate::ast::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_decls_split(inner, name, nested_twin, mono, twin)
        });
    }
}
