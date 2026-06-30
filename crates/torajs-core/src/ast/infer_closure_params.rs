//! Backward inference of anonymous-closure param/return type annotations
//! from the Array-method call site that consumes them.
//!
//! Chunk 342 — extracted from ast.rs. The pass + its two helper walkers
//! (`collect_let_anns` for explicit let annotations, `collect_let_init_anns`
//! for literal-init shape inference) form one logical unit and lift
//! cleanly into a single sibling. ast.rs re-exports `infer_anonymous_closure_params`
//! so external callers (`crates/torajs-cli/src/{cmd_build,main,lsp}.rs`)
//! keep their existing `ast::infer_anonymous_closure_params` path.

use super::{Ast, Expr, ExprId, Stmt};

/// Walk `body` collecting `let name = <init>` shapes where the init is a
/// literal whose type can be inferred (number / string / boolean / array
/// of any of those). Populates `out` with `name → "T[]" / "T"` strings,
/// matching the format used by `infer_anonymous_closure_params`'s
/// `infer_lit_ann` helper. Used so unannotated top-level lets still feed
/// the closure-param inference pass.
fn collect_let_init_anns(
    ast: &Ast,
    body: &[Stmt],
    out: &mut std::collections::HashMap<String, String>,
) {
    fn ann_of(ast: &Ast, eid: ExprId) -> Option<String> {
        match ast.get_expr(eid) {
            Expr::Number(_) => Some("number".into()),
            Expr::String(_) => Some("string".into()),
            Expr::Bool(_) => Some("boolean".into()),
            Expr::Array(els) if !els.is_empty() => {
                ann_of(ast, els[0]).map(|inner| format!("{inner}[]"))
            }
            _ => None,
        }
    }
    for s in body {
        match s {
            Stmt::LetDecl {
                name,
                type_ann: None,
                init,
                ..
            } => {
                if let Some(ann) = ann_of(ast, *init) {
                    out.insert(name.clone(), ann);
                }
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => collect_let_init_anns(ast, stmts, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_let_init_anns(ast, std::slice::from_ref(then_branch.as_ref()), out);
                if let Some(eb) = else_branch {
                    collect_let_init_anns(ast, std::slice::from_ref(eb.as_ref()), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_let_init_anns(ast, std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_let_init_anns(ast, std::slice::from_ref(i.as_ref()), out);
                }
                collect_let_init_anns(ast, std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

fn collect_let_anns(body: &[Stmt], out: &mut std::collections::HashMap<String, String>) {
    for s in body {
        match s {
            Stmt::LetDecl {
                name,
                type_ann: Some(ann),
                ..
            } => {
                out.insert(name.clone(), ann.clone());
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => collect_let_anns(stmts, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_let_anns(std::slice::from_ref(then_branch.as_ref()), out);
                if let Some(eb) = else_branch {
                    collect_let_anns(std::slice::from_ref(eb.as_ref()), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_let_anns(std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_let_anns(std::slice::from_ref(i.as_ref()), out);
                }
                collect_let_anns(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

/// Backward-infer the param type annotations of anonymous arrow
/// closures from the call site that consumes them. Runs after
/// `lift_arrow_fns` so each arrow is now a top-level FnDecl named
/// `__closure_<N>`; un-annotated params would later trip
/// `build_fn_type` with "parameter `a` requires a type annotation".
///
/// Inference rules (narrow MVP):
///   - Look for `Expr::Call { callee = Member(obj, method), args }`.
///   - For each arg that is an `Expr::Closure { fn_name }` with a
///     lifted FnDecl whose params lack annotations, look up the
///     receiver's type via the surrounding fn's let-decls + params.
///   - If the receiver type is `T[]` and the method is one of the
///     known callback-bearing Array methods (`sort` / `map` /
///     `filter` / `reduce` / `forEach` / `find` / `findIndex` /
///     `findLast` / `findLastIndex` / `some` / `every` / `flatMap`),
///     write the inferred per-position type annotations into the
///     lifted FnDecl.
///
/// Anything outside this rule (callbacks on non-Array receivers,
/// callbacks on un-annotated locals, etc.) keeps requiring explicit
/// type annotations.
pub fn infer_anonymous_closure_params(ast: &mut Ast) {
    use std::collections::HashMap;

    // Build per-fn name → param/let type-annotation table. Walk all
    // top-level FnDecl bodies, gathering let-decl names and param
    // names. The same name may appear in multiple fns; we key by the
    // enclosing fn so call-site inference resolves the right binding.
    //
    // Side-effect-free: just reads the AST, populates a name→ann map.
    let mut all_anns: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = s {
            for p in params {
                if let Some(ann) = &p.type_ann {
                    all_anns.insert(p.name.clone(), ann.clone());
                }
            }
            collect_let_anns(body, &mut all_anns);
        }
    }
    // V3-18 m1.h.23 — also walk top-level let decls. The synthetic
    // `main` wraps these at ssa_lower time, but at this AST pass
    // they're sitting at ast.stmts level, so the FnDecl-only walk
    // above missed them. Without this, `let arr = [1,2,3]; arr.find(x => ...)`
    // can't infer x's type.
    collect_let_anns(&ast.stmts, &mut all_anns);
    // Inferred-from-init shape: `let arr = [<lit>, ...]` infers
    // arr's annotation as "<lit_ty>[]" so .map / .filter / etc on
    // unannotated lets still get param inference.
    let mut inferred_inits: HashMap<String, String> = HashMap::new();
    collect_let_init_anns(ast, &ast.stmts, &mut inferred_inits);
    for (k, v) in inferred_inits {
        all_anns.entry(k).or_insert(v);
    }

    // Map from lifted closure fn_name → (param annotations, return
    // annotation). Filled by walking call sites; applied at the end
    // (deferred so we don't mutate ast.stmts mid-walk).
    let mut updates: HashMap<String, (Vec<String>, String)> = HashMap::new();

    let n = ast.exprs.len();
    for i in 0..n {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let callee = *callee;
        let args = args.clone();
        // Member(obj, method) with at least one Closure arg.
        let Expr::Member { obj, name } = ast.get_expr(callee).clone() else {
            continue;
        };
        let mut closure_args: Vec<(usize, String)> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            // Two shapes after lift_arrow_fns:
            //   - `Expr::Closure { fn_name, captures }` for arrows that
            //     captured outer-scope bindings.
            //   - `Expr::Ident(fn_name)` for arrows with no captures
            //     (lift emits a bare ident pointing at the lifted
            //     FnDecl). Both cases must be probed for inference.
            match ast.get_expr(*a) {
                Expr::Closure { fn_name, .. } => {
                    closure_args.push((i, fn_name.clone()));
                }
                Expr::Ident(n) if n.starts_with("__closure_") => {
                    closure_args.push((i, n.clone()));
                }
                _ => {}
            }
        }
        if closure_args.is_empty() {
            continue;
        }
        /* Resolve obj's type ann.
         * - `Ident(n)`        → look up in the all_anns table built
         *                       from FnDecl params + let-decl annotations.
         * - `Array(els)`      → infer `T[]` from els[0]'s shape (literal
         *                       receiver path — `[1,2,3].map(x => ...)`).
         *                       Empty literal can't infer an element type;
         *                       skipped. Only homogeneous-typed literals
         *                       matter here since the existing `T[]` infra
         *                       requires homogeneous elements.
         * - `String`          → "string"
         * - `Number`          → "number"
         * Anything more exotic falls through unchanged (caller relies on
         * an explicit annotation upstream). */
        fn infer_lit_ann(ast: &Ast, eid: ExprId) -> Option<String> {
            match ast.get_expr(eid) {
                Expr::Number(_) => Some("number".into()),
                Expr::String(_) => Some("string".into()),
                Expr::Bool(_) => Some("boolean".into()),
                Expr::Array(els) if !els.is_empty() => {
                    /* Recurse on first element to get its inferred ann,
                     * then suffix with []. Fails (returns None) if the
                     * first element isn't a recognized literal shape. */
                    infer_lit_ann(ast, els[0]).map(|inner| format!("{inner}[]"))
                }
                _ => None,
            }
        }
        let obj_ann = match ast.get_expr(obj) {
            Expr::Ident(n) => all_anns.get(n).cloned(),
            other => {
                let _ = other;
                infer_lit_ann(ast, obj)
            }
        };
        let Some(ann) = obj_ann else { continue };
        // Only handle T[] receivers for the known Array methods.
        let Some(elem_ann) = ann.strip_suffix("[]") else {
            continue;
        };
        let elem_ann = elem_ann.to_string();
        // Per-method expected (param annotations, return annotation).
        let expected: Option<(Vec<String>, String)> = match name.as_str() {
            "sort" => Some((vec![elem_ann.clone(), elem_ann.clone()], "number".into())),
            "map" => Some((vec![elem_ann.clone()], elem_ann.clone())),
            "filter" => Some((vec![elem_ann.clone()], "boolean".into())),
            "forEach" => Some((vec![elem_ann.clone()], "void".into())),
            "find" | "findLast" => Some((vec![elem_ann.clone()], "boolean".into())),
            "findIndex" | "findLastIndex" => Some((vec![elem_ann.clone()], "boolean".into())),
            "some" | "every" => Some((vec![elem_ann.clone()], "boolean".into())),
            "flatMap" => {
                // Return is `T[]` (flattened); inner cb returns array.
                Some((vec![elem_ann.clone()], format!("{elem_ann}[]")))
            }
            "reduce" | "reduceRight" => {
                // (acc, cur) => acc — caller supplies the seed; without
                // type-tracking the seed type, assume elem-typed accum
                // (works for sum/max/etc.).
                Some((vec![elem_ann.clone(), elem_ann.clone()], elem_ann.clone()))
            }
            _ => None,
        };
        let Some(expected) = expected else { continue };
        for (_arg_idx, fn_name) in &closure_args {
            updates.insert(fn_name.clone(), expected.clone());
        }
    }

    if updates.is_empty() {
        return;
    }

    // Apply updates: mutate each lifted FnDecl's params + return type.
    for stmt in &mut ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } = stmt
            && let Some((new_param_anns, new_ret_ann)) = updates.get(name)
        {
            // First param of a lifted closure is `__env`; user params
            // start at index 1.
            let user_start = if params.first().is_some_and(|p| p.name == "__env") {
                1
            } else {
                0
            };
            for (i, ann) in new_param_anns.iter().enumerate() {
                let pidx = user_start + i;
                if let Some(p) = params.get_mut(pidx)
                    && p.type_ann.is_none()
                {
                    p.type_ann = Some(ann.clone());
                }
            }
            if return_type.is_none() {
                *return_type = Some(new_ret_ann.clone());
            }
        }
    }
}
