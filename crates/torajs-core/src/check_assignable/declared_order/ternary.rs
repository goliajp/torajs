//! A conditional expression in a declared position.
//!
//! `const r: Shape | undefined = c ? undefined : new Sq()` is legal:
//! the conditional's type is the union of its branch types, and each
//! fits the declared one. The branch join alone (`unify_ternary`) has
//! no declared type to aim at, so an `undefined` arm against an object,
//! or two classes with no common ancestor, join to `any` — and a box is
//! not the pointer an object-typed slot reads. Under an object type (a
//! class, an interface or another object type, nullable or not) whose
//! every branch fits, the conditional takes the declared type instead:
//! the lowering stores each arm's pointer as it is (an `undefined` arm
//! as the slot's undefined cell), and every class instance let in is
//! noted like one assigned there directly.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, DiagPush, Type};

/// `to` when `value` is a conditional whose branches joined to `any`
/// and each fits the object type `to`; otherwise `value_ty`.
pub(super) fn declared_join(
    checker: &mut Checker,
    ast: &Ast,
    to: &Type,
    value: ExprId,
    value_ty: Type,
) -> Type {
    if value_ty != Type::Any {
        return value_ty;
    }
    let Expr::Ternary {
        then_branch,
        else_branch,
        ..
    } = ast.get_expr(value)
    else {
        return value_ty;
    };
    let (then_branch, else_branch) = (*then_branch, *else_branch);
    let (nullable, inner) = match to {
        Type::Nullable(t) => (true, &**t),
        t => (false, t),
    };
    if !matches!(inner, Type::Struct(_) | Type::ClassRef(_)) {
        return value_ty;
    }
    for b in [then_branch, else_branch] {
        let Some(bt) = checker.expr_types.get(&b).cloned() else {
            return value_ty;
        };
        let bt = super::in_declared_order(checker, ast, to, b, bt);
        let fits = match (&bt, inner) {
            (Type::Undefined | Type::Null, _) => nullable,
            (Type::Any | Type::Never, _) => false,
            // a class slot reads its own offsets: only an instance of
            // the class or of one below it is laid out that way, as a
            // direct initializer is held to
            (Type::ClassRef(c), Type::ClassRef(want)) => {
                if c != want && !checker.is_descendant_of(ast, c, want) {
                    checker.errors.push_err(format!(
                        "type mismatch on conditional branch: declared {to:?}, branch has {bt:?}"
                    ));
                    return value_ty;
                }
                true
            }
            _ => crate::check_assignable::is_assignable_to_resolved(
                to,
                &bt,
                &checker.class_structs,
                &checker.aliases,
                &checker.generic_alias_decls,
            ),
        };
        if !fits {
            return value_ty;
        }
    }
    checker.expr_types.insert(value, to.clone());
    to.clone()
}
