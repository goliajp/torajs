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

pub(crate) mod any_layout_slot;
pub(crate) mod any_scalar_slot;
pub(crate) mod contextual_array;
pub(crate) mod fn_face_slot;
mod fresh_map;
pub(crate) mod held_any;
pub(crate) use contextual_array::{apply_contextual_array_ann, apply_contextual_array_ann_store};

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, DiagPush, LocalInfo, Type, face_slots};
use crate::check_assignable::is_assignable_to_resolved;
use crate::check_stmt_let_decl_destr_lane::pick_ary_destr_lane;
use crate::check_stmt_let_decl_selfref::{SelfRef, predeclare_self_ref_closure};
use crate::check_type_ann::{resolve_type_ann_decl, resolve_type_ann_full, unknown_type};
use fn_face_slot::record_let_face;
use fresh_map::{map_ty_from_type_args, record_fresh_map_ty};

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
        let typed = if type_ann.is_some() {
            checker.type_of_raw(ast, init)
        } else {
            checker
                .type_of(ast, init)
                .map(|t| fn_union_face(checker, init, t))
        };
        match typed {
            Ok(t) => t,
            Err(e) => {
                checker.errors.push_err(e);
                return;
            }
        }
    };
    let init_ty = widen_escaping_generic(checker, ast, init, init_ty);
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
    // RFC 20260714-dstr-residual blade 3 — an array pattern's group temp
    // takes its lane here, where the source type is known.
    let stepped = pick_ary_destr_lane(checker, ast, Some(name), init, &init_ty);
    let is_stepped = stepped.is_some();
    let init_ty = stepped.unwrap_or(init_ty);
    let mut face_slot = false;
    let mut ret_slot = false;
    let final_ty = match type_ann {
        None => {
            ret_slot = face_slots::ret_slot_let(checker, ast, (name, mutable), init, &init_ty);
            init_ty
        }
        Some(ann) => {
            let Some(ann_ty) =
                resolve_type_ann_decl(ann, &checker.aliases, &[], &checker.generic_alias_decls)
            else {
                checker.errors.push_err(unknown_type(ann));
                return;
            };
            let init_ty =
                crate::check_assignable::in_declared_order(checker, ast, &ann_ty, init, init_ty);
            if held_any::face::refuses_init(checker, ast, name, init, &ann_ty) {
                return;
            }
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
            // An initializer carrying accessor slots is compared
            // through the property each one implements, and KEEPS its
            // own layout — see `check_accessor_objlit_slot` for why
            // adopting the annotation instead turns a refusal into a
            // closure pointer printed as a number.
            let ordinary = fn_slot_widened
                || is_assignable_to_resolved(
                    &ann_ty,
                    &init_ty,
                    &checker.class_structs,
                    &checker.aliases,
                    &checker.generic_alias_decls,
                );
            // Only where the ordinary comparison FAILS, which is what
            // keeps an `any` annotation winning: it admits the literal
            // outright, so the projection never runs and the binding
            // stays the box its dynamic member lane expects.
            let accessor_layout = !ordinary
                && crate::check_accessor_objlit_slot::admits(
                    &ann_ty,
                    &init_ty,
                    &checker.class_structs,
                    &checker.aliases,
                    &checker.generic_alias_decls,
                );
            if !ordinary && !accessor_layout {
                checker.errors.push_err(format!(
                    "type mismatch on `{name}`: declared {ann_ty:?}, init has {init_ty:?}"
                ));
                return;
            }
            let tys = (&init_ty, &ann_ty);
            face_slot = record_let_face(checker, ast, (ann, init, mutable), tys, fn_slot_widened);
            face_slots::record_objlit_stores(checker, ast, &ann_ty, init);
            if accessor_layout {
                // Pushing the annotation's element type onto the
                // literal is the same adoption by another route: it
                // retypes each element to the data-field shape, and
                // `xs[0].v` reads the getter closure as a number.
                init_ty
            } else {
                apply_contextual_array_ann(checker, ast, init, &ann_ty);
                any_layout_slot::declared_face(checker, ast, init, ann_ty)
            }
        }
    };
    let final_ty = widen_binding_ty(checker, ast, init, type_ann.as_deref(), final_ty);
    record_fresh_map_ty(checker, ast, init, &final_ty);
    if let SelfRef::Skeleton(provisional) = &self_ref_closure
        && *provisional != final_ty
    {
        checker.errors.push_err(format!(
            "internal: `{name}` pre-declared as {provisional:?} for its initializer's closures, declared {final_ty:?}"
        ));
    }
    let held_any = any_layout_slot::holds_any(
        &final_ty,
        type_ann.as_deref(),
        init,
        ast,
        &checker.expr_types,
    );
    if held_any && checker.scopes.len() == 1 {
        checker.held_any.globals.insert(name.to_string());
    }
    any_scalar_slot::note_held(checker, name, type_ann.as_deref(), init);
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
    let exec_result = crate::check_exec_record::init_is_record(checker, ast, type_ann, init);
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
            mutable,
            borrowed: is_alias_init,
            declared_class,
            builtin_mv,
            exec_result,
            fn_slot: (((ast.fnsig_mismatch_bindings.contains(name) || face_slot)
                && type_ann.as_deref().is_some_and(crate::ast::is_fn_like_ann))
                || ret_slot)
                .then_some(crate::check::FnSlot::Let(init)),
            union: const_literal(ast, mutable || is_stepped, type_ann, init),
            held_any,
            ann: type_ann.clone(),
            decl: Some(crate::check::FnSlot::Let(init)),
            ..LocalInfo::plain(final_ty)
        },
    ) {
        checker.errors.push_err(e);
    }
    // An initializer is the binding's first assignment — see
    // `Checker::apply_decl_narrow`.
    checker.apply_decl_narrow(ast, name, init);
}

/// L3b ③ — a generic fn escaping as a VALUE (`const g = f` where
/// `f`'s face still carries un-inferred TypeVars from the
/// implicit-generics pass): the binding's direct call is a
/// CallIndirect with no mono channel, so the TypeVar face rejects
/// every argument. Widen the face all-TypeVar→Any and record the
/// site; `check_monomorph_any_widen` clones an all-`any` spec and
/// the ident lowering takes ITS address (the generic original is a
/// checker template that never lowers).
fn widen_escaping_generic(checker: &mut Checker, ast: &Ast, init: ExprId, init_ty: Type) -> Type {
    match (&init_ty, ast.get_expr(init)) {
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
    }
}

/// An unannotated binding initialized with a choice among functions of
/// different signatures has their union as its type, as TS infers it:
/// the value still travels as `any`, and a call through the binding is
/// checked against the members (`check_type_of_call::union_callee`).
fn fn_union_face(checker: &Checker, init: ExprId, t: Type) -> Type {
    match checker.unions.types.get(&init) {
        Some(u)
            if t == Type::Any
                && crate::check_type_of_call::union_callee::fn_members(u).is_some() =>
        {
            u.clone()
        }
        _ => t,
    }
}

/// An unannotated `const` initialized with a literal keeps the literal
/// type (TS §3.2.10); a `let` widens it, and so does a group temp the
/// iterator lane rebinds to the stepped array: a string literal kept
/// there made its index reads look like string indexing.
fn const_literal(
    ast: &Ast,
    mutable: bool,
    type_ann: &Option<String>,
    init: ExprId,
) -> crate::check_local_info::UnionBinding {
    let literal = (!mutable && type_ann.is_none())
        .then(|| crate::check::Lit::of_expr(ast.get_expr(init)))
        .flatten()
        .map(Type::Literal);
    crate::check_local_info::UnionBinding {
        literal,
        ..Default::default()
    }
}

/// The widenings a binding's type takes after its initializer types —
/// shared by the declaration and by the skeleton pre-declare, which
/// must answer the same type the declaration will.
pub(crate) fn widen_binding_ty(
    checker: &Checker,
    ast: &Ast,
    init: ExprId,
    ann: Option<&str>,
    ty: Type,
) -> Type {
    let ty = if ann.is_none() {
        map_ty_from_type_args(checker, ast, init, ty)
    } else {
        ty
    };
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
    if ann.is_none()
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
    } else if let Some(t) = any_layout_slot::cast_face(&ty, init, ast, &checker.expr_types) {
        t
    } else if ann.is_none() && checker.cross_type_widened.contains(&init) {
        // RFC 20260804-mutable-let-widen — a later reassign of a
        // different syntactic family (JS `let it = new C();
        // it = Iterator.from(it)`) makes the binding `any` from
        // declaration; the lowerer consults the same set.
        Type::Any
    } else {
        ty
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
                checker.errors.push_err(unknown_type(ann));
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
