//! A `new Map()` initializer and the Map type its binding gets.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};
use crate::check_type_ann::resolve_type_ann_full;

/// A `new Map()` initializer is the Map its binding declares (TS
/// contextual typing). The type is recorded on the `new`: lowering
/// reads it there to give a fresh `Map<K, number>` its typed value
/// lane (`ssa_lower_map_num_lane`).
pub(super) fn record_fresh_map_ty(checker: &mut Checker, ast: &Ast, init: ExprId, ty: &Type) {
    if matches!(ast.get_expr(init), Expr::New { class_name, .. } if class_name == "Map")
        && matches!(ty, Type::Map(..))
    {
        checker.expr_types.insert(init, ty.clone());
    }
}

/// An unannotated `new Map<K, V>()` is the Map its type arguments
/// spell — `check_map` never sees them and answers `Map<any, any>`.
pub(super) fn map_ty_from_type_args(checker: &Checker, ast: &Ast, init: ExprId, ty: Type) -> Type {
    match ast.get_expr(init) {
        Expr::New {
            class_name,
            type_args,
            ..
        } if class_name == "Map" && !type_args.is_empty() && ty == Type::map_any() => {
            crate::ast_refs::collection_new_ann(ast, init)
                .and_then(|ann| {
                    resolve_type_ann_full(&ann, &checker.aliases, &[], &checker.generic_alias_decls)
                })
                .unwrap_or(ty)
        }
        _ => ty,
    }
}
