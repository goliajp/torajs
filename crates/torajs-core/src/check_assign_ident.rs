//! `Expr::Assign { target: Expr::Ident(name), value }` typecheck
//! sub-arm pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Assign` arm
//! as chunk-93 of the type_of_inner decomp.
//!
//! Two resolution paths (mirrors ssa_lower's `Expr::Assign` shape):
//!
//! 1. **Phase K.3 top-level data global** — when `lookup(name)` is
//!    `None` AND `globals` has the name: read declared global type,
//!    typecheck the value against it (`is_assignable_to_resolved`),
//!    return the global type. (Const-ness not tracked for globals
//!    separately from LetDecl's `mutable` flag — pre-pass
//!    registration order means any top-level binding is writable
//!    from named-fn bodies for now.)
//! 2. **Local binding** — `lookup(name)` returns `LocalInfo`; reject
//!    if `!mutable` (const); typecheck value vs slot type;
//!    `mark_unmoved(name)` clears the target's transient `moved`
//!    flag if rhs was `target + ...` (e.g. string concat); return
//!    the target type.
//!
//! Assignment does NOT consume the rhs (chunk 564): ssa_lower's
//! assign lane retains borrow-shape rhs (+1 share), so the source
//! binding keeps its stake and stays readable/transferable — the
//! historical "cannot transfer" rejection guarded a missing retain
//! contract that now exists.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};
use crate::check_assignable::is_assignable_to_resolved;

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    target: ExprId,
    name: String,
    value: ExprId,
) -> Result<Type, String> {
    if checker.lookup(&name).is_none()
        && let Some(global_ty) = checker.globals.get(&name).cloned()
    {
        // A write-target mark under an incomplete scope (speculative
        // pre-pass) that NOW resolves is legal — self-heal, mirroring
        // the read side (check_type_of_ident).
        checker.undeclared_reads.remove(&target);
        let value_ty = checker.type_of(ast, value)?;
        if !is_assignable_to_resolved(
            &global_ty,
            &value_ty,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        ) {
            return Err(format!(
                "type mismatch assigning to global `{name}`: declared {global_ty:?}, value is {value_ty:?}"
            ));
        }
        return Ok(global_ty);
    }
    let info = match checker.lookup(&name) {
        Some(i) => i,
        None => {
            // RFC 20260730-undeclared-ident, write position — §6.2.5.6
            // PutValue on an unresolvable Reference in strict code (module
            // code always is) raises a catchable ReferenceError at run
            // time, not a compile reject. Mark the TARGET ident in the
            // same `undeclared_reads` occurrence table the read side uses
            // (spec-wise both are the same unresolvable Reference; the
            // assign / post-incr lowering lanes consult it by target eid)
            // and keep typing the RHS — §13.15.2 evaluates rref before
            // PutValue throws, so its side effects (and any marked reads
            // inside it, e.g. the desugared `x = x * v` compound form)
            // are real. Same carve-outs as the read side: `__`-prefixed
            // names are compiler-synthesized, and known builtin globals
            // (`Object = 12` / `NaN = 12` global-property write
            // semantics) stay a hard reject — recorded boundary.
            if name.starts_with("__") || crate::check::is_known_builtin_global(&name) {
                return Err(format!("assignment to undeclared `{name}`"));
            }
            checker.undeclared_reads.insert(target, name);
            return checker.type_of(ast, value);
        }
    };
    // Same self-heal as the global arm above: the target resolved on
    // this (complete-scope) pass, so any speculative-pass mark is
    // stale — an IIFE param shadowing an outer fn param left the mark
    // behind and the assign lane threw a spurious ReferenceError
    // (test262 parameter-name-shadowing-parameter-name-let-const-and-var).
    checker.undeclared_reads.remove(&target);
    // §15.5.5 (RFC 20260810) — a write resolving to the enclosing
    // fn-expression's self-name hits an immutable function-env
    // binding: mark the target eid for the assign lane's runtime
    // TypeError (strict semantics — module code always is) and keep
    // typing the RHS (§13.15.2 evaluates rref before PutValue
    // throws). A deeper-scope shadow re-declared the name and owns
    // the write instead, so the mark stays off for it.
    if checker.self_name_active.as_deref() == Some(name.as_str())
        && checker
            .scopes
            .iter()
            .skip(1)
            .all(|s| !s.contains_key(&name))
    {
        checker.self_name_writes.insert(target);
        return checker.type_of(ast, value);
    }
    if !info.mutable {
        return Err(format!("cannot assign to const `{name}`"));
    }
    // ut3 assignment narrowing — validate against the DECLARED type
    // (ledger entry when currently narrowed): `b = null` after
    // `b = "x"` must stay legal, the narrow never shrinks the
    // assignable surface.
    let target_ty = checker.assign_declared_ty(&name, &info.ty);
    let value_ty = checker.type_of(ast, value)?;
    // 424-04 — the fn-face admit the LET position takes (423-03 ④)
    // now applies to the assign position too: a mutable fn-typed
    // binding is a closure_bindings member (the forwarder pass wraps
    // its store sites into closure cells), and the mismatch census
    // (`ast/forwarders_object_mismatch.rs`) routes a face-mismatched
    // binding's calls through the boxed dual entry, whose argc +
    // undefined-filled argv deliver §10.2.1.4 for every stored face.
    let fn_slot_widened = crate::check_assignable::fn_slot_admits(
        &target_ty,
        &value_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    if !fn_slot_widened
        && !is_assignable_to_resolved(
            &target_ty,
            &value_ty,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        )
    {
        return Err(format!(
            "type mismatch assigning to `{name}`: declared {target_ty:?}, value is {value_ty:?}"
        ));
    }
    // A possibly-null value kills an existing narrow right here
    // (expression-level assigns included); minting a narrow stays
    // statement-level (check_stmt's Assign hook).
    checker.assign_narrow_demote(&name, &value_ty);
    checker.mark_unmoved(&name);
    Ok(target_ty)
}
