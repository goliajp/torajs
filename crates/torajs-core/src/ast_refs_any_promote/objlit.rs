//! The desugar-phase census of method-bearing top-level object
//! literals the shared any-promotion verdict
//! (the parent module) will box — split from it to keep it under
//! the size line.

use crate::ast::{Ast, Expr};

/// The (k) leg of `objlit_nominal_anylane` — method-bearing top-level
/// literals THIS verdict family will promote to an Any slot, computed
/// against the pre-`objlit_nominal` snapshot so their this-using
/// methods take the `__this: any` receiver-first stamp instead of the
/// nominal `__this: __ObjLit_n` one (whose struct-offset body reads
/// against the promoted dynobj receiver answered raw entry bytes —
/// the 546-03 box-bits silent-wrong).
///
/// The gate MIRRORS the pass_2 / `inferred_slot_ty` fallback
/// position, restricted to the ObjectLit case: a flat-walked
/// (`toplevel_stmts_flat`) un-annotated non-`var` let whose name a
/// named fn reads, whose literal `objlit_literal_inlobj_ann` refuses
/// (an all-shaped literal rides the `__inlobj` + `__mth` typed lane
/// and must NOT be marked anylane), and whose shape
/// [`super::any_slot_safe_value`] admits — the same verdict fn both
/// consumers call, so the three sites cannot drift. The degraded
/// direction is the (i) leg's job; literals both legs catch mark
/// once (set semantics). Mirrors the (i) leg's timing caveat: this
/// runs on the desugar-phase snapshot of a set the checker recomputes
/// post-desugar, and the conformance gate is the drift detector.
pub(crate) fn promoted_method_objlits(ast: &Ast) -> std::collections::HashSet<u32> {
    let refs = crate::ast_refs::toplevel_binding_refs(ast);
    let mut out = std::collections::HashSet::new();
    for s in crate::ast::toplevel_stmts_flat(ast) {
        let crate::ast::Stmt::LetDecl {
            name,
            init,
            type_ann: None,
            is_var: false,
            ..
        } = s
        else {
            continue;
        };
        if !refs.named_fn_refs.contains(name) {
            continue;
        }
        let Expr::ObjectLit { fields } = ast.get_expr(*init) else {
            continue;
        };
        if fields
            .iter()
            .any(|(_, fe)| ast.objlit_method_exprs.contains(fe))
            && crate::ast_refs::objlit_literal_inlobj_ann(ast, *init).is_none()
            && super::any_slot_safe_value(ast, *init)
        {
            out.insert(init.0);
        }
    }
    out
}
