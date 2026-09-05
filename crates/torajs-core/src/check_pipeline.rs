//! Three pass-orchestration sub-fns extracted out of
//! [`crate::check::Checker::run_full_pipeline`] (chunk 136).
//!
//! The pre-extract `run_full_pipeline` body was 233 LOC (over the
//! 200-line god-fn hard limit). Splitting into the three native
//! passes (`Pass 0` register type aliases / `Pass 1` hoist fn
//! signatures / `Pre-pass` register literal globals + `Pass 2`
//! check stmts) brings the orchestrator under the limit and lets
//! each pass live with its own dedicated doc.
//!
//! Each pass owns a `&mut Checker` borrow + `&Ast` for the whole
//! call; the orchestrator runs them strictly sequentially because
//! later passes consume the side effects of earlier ones
//! (`Pass 1` reads `c.aliases` populated by `Pass 0`; `Pass 2`
//! reads `c.globals` populated by `Pass 1` and the pre-pass).

use crate::ast::PropKey;
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::{Checker, DiagPush, Type, build_fn_type_full, resolve_type_ann};
use crate::check_type_ann::resolve_type_ann_full;

/// Pass 0 — register every `Stmt::TypeDecl` into `c.aliases` (or
/// `c.generic_alias_decls` for generic ones). Two-phase to support
/// self-references and forward-references:
///
/// - **Phase 1** inserts a `Type::ClassRef(name)` placeholder for
///   every non-generic class TypeDecl before resolving any field
///   types. This lets [`resolve_type_ann_full`] succeed when a
///   class field references its own class
///   (`class Node { next: Node | null }`) or a forward-declared
///   sibling (`class A { b: B } class B { a: A }`) — both of which
///   would otherwise error because the class wasn't in `c.aliases`
///   when its field types were being resolved.
/// - **Phase 2** replaces each placeholder with the resolved fields.
///   Downstream consumers (`Member`-access type-of, `Assign` lhs/rhs
///   unify, etc.) index `c.aliases` by name on every read, so they
///   always see the post-replacement struct, not the placeholder.
///
/// Generic type aliases (`type Pair<A, B> = { ... }`) skip the
/// placeholder pass — they go straight into
/// `c.generic_alias_decls`, instantiated lazily by
/// [`crate::check::resolve_type_ann_with_vars`] when it sees a
/// `Pair<X|Y>` syntax site.
///
/// The "__alias__" wedge (V3-18 single-field bare type alias from
/// the parser) skips placeholder reservation and resolves to the
/// underlying type directly.
pub(crate) fn pass_0_register_type_aliases(c: &mut Checker, ast: &Ast) {
    // r505 (A12) — every declared class, before any body is checked:
    // the `__class_<C>` / `__proto_<C>` reads resolve against it.
    c.class_names = ast.class_parents.keys().cloned().collect();
    c.source_dunder_idents = ast.source_dunder_idents.clone();
    let mut placeholder_classes: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name, type_params, ..
        } = stmt
        {
            if !type_params.is_empty() {
                continue;
            }
            if c.aliases.contains_key(name) || c.generic_alias_decls.contains_key(name) {
                continue;
            }
            c.aliases.insert(name.clone(), Type::ClassRef(name.clone()));
            placeholder_classes.insert(name.clone());
        }
    }

    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
        {
            if (c.aliases.contains_key(name) && !placeholder_classes.contains(name))
                || c.generic_alias_decls.contains_key(name)
            {
                c.errors.push_err(format!("redeclaration of type `{name}`"));
                continue;
            }
            if !type_params.is_empty() {
                c.generic_alias_decls.insert(
                    name.clone(),
                    (
                        type_params.clone(),
                        fields.clone(),
                        ast.class_parents.contains_key(name),
                    ),
                );
                continue;
            }
            if fields.len() == 1 && fields[0].0 == "__alias__" {
                let alias_ann = &fields[0].1;
                match resolve_type_ann_full(alias_ann, &c.aliases, &[], &c.generic_alias_decls) {
                    Some(ty) => {
                        c.aliases.insert(name.clone(), ty);
                    }
                    None => {
                        c.errors.push_err(format!(
                            "unknown type `{alias_ann}` for type alias `{name}`"
                        ));
                    }
                }
                continue;
            }
            let mut field_tys: Vec<(PropKey, Type)> = Vec::new();
            let mut had_err = false;
            for (fname, fty_ann) in fields {
                match resolve_type_ann_full(fty_ann, &c.aliases, &[], &c.generic_alias_decls) {
                    Some(ty) => field_tys.push((fname.clone(), ty)),
                    None => {
                        c.errors.push_err(format!(
                            "unknown type `{fty_ann}` for field `{}` of `{name}`",
                            fname.lossy()
                        ));
                        had_err = true;
                        break;
                    }
                }
            }
            if !had_err {
                // RFC 20260715-nominal-class-identity — a CLASS keeps
                // its `ClassRef(name)` placeholder in `aliases`, so a
                // `let c: C` annotation resolves to the class's NAME.
                // Collapsing it to the field struct here is what let an
                // object literal of the same shape inherit the class's
                // accessors and methods (both sides asked "which class
                // has my shape?"). Structural consumers reach the shape
                // through `resolve_class_ref`, which reads this map.
                // A plain `type P = {...}` alias has no nominal identity
                // to keep — it resolves to the struct as before.
                let resolved = Type::Struct(field_tys);
                if ast.class_parents.contains_key(name) {
                    c.class_structs.insert(name.clone(), resolved);
                } else {
                    c.aliases.insert(name.clone(), resolved);
                }
            }
        }
    }
}

/// Pass 1 — hoist every top-level `Stmt::FnDecl`'s user-visible
/// signature into `c.globals`.
///
/// - **Lifted-closure FnDecls** (first param `__env`) drop the env
///   slot from the user-visible signature: callers see
///   `(real_params...) -> ret`. The full signature stays implicit
///   at the SSA layer.
/// - **Generic FnDecls** (non-empty `type_params`) get their
///   signature stored with TypeVar placeholders; call-site
///   inference instantiates them.
/// - **Defaults** — per-param default `ExprId`s are recorded into
///   `c.fn_defaults` for caller-side default substitution. None
///   positions are required args; the first non-None marks the
///   start of the optional tail (JS spec — defaults must be
///   trailing).
pub(crate) fn pass_1_hoist_fn_signatures(c: &mut Checker, ast: &Ast) {
    for stmt in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type,
            ..
        } = stmt
        {
            let is_closure = params.first().is_some_and(|p| p.name == "__env");
            let mut user_params: &[Param] = if is_closure { &params[1..] } else { params };
            // RFC 20260816-headless-argv-face — a head-less argv
            // body carries the synthetic raw-argv pointer at
            // position 0 (the head-less degeneration of "right after
            // the head"). It is an ABI slot the direct-call terminal
            // fills, never a user argument, so the caller-visible
            // signature drops it exactly as the env slot drops above.
            if user_params
                .first()
                .is_some_and(|p| p.name == "__torajs_argv")
            {
                user_params = &user_params[1..];
            }
            match build_fn_type_full(
                name,
                user_params,
                return_type,
                &c.aliases,
                type_params,
                &c.generic_alias_decls,
            ) {
                Ok(ty) => {
                    if c.globals.contains_key(name) {
                        c.errors
                            .push_err(format!("redeclaration of function `{name}`"));
                    } else {
                        c.globals.insert(name.clone(), ty);
                        if is_closure {
                            c.closure_fn_names.insert(name.clone());
                        }
                        if !type_params.is_empty() {
                            c.generic_type_params
                                .insert(name.clone(), type_params.clone());
                        }
                        let defaults: Vec<Option<ExprId>> =
                            user_params.iter().map(|p| p.default).collect();
                        if defaults.iter().any(|d| d.is_some()) {
                            c.fn_defaults.insert(name.clone(), defaults);
                        }
                    }
                }
                Err(e) => c.errors.push_err(e),
            }
        }
    }
}

/// Pre-pass + Pass 2 combined orchestrator.
///
/// **Pre-pass** — register top-level `const X = LITERAL` (Number /
/// String / Boolean) into `c.globals` so named-fn bodies can read
/// them at typecheck time. tr's lower path emits the literal inline
/// at every reference; non-literal initializers stay scoped to the
/// implicit main fn (alloca'd there, not visible from named-fn
/// bodies).
///
/// Phase K.3 — non-literal init with explicit annotation becomes a
/// real LLVM data global; record the annotated type so named-fn
/// bodies typecheck reads + writes against it.
///
/// Phase K.3b — un-annotated non-literal init: register exactly
/// what `ssa_lower`'s Pass 1.5 will promote, via the shared
/// `ast_refs` gate + slot-shape inference. Anything looser lets
/// programs typecheck whose lowering still aborts with
/// `unknown ident`; anything tighter brings back the bogus
/// `unknown identifier` on legal named-fn reads of call-init
/// globals.
///
/// **Pass 2** — walk every statement through `c.check_stmt`.
/// Closure-lifted FnDecls are skipped: their bodies are checked
/// lazily by the `Expr::Closure` arm of `type_of` (with captures
/// injected as locals). Generic FnDecls are also skipped: their
/// TypeVar-bearing bodies can't be checked without substitution;
/// the SSA monomorphization pass produces concrete bodies on demand,
/// and call-site inference validates that arguments stay consistent
/// with each TypeVar instance.
pub(crate) fn pass_2_register_globals_and_check_stmts(c: &mut Checker, ast: &Ast) {
    let binding_refs = crate::ast_refs::toplevel_binding_refs(ast);
    // Multi-flattened walk (rotation 230) — multi-declarator lets
    // live inside Stmt::Multi and must register like flat ones; the
    // lowerer's collect_toplevel_globals walks the same flat view
    // (the no-drift contract both sides document).
    for stmt in crate::ast::toplevel_stmts_flat(ast) {
        if let Stmt::LetDecl {
            name,
            init,
            type_ann,
            is_var,
            ..
        } = stmt
        {
            let lit_ty = match ast.get_expr(*init) {
                Expr::Number(_) => Some(Type::Number),
                Expr::BigInt { .. } => Some(Type::BigInt),
                Expr::String(_) => Some(Type::String),
                Expr::Bool(_) => Some(Type::Boolean),
                _ => None,
            };
            let ann_ty = match (lit_ty.clone(), type_ann) {
                // Chunk 809 — an explicit annotation wins over the
                // literal's shape: `let a: any = "s"` registers Any,
                // so a named-fn `a = 42` typechecks like bun runs it
                // (the lit_ty fallback registered String and rejected
                // every cross-type write). An unresolvable ann falls
                // back to the literal's type as before.
                (Some(lit), Some(ann)) => resolve_type_ann(ann, &c.aliases).or(Some(lit)),
                (None, Some(ann)) => resolve_type_ann(ann, &c.aliases),
                (None, None) => {
                    // Chunk 737 — immutable closure-captured bindings
                    // register too (the lowerer's capture filter
                    // resolves them to the global; mirror of the
                    // inferred_slot_ty gate). Chunk 740 — mutable
                    // captured bindings join: the capture filter gives
                    // the lifted body GlobalRef reads AND the Assign-
                    // Ident global lane gives it writes, so the global
                    // is the single home (the old env-copy snapshot
                    // disagreed with ES shared-binding semantics).
                    if binding_refs.named_fn_refs.contains(name) {
                        // Rotation 204 — a dynobj-degraded ObjectLit
                        // init registers Any, making a degraded
                        // binding named-fn-visible exactly like a
                        // `: any` annotation (the degrade IS the
                        // any-lane routing decision; see
                        // `crate::dynobj_degrade`). `var` stays
                        // main-local — the lowerer's promote loop
                        // only takes `is_var: false`, and a
                        // checker-only registration would typecheck
                        // programs whose lowering still aborts.
                        if !*is_var && c.dynobj_degraded.contains(init) {
                            Some(Type::Any)
                        }
                        // RFC 20260709-closure-global chunk 2 — an
                        // un-annotated lifted-arrow init registers
                        // under the sig synthesized from the lifted
                        // FnDecl's (preinfer-backfilled) anns, the
                        // same `__fn(...)` spelling the annotated
                        // lane resolves. Mutable bindings register
                        // too (chunk 730: the Assign-Ident lane owns
                        // drop-old/store-new, mirroring the lowerer's
                        // mutable_promote gate). Variadic sigs stay
                        // main-local (boxed-dual routing is a
                        // fn-local table — RFC O2).
                        else if let Expr::Closure { fn_name, .. } = ast.get_expr(*init) {
                            if *is_var {
                                None
                            } else {
                                crate::ast_refs::lifted_closure_fn_canon(ast, fn_name)
                                    .filter(|canon| !canon.contains("__rest("))
                                    .and_then(|canon| resolve_type_ann(&canon, &c.aliases))
                            }
                        }
                        // Rotation 592 — an un-annotated ALIAS of a
                        // lifted-arrow binding (`const c = k`)
                        // registers under that binding's own
                        // spelling. The alias holds the identical
                        // value, and both sides resolve the identical
                        // string (`closure_alias_fn_canon`), so the
                        // slots cannot drift. Without it a named-fn
                        // read of the alias answered "unknown
                        // identifier" while the same program written
                        // `const c: any = k` worked.
                        else if !*is_var
                            && let Some(canon) =
                                crate::ast_refs::closure_alias_fn_canon(ast, *init)
                            && !canon.contains("__rest(")
                        {
                            resolve_type_ann(&canon, &c.aliases)
                        }
                        // RFC 20260725 follow-up — an un-annotated
                        // all-literal ObjectLit init registers under
                        // its synthesized `__inlobj(...)` spelling
                        // (the lowerer's K.3b arm resolves the same
                        // string, so the slots can't drift). `var`
                        // stays main-local like every arm here.
                        else if !*is_var
                            && let Some(ann) =
                                crate::ast_refs::objlit_literal_inlobj_ann(ast, *init)
                        {
                            resolve_type_ann(&ann, &c.aliases)
                        }
                        // Cluster-`values` follow-up (rotation 253) —
                        // an un-annotated all-literal Array init
                        // registers under its synthesized `T[]`
                        // spelling (the lowerer's K.3b arm resolves
                        // the same string, so the slots can't
                        // drift). `var` reaches here already
                        // converted by the hoist escape hatch
                        // (Array inits keep their typed slot), so
                        // the is_var gate mirrors the arms above.
                        else if !*is_var
                            && let Some(ann) =
                                crate::ast_refs_arrlit::arrlit_literal_elem_ann(ast, *init)
                        {
                            resolve_type_ann(&ann, &c.aliases)
                        }
                        // An un-annotated `new C()` init registers
                        // under its class spelling (the lowerer's
                        // K.3b arm resolves the same string, so the
                        // slots can't drift). Its own nominal type,
                        // not Any: `any_promote_init` refuses class
                        // instances so main-side method calls keep
                        // the typed lanes, and before this arm that
                        // refusal left the binding unregistered —
                        // every named-fn read of a `let e = new
                        // Error()` answered "unknown identifier".
                        else if !*is_var
                            && let Some(ann) = crate::ast_refs::new_class_ann(ast, *init)
                        {
                            resolve_type_ann(&ann, &c.aliases)
                        } else {
                            let shaped = crate::ast_refs::infer_toplevel_slot_shape(ast, *init)
                                .map(|s| match s {
                                    crate::ast_refs::GlobalSlotShape::I64
                                    | crate::ast_refs::GlobalSlotShape::F64 => Type::Number,
                                    crate::ast_refs::GlobalSlotShape::Str => Type::String,
                                    crate::ast_refs::GlobalSlotShape::Bool => Type::Boolean,
                                    crate::ast_refs::GlobalSlotShape::Symbol => Type::Symbol,
                                    crate::ast_refs::GlobalSlotShape::BigInt => Type::BigInt,
                                });
                            // S2.35 — a call-result init the shape
                            // inference can't type (the test262
                            // IIFE-iterator idiom) registers Any via
                            // the shared verdict
                            // (`ast_refs_any_promote`; the lowerer's
                            // inferred_slot_ty consults the same fn
                            // in the same fallback position). The
                            // eid is recorded so the LetDecl arm
                            // widens the main binding to Any too —
                            // both homes must agree with the boxed
                            // slot. Shape-typed calls (simple ret
                            // ann / `Symbol()`) keep their exact
                            // slot — the fallback never demotes.
                            if shaped.is_none()
                                && !*is_var
                                && crate::ast_refs_any_promote::any_promote_init(ast, *init)
                            {
                                c.any_promoted_inits.insert(*init);
                                Some(Type::Any)
                            } else {
                                shaped
                            }
                        }
                    } else {
                        None
                    }
                }
                _ => lit_ty,
            };
            if let Some(ty) = ann_ty
                && !c.globals.contains_key(name)
            {
                c.globals.insert(name.clone(), ty);
            }
        }
    }

    let saved_hoists = crate::check_hoist_closure_lets::enter(c, ast, &ast.stmts);
    for stmt in &ast.stmts {
        if let Stmt::FnDecl { name, .. } = stmt
            && (c.closure_fn_names.contains(name) || c.generic_type_params.contains_key(name))
        {
            continue;
        }
        c.check_stmt(ast, stmt);
    }
    c.hoisted_closure_lets = saved_hoists;
}
