//! Which array callback loops can see their source shrink under them.
//!
//! §23.1.3's callback methods read `length` once and then ask
//! `HasProperty(O, k)` before every visit. On a typed-element cell
//! (`number[]`, `string[]`, …) the only way an index below the
//! original length stops being present is a shorter `length`: the
//! cell refuses accessors, `delete` widens the binding to `any[]`
//! (`delete_arr_widen`), and a sparse tail only ever forms on an Any
//! cell. So on a typed source the whole gate is `k < length now` —
//! but the loop runs without it (a per-step test splits the body and
//! costs the vectorized loop about 30%; `ssa_lower_arr_hole_gate`'s
//! module doc has the measurement), and a callback that shortened its
//! own source kept being called on the cut-off indices:
//! `arr.forEach(function () { arr.length = 3 })` ran five times where
//! the spec runs three, and on a `string[]` it read slots the shrink
//! had already released.
//!
//! This pass answers, per call site, whether the source MAY shrink
//! while the loop runs; the lowering pays the per-step test only
//! there. The question is asked by NAME over the whole program,
//! which is what makes it cheap to get right: the program is scanned
//! once for every length-shortening operation (`x.length = …`,
//! `x.length--`, `x.pop()` / `shift()` / `splice()`, and
//! `Object.defineProperty` / `Reflect.set`-family writes on `x`), and
//! the names they act on are recorded — wherever they sit, however
//! the callback reaches them. A site whose source is spelled by one
//! of those names is gated.
//!
//! The name is the only channel there is to a cell, save two:
//!
//! - the source escapes by value (`let y = xs`, `f(xs)`, `return
//!   xs`, `xs.sort()`'s answer) — then some other name may be the
//!   same cell, and a site on an escaping source is gated whenever
//!   ANY shrink exists;
//! - the callback's own `O` parameter — the method hands the source
//!   to it. That parameter's name is looked up on the callback when
//!   the callback is a declaration the lowering can see; when it is
//!   not (an `any` value, a parameter, a method), the site is gated
//!   whenever any shrink exists.
//!
//! A shrink whose target has no name (`rows[i].length = 0`,
//! `get().pop()`) could be any cell and gates every site.
//!
//! A program with no shrink at all — nearly all of them, every bench
//! case included — gates nothing, and its loops lower exactly as
//! before.

use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashSet;

/// What the scan found. Empty `names` with `unknown_target` false is
/// the common case and answers "never" everywhere.
#[derive(Debug, Clone, Default)]
pub struct HofLenShrinks {
    /// Names (bindings or property names) some operation shortens.
    names: HashSet<String>,
    /// A shortening operation on a target with no name.
    unknown_target: bool,
    /// Names that appear somewhere other than as the base of a
    /// member / index access, a call's callee or an assignment
    /// target — i.e. whose value is handed on.
    escaping: HashSet<String>,
}

const SHRINKING_METHODS: [&str; 3] = ["pop", "shift", "splice"];
/// Methods that answer their receiver: a use of their result is an
/// alias of the source.
const SELF_RETURNING: [&str; 4] = ["sort", "reverse", "fill", "copyWithin"];
const DEFINE_METHODS: [&str; 4] = [
    "defineProperty",
    "defineProperties",
    "set",
    "deleteProperty",
];

pub fn collect_hof_len_shrinks(ast: &mut Ast) {
    let mut out = HofLenShrinks::default();
    let mut based: HashSet<u32> = HashSet::new();
    for e in &ast.exprs {
        note_non_escaping_children(ast, e, &mut based);
        if let Some(target) = shrink_target(ast, e) {
            match name_of(ast, target) {
                Some(n) => {
                    out.names.insert(n.to_string());
                }
                None => out.unknown_target = true,
            }
        }
    }
    if !out.names.is_empty() || out.unknown_target {
        for (i, e) in ast.exprs.iter().enumerate() {
            if let Expr::Ident(n) = e
                && !based.contains(&(i as u32))
            {
                out.escaping.insert(n.clone());
            }
        }
    }
    ast.hof_len_shrinks = out;
}

/// The array expression an operation shortens, if it is one.
fn shrink_target(ast: &Ast, e: &Expr) -> Option<ExprId> {
    let length_of = |t: ExprId| match ast.get_expr(t) {
        Expr::Member { obj, name } if name == "length" => Some(*obj),
        _ => None,
    };
    match e {
        Expr::Assign { target, .. } => length_of(*target),
        Expr::PostIncr { target, .. } => length_of(*target),
        Expr::Call { callee, args } => match ast.get_expr(*callee) {
            Expr::Member { obj, name } if SHRINKING_METHODS.contains(&name.as_str()) => Some(*obj),
            Expr::Member { obj, name }
                if DEFINE_METHODS.contains(&name.as_str())
                    && matches!(ast.get_expr(*obj), Expr::Ident(ns) if ns == "Object" || ns == "Reflect") =>
            {
                args.first().copied()
            }
            _ => None,
        },
        _ => None,
    }
}

/// The name a source or shrink target is spelled with: a binding, or
/// the property a member access reads (`this.items`). `as` is
/// transparent.
fn name_of(ast: &Ast, mut e: ExprId) -> Option<&str> {
    loop {
        match ast.get_expr(e) {
            Expr::As { expr, .. } => e = *expr,
            Expr::Ident(n) => return Some(n),
            Expr::Member { name, .. } => return Some(name),
            _ => return None,
        }
    }
}

fn note_non_escaping_children(ast: &Ast, e: &Expr, based: &mut HashSet<u32>) {
    match e {
        Expr::Member { obj, name } if !SELF_RETURNING.contains(&name.as_str()) => {
            based.insert(obj.0);
        }
        Expr::Index { obj, .. } => {
            based.insert(obj.0);
        }
        Expr::Call { callee, .. } => {
            based.insert(callee.0);
        }
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
            if matches!(ast.get_expr(*target), Expr::Ident(_)) {
                based.insert(target.0);
            }
        }
        _ => {}
    }
}

/// Can the source of the callback loop at this site shrink while it
/// runs? `recv` is the receiver expression, `cb` the callback slot,
/// `o_param` the index of the source among the callback's arguments
/// (2 for forEach / map / filter / some / every, 3 for reduce).
pub(crate) fn hof_len_may_shrink(ast: &Ast, recv: ExprId, cb: ExprId, o_param: usize) -> bool {
    let s = &ast.hof_len_shrinks;
    if s.unknown_target {
        return true;
    }
    if s.names.is_empty() {
        return false;
    }
    let src_named = match name_of(ast, recv) {
        Some(n) => s.names.contains(n) || s.escaping.contains(n),
        // A temporary has no name of its own; only the callback's
        // parameter can reach it.
        None => false,
    };
    src_named || callback_o_param_may_shrink(ast, cb, o_param)
}

fn callback_o_param_may_shrink(ast: &Ast, cb: ExprId, o_param: usize) -> bool {
    let s = &ast.hof_len_shrinks;
    let name = match ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => fn_name.as_str(),
        Expr::Ident(n) => n.as_str(),
        _ => return true,
    };
    let name = name
        .strip_prefix("__forward_")
        .or_else(|| name.strip_prefix("__fwdrecv_"))
        .unwrap_or(name);
    let Some((params, reads_arguments)) = ast.stmts.iter().find_map(|st| match st {
        Stmt::FnDecl {
            name: n,
            params,
            body,
            ..
        } if n == name => Some((
            params,
            super::arguments_object_walkers::body_has_non_length_arguments_touch(ast, body),
        )),
        _ => None,
    }) else {
        return true;
    };
    if reads_arguments {
        return true;
    }
    let mut user = params.iter().filter(|p| !p.name.starts_with("__"));
    match user.nth(o_param) {
        Some(p) => s.names.contains(&p.name) || s.escaping.contains(&p.name),
        // The callback never names the source.
        None => false,
    }
}
