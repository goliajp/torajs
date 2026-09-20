//! `snapshot_class_index` — the per-class tuple pass 3 and its
//! siblings walk, taken once at the top of `desugar_classes` before
//! any ClassDecl is replaced. Split out of `desugar_classes.rs` at the
//! file-size cap; the seam is the question each side answers: this
//! file answers "which classes does the rest of the desugar see, and
//! what does it need of each", the driver next door answers "in what
//! order do the passes run".

use super::desugar_classes_super::ClassIndexEntry;
use super::{Ast, Stmt};

/// Snapshot the class metadata (cloned out so `ast.stmts` can be
/// mutated in-place without aliasing). M5.2 adds `parent` to the
/// tuple — for inheritance flattening + super(args) rewriting;
/// M-OO.4 adds the static-init / static-methods slices.
///
/// RFC 20260815 — the heritage is an expression; this static lane
/// keys everything on the parent NAME, so only a bare-Ident heritage
/// enters the index. A class whose heritage is a non-Ident expression
/// is left OUT (never mapped to None — that would mis-file it as a
/// root class, a silent wrong): it stays in `ast.stmts`, survives to
/// check, and is reported loudly through the same
/// `unclaimed_class_reasons` channel the capturing lane uses.
pub(super) fn snapshot_class_index(ast: &mut Ast) -> Vec<ClassIndexEntry> {
    let mut out = Vec::new();
    let mut expr_heritage: Vec<String> = Vec::new();
    for (i, s) in ast.stmts.iter().enumerate() {
        if let Stmt::ClassDecl {
            name,
            decl_id,
            type_params,
            parent,
            is_abstract: _,
            fields,
            static_init,
            ctor,
            methods,
            static_methods,
        } = s
        {
            // RFC 20260921 — republish the declaration span under the
            // name this class ENDS UP with. The parser keyed it by
            // name, where a same-named sibling overwrote it, so
            // `String(C)` answered the other class's text; by here the
            // collision renames have run and the name is this
            // declaration's alone.
            if let Some(span) = ast.class_decl_spans_by_decl.get(decl_id).copied() {
                ast.class_decl_spans.insert(name.clone(), span);
            }
            let parent_name = ast.parent_ident_name(*parent).map(str::to_string);
            if parent.is_some() && parent_name.is_none() {
                expr_heritage.push(name.clone());
                continue;
            }
            // A value-shaped parent (knife 2a) belongs to the
            // capturing lane; one that reaches this snapshot was
            // DECLINED there for a recorded reason — skip it (filing
            // `__ccp<N>` as a class name would panic the forward-ref
            // check) and let check report the decline.
            if parent_name
                .as_deref()
                .is_some_and(|p| ast.es5_value_parents.contains(p))
            {
                continue;
            }
            out.push((
                i,
                name.clone(),
                type_params.clone(),
                parent_name,
                fields.clone(),
                static_init.clone(),
                ctor.clone(),
                methods.clone(),
                static_methods.clone(),
                *decl_id,
            ));
        }
    }
    for name in expr_heritage {
        ast.unclaimed_class_reasons
            .push((name, super::capturing_classes::EXPR_HERITAGE_REASON));
    }
    out
}
