//! Pre-declaring a binding whose own initializer captures it —
//! `const f = function (n) { … f(n - 1) … }` and the arrow form.
//!
//! Split out of [`crate::check_stmt_let_decl`] under the 500-line
//! hard limit; bodies verbatim. That file walks a declaration from
//! init type to `LocalInfo`; this one answers the single question of
//! whether the binding has to exist before its own initializer is
//! checked, and cleans up after itself through [`SelfRef`].

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, LocalInfo, Type};
use crate::check_stmt_let_decl::widen_binding_ty;
use crate::check_type_ann::resolve_type_ann_full;

/// A closure init whose capture list names the very binding it
/// initializes — `const f = function (n) { … f(n - 1) … }` and the
/// arrow form. The capture resolves against the OUTER scope, which does
/// not hold the binding yet, so the capture walk answers "unknown
/// identifier" and the whole decl fails to type.
///
/// Declaring it up front is sound because a closure's value type comes
/// from its lifted FnDecl's annotations and never from its body
/// ([`crate::check_type_of_fn::closure_sig`]) — there is no circular
/// inference to break, only a scope-order artifact. Answers whether a
/// provisional entry was pushed so the real declare can replace it.
///
/// Only self-reference: a capture of a LATER binding (two arrows that
/// call each other) still fails, and does so loudly.
///
/// The annotation gate keeps this in step with the lowering lane that
/// has to serve it (`ssa_lower_stmt_let_decl_recursive`). That lane
/// claimed a binding only when it took a `Closure` slot, so an
/// `any`-annotated one was declined here in step — admitting it would
/// have traded an honest "unknown identifier" for a failure at lower
/// time. The lane now opens an `Any` box for the bare form too, the
/// same box it already opened for a closure nested inside a composite
/// init, so the annotation is served and admits here as well.
///
/// An `any` binding declares as `any`, not as the closure's signature:
/// that is what was written, and it is what makes a write from inside
/// the body (`let h: any = function () { h = 9 }`) an ordinary
/// PutValue rather than a type error.
/// What [`predeclare_self_ref_closure`] pushed.
pub(crate) enum SelfRef {
    None,
    /// Typed off the annotation or the closure's own signature.
    Provisional,
    /// Typed off the initializer's skeleton; the declaration must
    /// arrive at the same type, since the closure bodies were checked
    /// against it.
    Skeleton(Type),
}

pub(crate) fn predeclare_self_ref_closure(
    checker: &mut Checker,
    ast: &Ast,
    name: &str,
    type_ann: &Option<String>,
    init: ExprId,
) -> SelfRef {
    let Expr::Closure { fn_name, captures } = ast.get_expr(init) else {
        // 400-01 — a closure minted DEEPER in a composite init
        // self-capturing the binding (`const s: any = { f: function
        // () { … s … } }`): without the pre-declare the capture
        // resolved "unknown", the prune dropped it, and the body's
        // read fell to the dynamic-global lane (runtime
        // ReferenceError). The lowering half is the recursive lane's
        // ordinary-binding box (fn_name: None), which retypes its
        // capture slots to whatever the initializer produces. An
        // annotated binding declares its annotation; an unannotated
        // one declares the initializer's skeleton type — its closures
        // answer their signatures without walking the bodies, which
        // is what TS does too (the binding's type never depends on a
        // closure body unless the closure's own return is inferred,
        // and then the signature already says `any`).
        let mut nested: Vec<&str> = Vec::new();
        crate::ast::nested_closure_captures::collect(ast, init, &mut nested);
        if !nested.contains(&name) {
            return SelfRef::None;
        }
        let (ty, verdict) = match type_ann {
            Some(ann) => {
                let Some(ty) =
                    resolve_type_ann_full(ann, &checker.aliases, &[], &checker.generic_alias_decls)
                else {
                    return SelfRef::None;
                };
                (ty, SelfRef::Provisional)
            }
            None => {
                checker.speculative_depth += 1;
                checker.defer_closure_bodies += 1;
                let skeleton = checker.type_of(ast, init);
                checker.defer_closure_bodies -= 1;
                checker.speculative_depth -= 1;
                let Ok(ty) = skeleton else {
                    return SelfRef::None;
                };
                let ty = if ty == Type::Void {
                    Type::Undefined
                } else {
                    ty
                };
                let ty = widen_binding_ty(checker, ast, init, None, ty);
                (ty.clone(), SelfRef::Skeleton(ty))
            }
        };
        return declare_provisional(checker, name, ty, verdict);
    };
    if !captures.iter().any(|c| c == name) {
        return SelfRef::None;
    }
    let mut ann_any = false;
    if let Some(ann) = type_ann {
        let resolved =
            resolve_type_ann_full(ann, &checker.aliases, &[], &checker.generic_alias_decls);
        match resolved {
            Some(Type::Function(..)) => {}
            Some(Type::Any) => ann_any = true,
            _ => return SelfRef::None,
        }
    }
    let ty = if ann_any {
        Type::Any
    } else {
        let Ok(sig) = crate::check_type_of_fn::closure_sig(ast, fn_name, &checker.aliases) else {
            return SelfRef::None;
        };
        sig.value_ty
    };
    declare_provisional(checker, name, ty, SelfRef::Provisional)
}

fn declare_provisional(checker: &mut Checker, name: &str, ty: Type, verdict: SelfRef) -> SelfRef {
    let info = LocalInfo::plain(ty);
    match checker.declare(name.to_string(), info) {
        Ok(_) => verdict,
        Err(_) => SelfRef::None,
    }
}
