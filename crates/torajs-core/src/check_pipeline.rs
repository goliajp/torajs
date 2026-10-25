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
use crate::check::{
    Checker, DiagPush, GenericAliasMap, Type, build_fn_type_full, resolve_type_ann,
};
use crate::check_type_ann::{
    resolve_type_ann_decl, resolve_type_ann_field, resolve_type_ann_full, unknown_type,
};
use std::collections::HashMap;

mod synth_spelling;
use synth_spelling::synthesized_spelling;

/// Pass 0 — install the alias tables read off `ast.stmts`; see
/// [`collect_type_alias_tables`] for the two phases.
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
    let tables = collect_type_alias_tables(ast);
    c.aliases = tables.aliases;
    c.generic_alias_decls = tables.generic_alias_decls;
    c.class_structs = tables.class_structs;
    for msg in tables.errors {
        c.errors.push_err(msg);
    }
}

/// The type tables a program's declarations alone determine — what
/// [`pass_0_register_type_aliases`] installs on the checker, and what
/// the class desugar reads to tell a struct-shaped parameter
/// annotation before any checker exists.
pub(crate) struct TypeAliasTables {
    pub(crate) aliases: HashMap<String, Type>,
    pub(crate) generic_alias_decls: GenericAliasMap,
    pub(crate) class_structs: HashMap<String, Type>,
    pub(crate) errors: Vec<String>,
}

/// Register every `Stmt::TypeDecl` into `aliases` (or
/// `generic_alias_decls` for generic ones). Two-phase to support
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
/// Read the tables off `ast.stmts`. A class whose `TypeDecl` is not
/// in the list yet (the desugar reads these mid-pass, while classes
/// behind the current one are still `ClassDecl`s) keeps its
/// `ClassRef` placeholder from `class_parents`.
pub(crate) fn collect_type_alias_tables(ast: &Ast) -> TypeAliasTables {
    let mut t = TypeAliasTables {
        aliases: HashMap::new(),
        generic_alias_decls: GenericAliasMap::new(),
        class_structs: HashMap::new(),
        errors: Vec::new(),
    };
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
            if t.aliases.contains_key(name) || t.generic_alias_decls.contains_key(name) {
                continue;
            }
            t.aliases.insert(name.clone(), Type::ClassRef(name.clone()));
            placeholder_classes.insert(name.clone());
        }
    }
    let declared: std::collections::HashSet<&String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::TypeDecl { name, .. } => Some(name),
            _ => None,
        })
        .collect();
    for name in ast.class_parents.keys() {
        if !declared.contains(name) {
            t.aliases.insert(name.clone(), Type::ClassRef(name.clone()));
        }
    }

    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
        {
            if (t.aliases.contains_key(name) && !placeholder_classes.contains(name))
                || t.generic_alias_decls.contains_key(name)
            {
                t.errors.push(format!("redeclaration of type `{name}`"));
                continue;
            }
            if !type_params.is_empty() {
                t.generic_alias_decls.insert(
                    name.clone(),
                    (
                        type_params.clone(),
                        fields.clone(),
                        ast.class_parents.contains_key(name),
                        crate::check::StructFields::declared_optional(ast, name, fields),
                    ),
                );
                continue;
            }
            // the literal types the declaration wrote, which its AST
            // fields spell widened (`check_type_ann::literal`)
            let precise = crate::check_type_ann::precise_decl_fields(ast, name, fields);
            let fields = precise.unwrap_or(fields);
            if fields.len() == 1 && fields[0].0 == "__alias__" {
                let alias_ann = &fields[0].1;
                match resolve_type_ann_decl(alias_ann, &t.aliases, &[], &t.generic_alias_decls) {
                    Some(ty) => {
                        t.aliases.insert(name.clone(), ty);
                    }
                    None => {
                        let msg = unknown_type(alias_ann);
                        t.errors.push(format!("{msg} for type alias `{name}`"));
                    }
                }
                continue;
            }
            let mut field_tys: Vec<(PropKey, Type)> = Vec::new();
            let mut had_err = false;
            for (fname, fty_ann) in fields {
                let resolved = if precise.is_some() {
                    resolve_type_ann_field(fty_ann, &t.aliases, &[], &t.generic_alias_decls)
                } else {
                    resolve_type_ann_full(fty_ann, &t.aliases, &[], &t.generic_alias_decls)
                };
                match resolved {
                    Some(ty) => field_tys.push((fname.clone(), ty)),
                    None => {
                        let msg = unknown_type(fty_ann);
                        t.errors
                            .push(format!("{msg} for field `{}` of `{name}`", fname.lossy()));
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
                let resolved =
                    Type::Struct(crate::check::StructFields::declared(ast, name, field_tys));
                if ast.class_parents.contains_key(name) {
                    t.class_structs.insert(name.clone(), resolved);
                } else {
                    t.aliases.insert(name.clone(), resolved);
                }
            }
        }
    }
    t
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
            // A ctor on the argv lane carries it at position 2, after
            // its `__this` / `__new_target` head (rotation 616), so for
            // a ctor the strip is by name. A METHOD on the argv lane
            // (position 1) keeps its slot visible: the private-method
            // machinery reads that face (`arguments-method-value-argv`
            // went red when the strip was widened to every shape).
            let stripped: Vec<Param>;
            // A one-slot this-first body on the argv lane (a static
            // twin or a promoted declaration, rotation 627) strips
            // like a ctor: it is called directly.
            if crate::check_stmt_fn_decl::argv_slot_hidden(name, user_params) {
                stripped = user_params
                    .iter()
                    .filter(|p| p.name != "__torajs_argv")
                    .cloned()
                    .collect();
                user_params = &stripped;
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
            let literal_init = lit_ty.is_some();
            let ann_ty = match (lit_ty.clone(), type_ann) {
                // Chunk 809 — an explicit annotation wins over the
                // literal's shape: `let a: any = "s"` registers Any,
                // so a named-fn `a = 42` typechecks like bun runs it
                // (the lit_ty fallback registered String and rejected
                // every cross-type write). An unresolvable ann falls
                // back to the literal's type as before.
                (Some(lit), Some(ann)) => resolve_type_ann(ann, &c.aliases).or(Some(lit)),
                (None, Some(ann)) => resolve_type_ann(ann, &c.aliases).map(|t| {
                    crate::check_stmt_let_decl::any_layout_slot::declared_face(c, ast, *init, t)
                }),
                (None, None) => {
                    match synthesized_spelling(c, ast, name, *init, *is_var, &binding_refs) {
                        Some((spelling, widened)) => {
                            // A widened init's main binding becomes
                            // `any` too (`let_widen`): both homes must
                            // agree with the boxed slot.
                            if widened {
                                c.any_promoted_inits.insert(*init);
                            }
                            let ty = resolve_type_ann(&spelling, &c.aliases);
                            if ty.is_some() {
                                c.global_spellings.insert(name.clone(), spelling);
                            }
                            ty
                        }
                        None => None,
                    }
                }
                _ => lit_ty,
            };
            // A literal init registers its literal's type, an annotated
            // one its annotation; the lowerer still wants the synthesized
            // spelling for both — the mutable literal's slot, and the
            // fallback for an annotation naming a layout the literal's
            // accessor slots contradict.
            if (literal_init || type_ann.is_some())
                && let Some((spelling, _)) =
                    synthesized_spelling(c, ast, name, *init, *is_var, &binding_refs)
                && resolve_type_ann(&spelling, &c.aliases).is_some()
            {
                c.global_spellings.insert(name.clone(), spelling);
            }
            if let Some(ty) = ann_ty
                && !c.globals.contains_key(name)
            {
                c.globals.insert(name.clone(), ty);
            }
        }
    }

    let saved_hoists = crate::check_hoist_closure_lets::enter_module(c, ast, &ast.stmts);
    crate::check_gen_env::check_stmts(c, ast);
    c.hoisted_closure_lets = saved_hoists;
}
