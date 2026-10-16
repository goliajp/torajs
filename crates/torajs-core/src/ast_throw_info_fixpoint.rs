//! The transitive may-throw fixed point over the program's FnDecls —
//! moved out of [`crate::ast_throw_info`] as its own sibling (rotation
//! 550, pure motion): that file answers one fn's throw shape
//! (`fn_throw_info` and the stmt / expr scans), this one closes the
//! per-fn answers over the call graph.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ast_throw_info::fn_throw_info;
use std::collections::{HashMap, HashSet};

/// One fn's throw shape; `nonint_read` and `code_write` are settled
/// program-wide (`compute_may_throw_fns`).
pub(crate) struct FnThrow {
    pub(crate) direct: bool,
    pub(crate) called: Vec<String>,
    pub(crate) nonint_read: bool,
    pub(crate) code_write: bool,
}

/// M4.3.b — the transitive may-throw set: collect each FnDecl's
/// `(direct_throw, called_names)` then iterate to fixed-point — a fn
/// is may_throw if it throws directly OR calls any may_throw fn.
/// Also answers whether a non-integral keyed read can run code at
/// all; the lowering checks after such a read on that same answer,
/// so a fn never raises where its callers were told it cannot.
pub(crate) fn compute_may_throw_fns(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    widths: &crate::num_width::WidthTable,
) -> (HashSet<String>, bool) {
    let mut may_throw: HashSet<String> = HashSet::new();
    let mut decl_throw_info: Vec<(String, bool, Vec<String>)> = Vec::new();
    let assigned = assigned_names(ast);
    // Rotation 550 (550-01) — a call through a let-bound arrow
    // (`const boom = () => { throw … }; boom()`) records the BINDING
    // name, but the body lives in the lifted `__closure_N` FnDecl;
    // resolving through the same alias table the param-tag / infer
    // rounds use (549-02) lets the fixed point below reach it.
    // Without it every callback whose only throw source was such a
    // call was judged never-throwing, the HOF loop pruned its check,
    // and the pending throw strayed into the NEXT checked call
    // (`for … try { a(i).map(x => x + boom()) } catch` caught once
    // in three).
    let aliases = crate::ast_closure_param_tag_collect::closure_let_aliases(ast);
    let fn_lets = fn_typed_lets(ast, expr_types, &aliases);
    let mut nonint_readers: Vec<String> = Vec::new();
    let mut props_run_code = props_run_code_outside_fns(ast, expr_types, widths, &assigned);
    for stmt in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            body,
            return_type,
            span,
            ..
        } = stmt
        {
            let info = fn_throw_info(ast, name, params, body, expr_types, widths, &assigned);
            let (mut direct, mut called) = (info.direct, info.called);
            props_run_code |= info.code_write;
            if info.nonint_read {
                nonint_readers.push(name.clone());
            }
            let ret = return_type.as_deref();
            direct |= crate::ast_throw_info::returns_any_into_obj(
                ast, name, ret, *span, body, expr_types,
            );
            // RFC 20260922 S0 — a checked cast can raise at any typed
            // boundary, which this scan does not model; the measurement
            // gate keeps every caller's check instead.
            direct |= crate::ssa_lower_checked_cast::gate_on();
            // Rotation 552 (551-04) — a call through a fn-valued let
            // the fixed point cannot resolve is a call to a statically
            // unknown target: conservative may-throw, the same rule
            // `scan_call` applies to a fn-typed param or body let.
            if called.iter().any(|c| fn_lets.contains(c)) {
                direct = true;
            }
            if direct {
                may_throw.insert(name.clone());
            }
            let lifted: Vec<String> = called
                .iter()
                .filter_map(|c| aliases.get(c))
                .flatten()
                .map(|(f, _)| f.clone())
                .collect();
            called.extend(lifted);
            // Rotation 507 — a `__dispatch_<M>` stub's AST body only
            // forwards to the base owner, but the slot it stands for
            // resolves to EVERY owner's body at runtime: a throwing
            // override behind a base-typed call was invisible here, so
            // the caller of a fn that only called the stub never
            // checked, printed the ret sentinel 0 and ran on (probe:
            // `viaParam(new Other(13))` — the uncaught error surfaced
            // at exit instead of in the enclosing try).
            if let Some(m) = name.strip_prefix("__dispatch_") {
                let (bare, suffix) = m
                    .split_once("$$")
                    .map(|(b, s)| (b, format!("$${s}")))
                    .unwrap_or((m, String::new()));
                for o in ast.method_owners.get(bare).into_iter().flatten() {
                    called.push(format!("__cm_{o}__{bare}"));
                    called.push(format!("__cm_{o}__{bare}{suffix}"));
                }
            }
            decl_throw_info.push((name.clone(), direct, called));
        }
    }
    if props_run_code {
        may_throw.extend(nonint_readers);
    }
    loop {
        let mut grew = false;
        for (name, _direct, called) in &decl_throw_info {
            if may_throw.contains(name) {
                continue;
            }
            for c in called {
                if may_throw.contains(c) {
                    may_throw.insert(name.clone());
                    grew = true;
                    break;
                }
            }
        }
        if !grew {
            break;
        }
    }
    (may_throw, props_run_code)
}

/// A non-integral keyed read raises only where some named property on
/// an array can run code: a prototype / accessor spelling
/// (`Ast::array_prop_reads_may_raise`) or a keyed store that can leave
/// a code-bearing value in one (`Scan::code_write`). This answers for
/// everything but the FnDecls, whose own scans add their stores: the
/// top-level statements are no FnDecl of their own.
fn props_run_code_outside_fns(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    widths: &crate::num_width::WidthTable,
    assigned: &HashSet<String>,
) -> bool {
    if ast.array_prop_reads_may_raise {
        return true;
    }
    let mut top = crate::ast_throw_info::Scan::new(ast, "", &[], expr_types, widths, assigned);
    top.stmts(&ast.stmts);
    top.code_write
}

/// Names some assignment or `++` / `--` anywhere in the program
/// writes. A narrowing (`if (t === undefined) return; t.k`) holds only
/// for a binding whose value the test saw is the value the read
/// sees, and a name written anywhere — later in the sequence, in a
/// loop body, inside a closure the binding is captured by — is not
/// one; program-wide by name is the conservative side.
fn assigned_names(ast: &Ast) -> HashSet<String> {
    ast.exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                match ast.get_expr(*target) {
                    Expr::Ident(n) => Some(n.clone()),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// Rotation 552 (551-04) — fn-typed `let`s whose init is NOT a closure
/// literal (`let f = cond ? boom : mk0`, `let g = pick(1)`). A call
/// through such a name reaches nothing the fixed point can follow: it
/// is neither a FnDecl nor an alias of a lifted closure, and outside
/// the declaring body it is not in that body's `fn_values` either —
/// so a callback whose only throw source was `f()` was judged
/// never-throwing, the HOF loop pruned its check, and the pending
/// throw strayed into the next checked call (`for … try {
/// a(i).map(x => f()) } catch` caught 300000 of 600000). Program-wide
/// by name, the same grade as [`closure_let_aliases`]; a collision
/// only errs toward one cold throw-check.
///
/// [`closure_let_aliases`]: crate::ast_closure_param_tag_collect::closure_let_aliases
fn fn_typed_lets(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    aliases: &crate::ast_closure_param_tag_collect::ClosureAliases,
) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::LetDecl { name, init, .. } = s
            && !aliases.contains_key(name)
            && matches!(expr_types.get(init), Some(crate::check::Type::Function(..)))
        {
            out.insert(name.clone());
        }
        crate::ast_closure_param_tag::push_child_stmts(s, &mut stack);
    }
    out
}
