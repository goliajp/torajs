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
    let init_ty = match pick_ary_destr_lane(checker, ast, init, &init_ty) {
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
            if !is_assignable_to_resolved(
                &ann_ty,
                &init_ty,
                &checker.class_structs,
                &checker.aliases,
                &checker.generic_alias_decls,
            ) {
                checker.errors.push_err(format!(
                    "type mismatch on `{name}`: declared {ann_ty:?}, init has {init_ty:?}"
                ));
                return;
            }
            apply_contextual_array_ann(checker, ast, init, &ann_ty);
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

/// RFC 20260714-dstr-residual blade 3 — decide which lane an array
/// binding pattern's group temp takes, and answer the type the temp
/// binds when that lane is the iterator one.
///
/// The parse-time desugar reads a pattern's elements by index, which is
/// right for an Array (and for a String, whose per-code-unit index walk
/// is the same deviation the runtime iteration kernel already
/// documents) and wrong for everything else: ES §13.15.5.3 destructures
/// through the iterator protocol, so a generator, a Map / Set, a class
/// instance with `[Symbol.iterator]()`, or any of those behind `any` has
/// to be STEPPED, not indexed. Before this, an `any` source silently
/// read `undefined` out of every slot and a typed generator source was
/// rejected outright (`no member .0 on Struct([__gen_nominal_g, …])`).
///
/// The verdict is recorded against the group temp's init and the temp
/// binds `Array<Any>` — the shape the lowerer materializes the walk
/// into, so every index read below it stays exactly as desugared.
fn pick_ary_destr_lane(
    checker: &mut Checker,
    ast: &Ast,
    init: ExprId,
    init_ty: &Type,
) -> Option<Type> {
    let limit = *ast.ary_destr_groups.get(&init)?;
    if matches!(init_ty, Type::Array(_) | Type::String) {
        return None;
    }
    checker.iter_destr_srcs.insert(init, limit);
    Some(Type::Array(Box::new(Type::Any)))
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
fn apply_contextual_array_ann(checker: &mut Checker, ast: &Ast, eid: ExprId, ann: &Type) {
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
