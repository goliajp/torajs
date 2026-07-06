//! `Stmt::LetDecl { mutable, name, type_ann, init, is_var }`
//! typecheck pulled out of [`crate::check::Checker::check_stmt`]'s
//! `Stmt::LetDecl` arm as chunk-108 of the check_stmt decomp.
//! 11th check_stmt sibling — last big check_stmt arm.
//!
//! Steps:
//!
//! 1. **Empty array narrow** (M1.2 / P0.10) — bare `[]` carries no
//!    element-type info; annotation must provide it. Untyped `[]`
//!    defaults to `Array<Any>` (TS spec: untyped `[]` is `any[]`);
//!    test262 uses bare `let arr = []` pervasively. Annotation
//!    must be `Array<_>` if present.
//! 2. **Non-empty init** — typecheck via `type_of`.
//! 3. **Annotation check** — if `type_ann` present, resolve +
//!    `is_assignable_to_resolved` against init type. Final type =
//!    annotation; otherwise init type.
//! 4. **Alias classification** — `classify_init_alias`: Member /
//!    Index / cross-scope Ident init aliases a heap owned
//!    elsewhere. Mark `borrowed` so transfer sites reject
//!    mid-scope moves; return/throw escapes stay legal
//!    (retain-at-boundary).
//! 5. **M-OO.5 nominal info** — if annotation matches a known class
//!    name (in `ast.class_parents`), propagate `declared_class` so
//!    `name.private_member` access can lookup visibility entry.
//! 6. **Declare** — push binding with computed `LocalInfo`.
//!
//! Note: let-rhs is NOT a transfer site — same-scope `let t = s`
//! SHARES ownership (ssa_lower retains at the binding site —
//! CPython incref / Swift strong-assignment semantics); `s` stays
//! fully usable afterwards.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, DiagPush, LocalInfo, Type};
use crate::check_assignable::is_assignable_to_resolved;
use crate::check_type_ann::resolve_type_ann_full;

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    mutable: bool,
    name: &str,
    type_ann: &Option<String>,
    init: ExprId,
) {
    let is_empty_array = matches!(ast.get_expr(init), Expr::Array(els) if els.is_empty());
    let init_ty = if is_empty_array {
        match check_empty_array_ann(checker, name, type_ann) {
            Some(t) => t,
            None => return,
        }
    } else {
        match checker.type_of(ast, init) {
            Ok(t) => t,
            Err(e) => {
                checker.errors.push_err(e);
                return;
            }
        }
    };
    let final_ty = match type_ann {
        None => init_ty,
        Some(ann) => {
            let Some(ann_ty) =
                resolve_type_ann_full(ann, &checker.aliases, &[], &checker.generic_alias_decls)
            else {
                checker.errors.push_err(format!("unknown type `{ann}`"));
                return;
            };
            if !is_assignable_to_resolved(
                &ann_ty,
                &init_ty,
                &checker.aliases,
                &checker.generic_alias_decls,
            ) {
                checker.errors.push_err(format!(
                    "type mismatch on `{name}`: declared {ann_ty:?}, init has {init_ty:?}"
                ));
                return;
            }
            ann_ty
        }
    };
    // RC-4 F1c — a defineProperty receiver's unannotated ObjectLit
    // binding types as `any`: the define lowering converts the cell
    // to a DynObj and the write-back only rebinds Any-typed slots,
    // so a static struct type would strand the defined property on
    // an orphan cell (test262 gOPN accessor family).
    let final_ty = if type_ann.is_none()
        && matches!(ast.get_expr(init), Expr::ObjectLit { .. })
        && checker.dynobj_degraded.contains(name)
    {
        Type::Any
    } else {
        final_ty
    };
    let is_alias_init = checker.classify_init_alias(ast, init);
    let declared_class: Option<String> = type_ann.as_ref().and_then(|s| {
        if ast.class_parents.contains_key(s.as_str()) {
            Some(s.clone())
        } else {
            None
        }
    });
    if let Err(e) = checker.declare(
        name.to_string(),
        LocalInfo {
            ty: final_ty,
            mutable,
            moved: false,
            borrowed: is_alias_init,
            declared_class,
        },
    ) {
        checker.errors.push_err(e);
    }
}

fn check_empty_array_ann(
    checker: &mut Checker,
    name: &str,
    type_ann: &Option<String>,
) -> Option<Type> {
    match type_ann {
        Some(ann) => {
            let Some(t) =
                resolve_type_ann_full(ann, &checker.aliases, &[], &checker.generic_alias_decls)
            else {
                checker.errors.push_err(format!("unknown type `{ann}`"));
                return None;
            };
            if !matches!(t, Type::Array(_)) {
                checker.errors.push_err(format!(
                    "empty array literal `{name}` needs an array type annotation, got `{ann}`"
                ));
                return None;
            }
            Some(t)
        }
        None => Some(Type::Array(Box::new(Type::Any))),
    }
}
