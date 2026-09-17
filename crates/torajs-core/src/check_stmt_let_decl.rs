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
    let self_ref_closure = predeclare_self_ref_closure(checker, ast, name, type_ann, init);
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
    // L3b ③ — a generic fn escaping as a VALUE (`const g = f` where
    // `f`'s face still carries un-inferred TypeVars from the
    // implicit-generics pass): the binding's direct call is a
    // CallIndirect with no mono channel, so the TypeVar face rejects
    // every argument. Widen the face all-TypeVar→Any and record the
    // site; `check_monomorph_any_widen` clones an all-`any` spec and
    // the ident lowering takes ITS address (the generic original is a
    // checker template that never lowers).
    let init_ty = match (&init_ty, ast.get_expr(init)) {
        (Type::Function(ptys, rty), Expr::Ident(n))
            if checker.generic_type_params.contains_key(n.as_str())
                && (ptys.iter().any(|t| matches!(t, Type::TypeVar(_)))
                    || matches!(**rty, Type::TypeVar(_))) =>
        {
            checker.fn_escape_widen_sites.insert(init, n.clone());
            let subst: std::collections::HashMap<String, Type> = checker.generic_type_params
                [n.as_str()]
            .iter()
            .map(|tv| (tv.clone(), Type::Any))
            .collect();
            crate::check_substitute_typevars::substitute_typevars(&init_ty, &subst)
        }
        _ => init_ty,
    };
    // Chunk 618 — a void-call init binds undefined (the call runs
    // for effect; a fn that produces no value returns undefined).
    // Void is not a value type: pre-fix the binding carried Void
    // and every consumer (print / any-box) panicked at lower time
    // ("box_to_any element type Void", p1 probe).
    let init_ty = if init_ty == Type::Void {
        Type::Undefined
    } else {
        init_ty
    };
    // RFC 20260714-dstr-residual blade 3 — an array binding pattern
    // whose source isn't a statically indexable container. This is the
    // one place in the pipeline that knows: pick the group's lane here
    // and let the lowerer read the verdict back off the post-check AST.
    let init_ty = match crate::check_stmt_let_decl_destr_lane::pick_ary_destr_lane(
        checker, ast, init, &init_ty,
    ) {
        Some(materialized) => materialized,
        None => init_ty,
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
            // 423-03 ④ — a fn-typed LET takes the same S2 call-face
            // admit a fn-typed call ARG takes (`const slot: () =>
            // void = gb` where gb declares a defaulted param): the
            // sig-thunk pass reabstracts the mismatched init exactly
            // like a call-arg position, so the checker face and the
            // lowering face agree. See `fn_slot_admits` on why this
            // is not widened inside `is_assignable_to` itself.
            let fn_slot_widened = crate::check_assignable::fn_slot_admits(
                &ann_ty,
                &init_ty,
                &checker.class_structs,
                &checker.aliases,
                &checker.generic_alias_decls,
            );
            if !fn_slot_widened
                && !is_assignable_to_resolved(
                    &ann_ty,
                    &init_ty,
                    &checker.class_structs,
                    &checker.aliases,
                    &checker.generic_alias_decls,
                )
            {
                checker.errors.push_err(format!(
                    "type mismatch on `{name}`: declared {ann_ty:?}, init has {init_ty:?}"
                ));
                return;
            }
            apply_contextual_array_ann(checker, ast, init, &ann_ty);
            ann_ty
        }
    };
    let final_ty = widen_binding_ty(checker, ast, init, type_ann.is_none(), final_ty);
    if let SelfRef::Skeleton(provisional) = &self_ref_closure
        && *provisional != final_ty
    {
        checker.errors.push_err(format!(
            "internal: `{name}` pre-declared as {provisional:?} for its initializer's closures, declared {final_ty:?}"
        ));
    }
    let is_alias_init = checker.classify_init_alias(ast, init);
    let declared_class: Option<String> = type_ann.as_ref().and_then(|s| {
        let base = crate::check_type_ann::strip_nullable(s);
        if ast.class_parents.contains_key(base) {
            Some(base.to_string())
        } else {
            None
        }
    });
    // RFC 20260725-str-method-value-reify — mark bindings whose init
    // is a String-receiver builtin method VALUE read (`const m =
    // s.slice`): their `.call`/`.apply`/`.bind` admit any-dispatched
    // (route_early), sidestepping the member table's fixed-arity sig.
    let builtin_mv = is_builtin_mv_init(checker, ast, init);
    if !matches!(self_ref_closure, SelfRef::None) {
        // Drop the provisional entry the pre-declare pushed — it existed
        // only so the init's own capture could resolve. The real
        // `LocalInfo` below is the one that carries the alias / nominal
        // verdicts, so it replaces rather than collides with it.
        checker
            .scopes
            .last_mut()
            .expect("at least one scope is always present")
            .remove(name);
    }
    if let Err(e) = checker.declare(
        name.to_string(),
        LocalInfo {
            ty: final_ty,
            mutable,
            moved: false,
            borrowed: is_alias_init,
            declared_class,
            builtin_mv,
        },
    ) {
        checker.errors.push_err(e);
    }
    // An initializer is the binding's first assignment — see
    // `Checker::apply_decl_narrow`.
    checker.apply_decl_narrow(ast, name, init);
}

/// The widenings a binding's type takes after its initializer types —
/// shared by the declaration and by the skeleton pre-declare, which
/// must answer the same type the declaration will.
fn widen_binding_ty(
    checker: &Checker,
    ast: &Ast,
    init: ExprId,
    unannotated: bool,
    ty: Type,
) -> Type {
    // RC-4 F1c — a defineProperty receiver's unannotated ObjectLit
    // binding types as `any`: the define lowering converts the cell
    // to a DynObj and the write-back only rebinds Any-typed slots,
    // so a static struct type would strand the defined property on
    // an orphan cell (test262 gOPN accessor family).
    //
    // 2026-07-16 (rotation 121 chunk 5-followup) — the same widen
    // applies to an unannotated ObjectLit init whose inferred struct
    // carries a `Type::Undefined` field: pre-bound
    // `const d1 = { value: undefined }; f(d1)` (arg param typed
    // `any`) otherwise walks the struct → `box_to_any` refcounted
    // arm and the callee's `desc.value` read collapses to null
    // (Type::Undefined field storage is ptr-null, so the any-lane
    // sees ANY_NULL=0). Widening the binding to `any` routes it
    // through the dobj lane (mirrors chunk 5's inline-ObjectLit fn
    // arg fix at the call site — this is the pre-bound analog).
    if unannotated
        && matches!(ast.get_expr(init), Expr::ObjectLit { .. })
        && (checker.dynobj_degraded.contains(&init) || struct_has_undef_field(&ty))
    {
        Type::Any
    } else if checker.any_promoted_inits.contains(&init) {
        // S2.35 — pass_2 promoted this toplevel binding to an Any
        // global (call-init / method-objlit shared verdict); the
        // main binding widens to match, or member calls off the
        // main home would route to mono sigs the boxed slot value
        // can never satisfy.
        Type::Any
    } else if checker.cross_type_widened.contains(&init) {
        // RFC 20260804-mutable-let-widen — a later reassign of a
        // different syntactic family (JS `let it = new C();
        // it = Iterator.from(it)`) makes the binding `any` from
        // declaration; the lowerer consults the same set.
        Type::Any
    } else {
        ty
    }
}

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
enum SelfRef {
    None,
    /// Typed off the annotation or the closure's own signature.
    Provisional,
    /// Typed off the initializer's skeleton; the declaration must
    /// arrive at the same type, since the closure bodies were checked
    /// against it.
    Skeleton(Type),
}

fn predeclare_self_ref_closure(
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
        if !nested.iter().any(|c| *c == name) {
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
                let ty = widen_binding_ty(checker, ast, init, true, ty);
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
    let info = LocalInfo {
        ty,
        mutable: true,
        moved: false,
        borrowed: false,
        declared_class: None,
        builtin_mv: false,
    };
    match checker.declare(name.to_string(), info) {
        Ok(_) => verdict,
        Err(_) => SelfRef::None,
    }
}

/// The checker mirror of the lowering's mint gate (`ssa_lower_member
/// ::try_lower_str_method_value`): init is a Member read whose
/// receiver types to a builtin prototype family (String / Number /
/// Boolean) and whose own type is a Function, and the name interns
/// to a builtin method id with a spec meta row.
fn is_builtin_mv_init(checker: &mut Checker, ast: &Ast, init: ExprId) -> bool {
    let Expr::Member { obj, name } = ast.get_expr(init) else {
        return false;
    };
    // A namespace static (`const f = String.fromCharCode`) is the
    // other half of the family — same reified-cell lowering, same
    // reason its signature must not gate the call.
    if let Expr::Ident(ns) = ast.get_expr(*obj)
        && torajs_rc::ns_static::ns_static_id(ns, name) >= 0
        && matches!(checker.type_of(ast, init), Ok(Type::Function(..)))
    {
        return true;
    }
    let recv_ok = checker
        .type_of(ast, *obj)
        .is_ok_and(|t| crate::ssa_lower_member::mv_family_of_checker_ty(&t).is_some());
    if !recv_ok || !matches!(checker.type_of(ast, init), Ok(Type::Function(..))) {
        return false;
    }
    let mid = torajs_rc::any_method_id(name);
    mid != torajs_rc::ANY_METHOD_UNKNOWN && torajs_rc::any_method_meta(mid).is_some()
}

/// 2026-07-16 (rotation 121 chunk 5-followup) — an ObjectLit init
/// carrying a `Type::Undefined` field can't safely stay as a
/// `Type::Struct` binding: any `box_to_any` at a subsequent coerce
/// point (fn arg into an `any` param, etc.) collapses each
/// Type::Undefined slot to ANY_NULL (ptr-null storage), stranding
/// the spec-correct `undefined` semantics. Widening to `any` routes
/// the init through the dobj lane so undef fields keep their
/// ANY_UNDEF tag. Top-level fields only — nested struct inference
/// is left untouched until a probe motivates it.
fn struct_has_undef_field(t: &Type) -> bool {
    matches!(t, Type::Struct(fs) if fs.iter().any(|(_, ft)| matches!(ft, Type::Undefined)))
}

/// Chunk 702 — TS contextual typing for array-literal inits: after the
/// annotation is verified assignable, the literal's recorded type IS
/// the annotation, all the way down through nested literals. Before
/// this, `const anyz: any[][] = [[2]]` left the inner literal typed
/// `Array<Number>` (pure inference), so lowering minted a
/// typed-behind-any block and a kind-change mutator
/// (`anyz[0].unshift("y")`) hit the catchable-TypeError protocol that
/// exists to protect typed-ALIAS any-views — a literal has no typed
/// alias, so bun's accept semantics apply. Overwriting is
/// equal-or-widening only: assignability was just verified, and for a
/// non-Any annotation the contextual type matches the inference.
/// Spread elements aren't literals — recursion simply skips them (the
/// spread lowering keeps its own element-type derivation).
///
/// `pub(crate)` since rotation 412: a literal Array ARGUMENT against
/// a declared `T[]` param is the same contextual-typing story
/// ([`crate::check_type_of_call::general`] calls this per admitted
/// arg) — without it a uniform-kind literal against an `any[]` param
/// minted the typed flavor and the callee's Arr<Any> readers
/// mis-decoded the slots.
pub(crate) fn apply_contextual_array_ann(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    ann: &Type,
) {
    let Type::Array(elem_ann) = ann else { return };
    let Expr::Array(elements) = ast.get_expr(eid) else {
        return;
    };
    checker.expr_types.insert(eid, ann.clone());
    // The lowering flavor gate keys off this side-set, NOT off
    // expr_types — infer-widened Array<Any> shapes (`["a",
    // undefined]`) share the type but belong to the typed lane.
    if matches!(**elem_ann, Type::Any) {
        checker.contextual_any_literals.insert(eid);
    }
    for &el in elements {
        apply_contextual_array_ann(checker, ast, el, elem_ann);
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
            // An `any` annotation absorbs the literal (chunk-809
            // any-ann family; rotation 73 L3b): `const e: any = []`
            // is a plain Arr<Any> boxed into the any slot — bun
            // accepts. Only a non-array, non-any annotation is a
            // real mismatch.
            if matches!(t, Type::Any) {
                return Some(Type::Array(Box::new(Type::Any)));
            }
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
