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
use crate::check::{Checker, DiagPush, Type};
use crate::check_assignable::is_assignable_to_resolved;
use crate::check_stmt_let_decl::fn_face_slot::{
    record_census_store, record_global_store_face, record_store_face,
};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    target: ExprId,
    name: String,
    value: ExprId,
) -> Result<Type, String> {
    // §15.7.14 step 3 again, in the ES5 lane's spelling. A
    // `__cci<N>_<C>` binding is the class-scope immutable one
    // `capturing_classes` mints, and nothing outside the class body
    // can name it — the container reads the mutable `__cc<N>_<C>`
    // alias — so a write here is the class writing its own name:
    // the same runtime TypeError the `__class_<C>` arm below gives
    // the hoist lane, and the same kernel a fn-expression writing its
    // own self-name uses.
    //
    // Asked before either resolution arm because neither would catch
    // it: a method body reaches the binding as a lifted-closure
    // capture, where the `const` the lane declared is not the thing
    // being looked up. Until now the write silently SUCCEEDED —
    // `typeof C` inside the body answered "number" afterwards while
    // bun threw — which is the failure mode this ordering exists for.
    if let Some(r) = write_outside_lookup(checker, ast, target, &name, value) {
        return r;
    }
    if checker.lookup(&name).is_none()
        && let Some(global_ty) = checker.globals.get(&name).cloned()
    {
        // A write-target mark under an incomplete scope (speculative
        // pre-pass) that NOW resolves is legal — self-heal, mirroring
        // the read side (check_type_of_ident).
        checker.undeclared_reads.remove(&target);
        let value_ty = checker.type_of(ast, value)?;
        let value_ty =
            crate::check_assignable::in_declared_order(checker, ast, &global_ty, value, value_ty);
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
        record_global_store_face(checker, ast, &name, &global_ty, &value_ty);
        // An array literal takes the slot's element type (rotation 629
        // — `var x = []; x = [0]` minted a `number[]` for an `any[]`
        // slot and died at lowering), the contextual typing a declared
        // `any[]` gives its initializer.
        crate::check_stmt_let_decl::apply_contextual_array_ann_store(
            checker, ast, value, &global_ty,
        );
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
            // are real. Same carve-outs as the read side: a name TR
            // MINTED (`is_synthesized_dunder` — not merely one spelled
            // with the `__` prefix, which sputnik writes as user code),
            // and known builtin globals (`Object = 12` / `NaN = 12`
            // global-property write semantics) stay a hard reject —
            // recorded boundary.
            // §15.7.14 ClassDefinitionEvaluation step 3 — the class
            // scope holds an IMMUTABLE binding for the class's own
            // name, and every method body sits inside that scope. So
            // a write to it from inside the class is a runtime
            // TypeError, exactly like the fn-expression self-name
            // below, and bun answers both with the same message
            // ("Attempted to assign to readonly property." — probed
            // against a static method, an instance method and a
            // static field initialiser). The OUTER binding a class
            // DECLARATION also creates is mutable and is a separate
            // question — `class C {}; C = 1` at top level is legal and
            // still rejected here, which is the two-binding model
            // carried in plan-state, not this lane.
            //
            // `current_class` is what says "inside": the checker sets
            // it while walking a `__cm_<C>__*` / `__sm_<C>__*` body,
            // and `desugar_classes` has already rewritten the source's
            // `C` to `__class_<C>` by the time this runs. Writing
            // another class's name from in here is the outer binding
            // again, so the two names must agree.
            if let Some(cls) = name.strip_prefix("__class_")
                && checker.current_class.as_deref() == Some(cls)
                && checker.class_names.contains(cls)
            {
                checker.self_name_writes.insert(target);
                return checker.type_of(ast, value);
            }
            if checker.is_synthesized_dunder(&name) || crate::check::is_known_builtin_global(&name)
            {
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
        // Band B (`docs/language-spec/ch1-type-runtime-boundary.md`
        // §4, row "spec defines a runtime error"): §6.2.5.6 PutValue
        // reaches §9.1.1.1.5 SetMutableBinding, whose step 5 forces
        // S to true for a STRICT immutable binding — which is what
        // `const` / `using` / `await using` and an import binding
        // create. So the spec's answer here is a runtime TypeError,
        // not the absence of a program: test262's
        // `using-invalid-assignment-*` are POSITIVE tests wrapping
        // the write in `assert.throws(TypeError, ...)`, and they
        // cannot run at all if the file is refused.
        //
        // bun refuses it too, and says why in its own message —
        // "This assignment will throw because `x` is a constant".
        // Both it and tsc know the outcome and neither hands back an
        // executable; deciding it here and still emitting the throw
        // is the position §4's table describes.
        checker.const_writes.insert(target);
        checker
            .errors
            .push_warn(format!("assignment to const `{name}` throws at run time"));
        return checker.type_of(ast, value);
    }
    // ut3 assignment narrowing — validate against the DECLARED type
    // (ledger entry when currently narrowed): `b = null` after
    // `b = "x"` must stay legal, the narrow never shrinks the
    // assignable surface.
    let target_ty = checker.assign_declared_ty(&name, &info.ty);
    let target_ty = checker.union_assign_target(&name, target_ty);
    let value_ty = checker.type_of(ast, value)?;
    let value_ty =
        crate::check_assignable::in_declared_order(checker, ast, &target_ty, value, value_ty);
    let decl = (info.ann.as_deref(), info.decl.as_ref());
    admit_store(checker, (ast, &name), decl, &target_ty, (&value_ty, value))?;
    // See the global arm above — the literal is typed by the slot.
    crate::check_stmt_let_decl::apply_contextual_array_ann_store(checker, ast, value, &target_ty);
    // A possibly-null value kills an existing narrow right here
    // (expression-level assigns included); minting a narrow stays
    // statement-level (check_stmt's Assign hook).
    checker.assign_narrow_demote(&name, &value_ty);
    checker.mark_unmoved(&name);
    Ok(target_ty)
}

/// The two write targets decided before either resolution arm: the
/// ES5 lane's class-scope binding (`__cci<N>_<C>`, a runtime
/// TypeError like the fn-expression self-name), and a target the
/// module resolver judged unresolvable in its own module
/// (`Ast::module_foreign_reads` — §16.2.1.6: PutValue on an
/// unresolvable Reference in module code raises ReferenceError at run
/// time, whatever the flat top level would resolve the name to).
fn write_outside_lookup(
    checker: &mut Checker,
    ast: &Ast,
    target: ExprId,
    name: &str,
    value: ExprId,
) -> Option<Result<Type, String>> {
    if crate::ast::capturing_classes::is_es5_inner_class_binding(name) {
        checker.self_name_writes.insert(target);
        return Some(checker.type_of(ast, value));
    }
    if ast.module_foreign_reads.contains(&target) {
        checker.undeclared_reads.insert(target, name.to_string());
        return Some(checker.type_of(ast, value));
    }
    None
}

/// Does the slot `name`, declared `target_ty`, take a store of
/// `value_ty`? 424-04 — the fn-face admit the LET position takes
/// (423-03 ④) applies to the assign position too: a mutable fn-typed
/// binding is a closure_bindings member (the forwarder pass wraps its
/// store sites into closure cells), and a face-mismatched binding's
/// calls ride the boxed dual entry, whose argc + undefined-filled argv
/// deliver §10.2.1.4 for every stored face. A store of another face
/// than the slot's, told apart by the calling convention, joins that
/// census here for the declaration `decl` (annotation, initializer)
/// the write reaches (`check_stmt_let_decl::fn_face_slot`), and so
/// does a store of a census slot's value.
fn admit_store(
    checker: &mut Checker,
    (ast, name): (&Ast, &str),
    decl: (Option<&str>, Option<&crate::check::FnSlot>),
    target_ty: &Type,
    (value_ty, value): (&Type, ExprId),
) -> Result<(), String> {
    let fn_slot_widened = crate::check_assignable::fn_slot_admits(
        target_ty,
        value_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    if fn_slot_widened || *value_ty == Type::Any {
        let (ann, init) = decl;
        record_store_face(checker, name, ann, init, target_ty, value_ty);
    }
    if !fn_slot_widened
        && !is_assignable_to_resolved(
            target_ty,
            value_ty,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        )
    {
        return Err(format!(
            "type mismatch assigning to `{name}`: declared {target_ty:?}, value is {value_ty:?}"
        ));
    }
    record_census_store(checker, ast, name, decl.1, target_ty, value);
    Ok(())
}
