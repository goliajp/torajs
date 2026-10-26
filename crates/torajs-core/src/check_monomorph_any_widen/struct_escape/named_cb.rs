//! A function declaration passed by name to an iteration that hands
//! its callback `any` elements.
//!
//! `d.map(show)` over a typed array whose alias escaped into the `any`
//! world (or over an `any[]`) feeds `show` whatever the array holds.
//! A declaration has no boxed entry and no twin, so the iteration lane
//! called it through its typed face, which converted every element to
//! the parameter type on the way in: `typeof v` on "s" answered
//! "number" (§10.2.1.3 binds an argument as it is). At such a site the
//! name is replaced by its relay shell `__forward_<name>`, the value
//! the declaration already is everywhere else it is read as one, and
//! the shell gets the twin pair a shell reached through `any` gets
//! (`struct_escape::mint_pair`): its boxed entry runs the typed body on
//! a fitting element and the all-`any` twin on any other. A typed
//! receiver keeps the direct call of the declaration.

use super::{is_twin, widenable_target};
use crate::ast::{Ast, Expr, ExprId, ShellFace, Stmt};
use crate::check::{Checker, Type};
use crate::num_width::WidthTable;

/// The callback sites of iterations that hand their callback `any`
/// elements, or a reducer an `any` seed: `(receiver, callback)` in
/// arena order.
pub(super) fn any_elem_sites(c: &Checker, ast: &Ast, widths: &WidthTable) -> Vec<(ExprId, ExprId)> {
    let mut out = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Some(recv) = crate::num_width::hof_receivers::iterated_array(ast, *callee, args) else {
            continue;
        };
        let cb = if args.first() == Some(&recv) {
            args.get(1)
        } else {
            args.first()
        };
        let any_arr = matches!(c.expr_types.get(&recv), Some(Type::Array(el)) if **el == Type::Any);
        if let Some(&cb) = cb
            && (widths.hof_receiver_escapes(recv)
                || any_arr
                || any_reduce_seed(c, ast, *callee, args)
                || mapset_fn_slot(c, recv, cb))
        {
            out.push((recv, cb));
        }
    }
    out
}

/// `m.forEach(cb)` on a Map or Set whose callback declares a fn-typed
/// parameter: the walk hands every key, value and member as a box
/// (the payload is `any` at every site), and no box carries the typed
/// entry such a parameter calls through.
fn mapset_fn_slot(c: &Checker, recv: ExprId, cb: ExprId) -> bool {
    matches!(c.expr_types.get(&recv), Some(Type::Map(..) | Type::Set))
        && matches!(c.expr_types.get(&cb),
            Some(Type::Function(ps, _)) if ps.iter().any(|p| matches!(p, Type::Function(..))))
}

/// `xs.reduce(cb, seed)` with an `any` seed: the first step binds
/// whatever the seed holds to the callback's typed accumulator
/// parameter (§23.1.3.24 step 8.b.iii passes it as it is), and every
/// later step binds what that callback answered. A callback answering
/// `any` hands every later step a value of any kind the same way.
fn any_reduce_seed(c: &Checker, ast: &Ast, callee: ExprId, args: &[ExprId]) -> bool {
    matches!(ast.get_expr(callee), Expr::Member { name, .. } if name == "reduce" || name == "reduceRight")
        && (args
            .get(1)
            .is_some_and(|s| c.expr_types.get(s) == Some(&Type::Any))
            || matches!(args.first().and_then(|cb| c.expr_types.get(cb)),
                Some(Type::Function(_, ret)) if **ret == Type::Any))
}

/// Replace each function declaration named at `sites` by its relay
/// shell, minting the shell where none exists. True when a site was
/// rewritten: the width table read before is stale then, even when the
/// shell's twin pair already exists.
pub(super) fn wrap_named_callbacks(
    c: &mut Checker,
    ast: &mut Ast,
    widths: &WidthTable,
    sites: &[(ExprId, ExprId)],
) -> bool {
    let mut rewrote = false;
    for &(_, cb) in sites {
        let Expr::Ident(name) = ast.get_expr(cb).clone() else {
            continue;
        };
        if !widths.bound_closures(cb).is_empty()
            || !wrappable(c, ast, &name)
            || shadowed_at(ast, &name, cb)
        {
            continue;
        }
        let shell = format!("__forward_{name}");
        let prior = ast.get_expr(cb).clone();
        ast.exprs[cb.0 as usize] = Expr::Closure {
            fn_name: shell.clone(),
            captures: Vec::new(),
        };
        if !declared(ast, &shell) && !mint_shell(c, ast, &shell, &name, cb) {
            ast.exprs[cb.0 as usize] = prior;
            continue;
        }
        rewrote = true;
    }
    rewrote
}

/// Is `name` a declaration the struct-escape lane can twin
/// ([`widenable_target`])? A rest parameter, or a store of its own
/// values into a binding outside the body (a typed slot would take the
/// twin's `any` value converted), keeps the declaration on its typed
/// face.
fn wrappable(c: &Checker, ast: &Ast, name: &str) -> bool {
    let plain = ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name: n, params, .. }
            if n == name
                && params.first().is_none_or(|p| p.name != "__env")
                && !params.iter().any(|p| p.is_rest)
                && !crate::ast::free_vars_write::stores_own_value_outside(ast, s))
    });
    plain
        && !is_twin(name)
        && !crate::ast::is_shell(name)
        && !ast.async_generator_fns.contains(name)
        && matches!(c.globals.get(name), Some(Type::Function(..)))
        && widenable_target(c, ast, name, false).is_some()
}

/// Can the read `site` of `name` resolve to a binding other than the
/// top-level declaration? The arena is flat, so the statement tree
/// answers: a block, or a top-level function's parameter list, that
/// binds `name` owns every read under it, and so does a lifted body
/// capturing `name`; a nested function's parameter, a catch or loop
/// variable, a `var` or a top-level redeclaration is refused for the
/// whole name.
fn shadowed_at(ast: &Ast, name: &str, site: ExprId) -> bool {
    use crate::ast::namedfn_rebound_scope as scope;
    scope::rebound_beyond_blocks(&ast.stmts, name)
        || scope::block_scoped_expr_ids(ast, name).contains(&site)
        || scope::captured_expr_ids(ast, name).contains(&site)
}

fn declared(ast: &Ast, name: &str) -> bool {
    ast.stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name: n, .. } if n == name))
}

/// `__forward_<target>`, minted after the check the way the desugar
/// mints it before, and checked at its construction site `site`. A
/// shell that does not check is taken back out.
fn mint_shell(c: &mut Checker, ast: &mut Ast, shell: &str, target: &str, site: ExprId) -> bool {
    let Some((params, ret, span)) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            return_type,
            span,
            ..
        } if name == target => Some((params.clone(), return_type.clone(), *span)),
        _ => None,
    }) else {
        return false;
    };
    let Some(face) = c.globals.get(target).cloned() else {
        return false;
    };
    let decl =
        crate::ast::synth_forwarder_decl(ast, shell, target, &params, ret, span, ShellFace::Plain);
    ast.stmts.push(decl);
    c.globals.insert(shell.to_string(), face);
    c.closure_fn_names.insert(shell.to_string());
    if let Some(d) = c.fn_defaults.get(target).cloned() {
        c.fn_defaults.insert(shell.to_string(), d);
    }
    let saved_errors = c.errors.len();
    let checked = crate::check_type_of_fn::check_closure_body(c, ast, site, shell, Vec::new());
    if checked.is_ok() && c.errors.len() == saved_errors {
        return true;
    }
    c.errors.truncate(saved_errors);
    ast.stmts
        .retain(|s| !matches!(s, Stmt::FnDecl { name, .. } if name == shell));
    c.globals.remove(shell);
    c.closure_fn_names.remove(shell);
    c.fn_defaults.remove(shell);
    false
}
