//! Arrow-fn lambda-lift + class-factory helpers — chunk 360,
//! extracted from ast.rs.
//!
//! Pub entry `lift_arrow_fns` (M2) walks `ast.exprs` in index order
//! replacing each `Expr::ArrowFn` with `Expr::Closure { fn_name,
//! captures }` and appending a lifted `Stmt::FnDecl` (env-first
//! calling convention with a synthesized `__env` param) to
//! `ast.stmts`. Free-var detection delegates to sibling
//! `ast::free_vars`.
//!
//! Cluster-4 class-factory helpers (all `pub(crate)` for sibling
//! access via `super::`):
//!   * `rewrite_this_in_ann` — polymorphic-this substitution used
//!     by `ast::desugar_classes_emit`.
//!   * `default_init_for_field` / `default_init_for_type` — the
//!     recursive default-initializer chain (`field_defaults`), used by
//!     `ast::desugar_classes_field_inits` and by several other ast.rs
//!     passes that seed zero values.
//!   * `method_owner_is_in_chain` — override-detection walker
//!     imported by `ast::desugar_classes_method_owners`.
//!   * `is_fn_like_ann` — closure-ABI slot detector shared by
//!     multiple ast.rs passes.

use super::{Ast, Expr, Param, Stmt};

mod field_defaults;
pub(crate) use field_defaults::{default_init_for_field, default_init_for_type};

/// V3-18 wedge — rewrite the placeholder `"this"` in a class
/// method's return-type annotation to the enclosing class's
/// `this_ann` (e.g., `C` or `C<T|U>` for generic classes), per
/// TS spec §3.6.3 polymorphic-this semantics. Standard usage:
/// class Builder { add(...): this { return this } }
/// The parser stores the literal `"this"` in `m.return_type`;
/// desugar_classes substitutes it here before emit so check.rs
/// and ssa_lower see the concrete class type at every method's
/// return boundary. Also handles the `__nullable(this)` wrapper
/// case for the rare `: this | null` shape.
pub(crate) fn rewrite_this_in_ann(ann: &Option<String>, this_ann: &str) -> Option<String> {
    let a = ann.as_deref()?;
    if a == "this" {
        return Some(this_ann.to_string());
    }
    if a == "__nullable(this)" {
        return Some(format!("__nullable({this_ann})"));
    }
    Some(a.to_string())
}

/// True iff `owner` is `target_ancestor` or any ancestor of `target_ancestor`.
/// Used by the override-detection check.
pub(crate) fn method_owner_is_in_chain(
    parent_map: &std::collections::HashMap<String, Option<String>>,
    owner: &str,
    target_ancestor: &str,
) -> bool {
    if owner == target_ancestor {
        return true;
    }
    let mut cur = parent_map.get(target_ancestor).cloned().flatten();
    while let Some(p) = cur {
        if p == owner {
            return true;
        }
        cur = parent_map.get(&p).cloned().flatten();
    }
    false
}

/// M2 — lambda-lift arrow fns. Walks `ast.exprs` in index order; each
/// `Expr::ArrowFn` is replaced in-place and a corresponding `Stmt::FnDecl`
/// is appended to `ast.stmts`.
///
/// Non-capturing arrows: the source-site expression becomes
/// `Expr::Ident("__closure_N")`, lowering to a plain `FnAddr` in SSA. This
/// is the original M2 Phase A path.
///
/// Capturing arrows (M2 Phase C): the source-site becomes
/// `Expr::Closure { fn_name, captures }`. The lifted FnDecl is given a
/// hidden first parameter named `__env` (typed at the SSA layer); the
/// lowerer reads each capture out of `__env` and binds it as a local at
/// the top of the body, so the body's `Ident(name)` references resolve
/// against the captured value rather than the (now out-of-scope) outer
/// binding.
///
/// Iteration order: parser emits inner expressions before outer, so a
/// nested arrow fn sits at a lower `ExprId` than its enclosing arrow fn.
/// We walk indices low→high; the inner arrow gets lifted first and the
/// outer arrow's body still references it via the (now Ident/Closure) ExprId.
pub fn lift_arrow_fns(ast: &mut Ast) {
    let mut counter = 0u32;
    let mut new_decls: Vec<Stmt> = Vec::new();
    // Top-level FnDecl names are globals — references to them inside an
    // arrow body should not count as captures. Collect once before
    // walking the exprs.
    let global_fn_names: Vec<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    // ...except where an enclosing scope rebinds one of those names.
    // The reference then belongs to the local and the arrow must
    // capture it; pre-binding would hand it to the declaration
    // (RFC 20260828). Computed off the statement tree because this
    // scan is flat and cannot tell which body an arrow sits in.
    // The built-in global names (`fs`, `process`, `Math`, …) are the
    // same question one table over: the walk skips them, so a local
    // `const fs = […]` was never captured and the lifted body died on
    // `unknown ident`. One scan answers both lists.
    let shadow_list: Vec<String> = global_fn_names
        .iter()
        .cloned()
        .chain(
            crate::ast::free_vars_globals::GLOBAL_NAMES
                .iter()
                .map(|n| n.to_string()),
        )
        .collect();
    let shadowed = crate::ast::scope_chain::shadowed_globals_by_arrow(ast, &shadow_list);
    let n = ast.exprs.len();
    for i in 0..n {
        if !matches!(ast.exprs[i], Expr::ArrowFn { .. }) {
            continue;
        }
        let name = format!("__closure_{counter}");
        counter += 1;
        // Chunk 796 — a named function expression carries its
        // self-name into the lifted-closure registry (ES §15.5.5;
        // pass-2B overlays it over any binding name).
        if let Some(sn) = ast.fn_expr_self_names.get(&crate::ast::ExprId(i as u32)) {
            let sn = sn.clone();
            ast.closure_self_names.insert(name.clone(), sn);
        }
        // RFC 20260714-dstr-residual blade 2 — a destructuring
        // default's anonymous fn takes its binding name (§8.4.5);
        // recorded by ExprId at pattern-parse time, carried onto the
        // lifted closure name here (merged in pass-2B before the
        // self-name overlay).
        if let Some(bn) = ast.dstr_default_names.get(&crate::ast::ExprId(i as u32)) {
            let bn = bn.clone();
            ast.closure_dstr_names.insert(name.clone(), bn);
        }
        // RFC 20260721-builtin-method-reflection 刀 4+9 — fn-flavor
        // side-channel onto the lifted name. An async form (arrow or
        // fn-expr; `async_fn_value_exprs` marks both) reflects
        // %AsyncFunction%; a plain fn-expr owns a `.prototype`
        // (generator fn-exprs never reach this walk — hoisted to decl
        // form before lifting).
        let eid = crate::ast::ExprId(i as u32);
        if ast.implicit_rest_arrows.contains(&eid) {
            ast.implicit_rest_fns.insert(name.clone());
        }
        if ast.async_ssa.value_exprs.contains(&eid) {
            ast.async_ssa.fns.insert(name.clone());
        }
        if ast.async_fn_value_exprs.contains(&eid) {
            ast.fn_async_value_fns.insert(name.clone());
        } else if ast.fn_expr_exprs.contains(&eid) {
            ast.fn_proto_fns.insert(name.clone());
        }
        // Compute captures BEFORE moving the arrow body out — collect free
        // vars (idents referenced inside the body that are neither one of
        // the arrow's params nor declared by an inner let, and not a
        // top-level FnDecl name).
        // The shadowed case is rare; the common one must not copy the
        // whole global list per arrow.
        let narrowed: Vec<String>;
        let rebound: Vec<String>;
        let (prebound, rebound_globals): (&[String], &[String]) = match shadowed.get(&eid) {
            Some(sh) => {
                narrowed = crate::ast::scope_chain::not_rebound(&global_fn_names, sh);
                rebound = sh
                    .iter()
                    .filter(|s| !global_fn_names.contains(s))
                    .cloned()
                    .collect();
                (&narrowed, &rebound)
            }
            None => (&global_fn_names, &[]),
        };
        let captures = match &ast.exprs[i] {
            Expr::ArrowFn { params, body, .. } => crate::ast::free_vars::free_vars_of_arrow(
                ast,
                params,
                body,
                prebound,
                rebound_globals,
                ast.closure_self_names.get(&name).map(|s| s.as_str()),
            ),
            _ => Vec::new(),
        };
        // P3.closure-in-struct-field — always produce a Closure value
        // (env-carrying, env-first CallIndirect ABI) regardless of
        // capture count. Zero-capture arrows still get an `__env()`
        // annotation so the lowerer treats them as closure-shaped and
        // the call-site dispatch is uniform with capturing arrows.
        let placeholder = Expr::Closure {
            fn_name: name.clone(),
            captures: captures.clone(),
        };
        let arrow = std::mem::replace(&mut ast.exprs[i], placeholder);
        if let Expr::ArrowFn {
            params,
            return_type,
            body,
        } = arrow
        {
            let mut final_params = params;
            let env_ann = format!("__env({})", captures.join("|"));
            final_params.insert(
                0,
                Param {
                    name: "__env".into(),
                    type_ann: Some(env_ann),
                    default: None,
                    is_rest: false,
                },
            );
            new_decls.push(Stmt::FnDecl {
                name,
                type_params: Vec::new(),
                params: final_params,
                return_type,
                body,
                is_generator: false,
                // the lifted decl compiles the user's arrow / fn
                // expression -- carry its recorded source range (B1b)
                span: ast.expr_spans[i],
            });
        }
    }
    ast.stmts.extend(new_decls);
}

/// Closure ABI slot detector. After `tag_struct_field_closure_types`
/// rewrites TypeDecl fn-like fields to `__cls(P)->(R)`, this returns
/// true exactly when a field's annotation indicates the SSA slot will
/// be Type::Closure. User-source `(P)=>R` / `__fn(P)->(R)` also pass
/// this test for resilience (the desugar passes run before
/// type-checking, so a TypeDecl field that escaped tagging would still
/// trigger an ObjectLit rewrite if needed — defensive).
pub(crate) fn is_fn_like_ann(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("__cls(") || t.starts_with("__fn(") || t.contains("=>") || t.starts_with('(')
}

/// Retag a fn-typed ann sitting in a **struct-field position** from
/// the bare-fn-ptr repr (`__fn(P)->(R)` → `Type::FnSig`) to the closure
/// repr (`__cls(P)->(R)` → `Type::Closure`, env-first CallIndirect).
/// Non-fn anns pass through untouched.
///
/// A field slot is mutable and can receive a capturing closure, so it
/// must be Closure-repr; `__fn(` would intern a bare-ptr slot and the
/// field call would CallIndirect into the closure's env header —
/// SIGBUS. Param / return / let positions keep `__fn(` for direct
/// dispatch on the hot fn-as-callback path.
///
/// Every site that MINTS a struct-field ann must route through here:
/// named `TypeDecl` / `ClassDecl` fields
/// (`tag_struct_field_closure_types`), the parser's syntax-minted
/// `__inlobj(` (`parser::type_ann`), and the return-type inferrer's
/// `__inlobj(` (`implicit_generics_infer`). Each of the three was a
/// separate SIGBUS before it was routed here.
pub(crate) fn retag_field_fn_ann(ann: &str) -> String {
    if let Some(rest) = ann.strip_prefix("__fn(") {
        format!("__cls({rest}")
    } else if let Some(rest) = ann.strip_prefix("__nullable(__fn(") {
        format!("__nullable(__cls({rest}")
    } else {
        ann.to_string()
    }
}

/// Chunk 733 — fn-typed ARRAY annotation detector (`((n)=>n)[]` /
/// `Array<(n)=>n>` spellings, parser-internal `__fn(...)->(R)[]` /
/// `Array<__fn(...)->(R)>`). The SSA `parse_type` re-reprs such an
/// element slot as Closure (mutable position, can hold capturing
/// closures), so a bare top-FnDecl Ident stored into one needs the
/// `__forward_<name>` wrap — the fn-arr axes in
/// `ast_collect_fn_closure` match store-sites against bindings /
/// params carrying an annotation this test accepts.
pub(crate) fn is_fn_arr_ann(s: &str) -> bool {
    let t = s.trim();
    if let Some(rest) = t.strip_suffix("[]") {
        return is_fn_like_ann(rest);
    }
    for head in ["Array<", "ReadonlyArray<", "Iterable<"] {
        if let Some(rest) = t.strip_prefix(head)
            && t.ends_with('>')
        {
            return is_fn_like_ann(&rest[..rest.len() - 1]);
        }
    }
    false
}
