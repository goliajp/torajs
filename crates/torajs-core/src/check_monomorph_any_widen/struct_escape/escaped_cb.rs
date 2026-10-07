//! The twins of the callbacks a typed array hands its elements to once
//! that array has escaped into the `any` world.
//!
//! `const u: any = d; u[1] = "s"; d.map(v => typeof v)`: the width
//! analysis puts `d` in an alias class that reaches an `any` slot, so it
//! lowers as an `Arr<Any>` and its elements are whatever the alias
//! stored, while the checker still reads `d` as `number[]` and types
//! `v` as `number`. The typed callback sites (`typed_sites.rs`) would
//! leave such a callback without a twin; here it gets one, and the
//! iteration lowering hands an element the callback's slot cannot hold
//! to the callback's boxed entry, whose tag test calls the twin
//! (§10.2.1.3 binds an argument as it is: `typeof v` on "s" is
//! "string"). An `any[]` receiver hands its callbacks the same values.
//! A function expression reading `this` keeps its receiver head on the
//! twin, and a function declaration passed by name stands there as its
//! relay shell, which takes the shell's twin pair (`named_cb`). The
//! analysis runs over the AST the twins are appended to, so the driver
//! (`check_monomorph::finish`) asks again after every mint until no
//! escaped callback lacks one.

use std::collections::{HashMap, HashSet, VecDeque};

use super::{closure, is_twin, mint_pair, named_cb, twin_name, widenable_target};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Checker;
use crate::check_monomorph::WorkItem;
use crate::num_width::WidthTable;
use crate::ssa_lower_generics_monomorph::Generics;

/// Mint the twin of every lifted closure passed to an iteration over an
/// escaped array or an `any[]` that has none yet, and the twin pair of
/// every relay shell there, a function declaration passed by name
/// becoming its shell first (`named_cb`). A shell is tried once:
/// `tried` keeps the ones a past round tried. True when anything
/// changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn mint_escaped_callback_twins(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
    retired: &HashSet<String>,
    tried: &mut HashSet<String>,
    widths: &WidthTable,
) -> bool {
    let sites = named_cb::any_elem_sites(c, owned_ast, widths);
    let mut changed = named_cb::wrap_named_callbacks(c, owned_ast, widths, &sites);
    for shell in shells_at(c, owned_ast, &sites) {
        if tried.insert(shell.clone()) {
            changed |= mint_shell_pair(
                c,
                owned_ast,
                generics,
                cache,
                worklist,
                call_retargets,
                mono_decls,
                &shell,
            );
        }
    }
    let names = escaped_callbacks(c, owned_ast, &sites);
    if names.is_empty() {
        return changed;
    }
    closure::mint(
        c,
        owned_ast,
        generics,
        cache,
        worklist,
        call_retargets,
        mono_decls,
        retired,
        names,
        true,
    ) || changed
}

/// The twin pair of `shell` over its target. True when a twin was
/// added.
#[allow(clippy::too_many_arguments)]
fn mint_shell_pair(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
    shell: &str,
) -> bool {
    let Some(target) = crate::ast::shell_target(shell).map(str::to_string) else {
        return false;
    };
    let recv = shell.starts_with("__fwdrecv_");
    let Some(found) = widenable_target(c, owned_ast, &target, recv) else {
        return false;
    };
    mint_pair(
        c,
        owned_ast,
        generics,
        cache,
        worklist,
        call_retargets,
        mono_decls,
        shell,
        &target,
        &found.params,
    )
}

/// The relay shells, in arena order, standing at `sites` without a
/// twin.
fn shells_at(c: &Checker, ast: &Ast, sites: &[(ExprId, ExprId)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for &(_, cb) in sites {
        if let Expr::Closure { fn_name, .. } = ast.get_expr(cb)
            && crate::ast::is_shell(fn_name)
            && !is_twin(fn_name)
            && !c.globals.contains_key(&twin_name(fn_name))
            && !out.contains(fn_name)
        {
            out.push(fn_name.clone());
        }
    }
    out
}

/// The lifted closures, in arena order, passed as the callback at
/// `sites`, without a twin. A relay shell takes the pair instead.
fn escaped_callbacks(c: &Checker, ast: &Ast, sites: &[(ExprId, ExprId)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for &(_, cb) in sites {
        if let Expr::Closure { fn_name, .. } = ast.get_expr(cb)
            && !crate::ast::is_shell(fn_name)
            && !is_twin(fn_name)
            && !c.globals.contains_key(&twin_name(fn_name))
            && !out.contains(fn_name)
        {
            out.push(fn_name.clone());
        }
    }
    out
}
