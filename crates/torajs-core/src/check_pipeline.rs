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
                c.generic_alias_decls
                    .insert(name.clone(), (type_params.clone(), fields.clone()));
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
            let mut field_tys: Vec<(String, Type)> = Vec::new();
            let mut had_err = false;
            for (fname, fty_ann) in fields {
                match resolve_type_ann_full(fty_ann, &c.aliases, &[], &c.generic_alias_decls) {
                    Some(ty) => field_tys.push((fname.clone(), ty)),
                    None => {
                        c.errors.push_err(format!(
                            "unknown type `{fty_ann}` for field `{fname}` of `{name}`"
                        ));
                        had_err = true;
                        break;
                    }
                }
            }
            if !had_err {
                c.aliases.insert(name.clone(), Type::Struct(field_tys));
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
            let user_params: &[Param] = if is_closure { &params[1..] } else { params };
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
    for stmt in &ast.stmts {
        if let Stmt::LetDecl {
            name,
            init,
            type_ann,
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
                (None, Some(ann)) => resolve_type_ann(ann, &c.aliases),
                (None, None) => {
                    if binding_refs.named_fn_refs.contains(name)
                        && !binding_refs.closure_captured.contains(name)
                    {
                        crate::ast_refs::infer_toplevel_slot_shape(ast, *init).map(|s| match s {
                            crate::ast_refs::GlobalSlotShape::I64
                            | crate::ast_refs::GlobalSlotShape::F64 => Type::Number,
                            crate::ast_refs::GlobalSlotShape::Str => Type::String,
                            crate::ast_refs::GlobalSlotShape::Bool => Type::Boolean,
                        })
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

    for stmt in &ast.stmts {
        if let Stmt::FnDecl { name, .. } = stmt
            && (c.closure_fn_names.contains(name) || c.generic_type_params.contains_key(name))
        {
            continue;
        }
        c.check_stmt(ast, stmt);
    }
}
