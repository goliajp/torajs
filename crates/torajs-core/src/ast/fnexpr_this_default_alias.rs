//! The no-receiver slot reached through a NAME rather than written in
//! the argument position — `const f = function () { … this … };
//! p.then(f)`.
//!
//! Split from `fnexpr_this_default.rs` under the 500-line file rule
//! (that file sits at 461, and a helper plus its doc would break it).
//!
//! Measured before it was written, which is what scoped it: the array
//! callbacks did NOT need this. `const g = function () { … this … };
//! [1, 2].forEach(g)` already answers `undefined` on HEAD, because the
//! callback reaches the callee through a plain user-fn argument
//! position where the `__forward_` shim supplies the receiver-less
//! answer. `Promise.resolve(1).then(f)` with the same spelling refuses
//! to compile. So the gap is the handler slots, and admitting the
//! array ones here would be pure added code for no movement — the
//! rotation-386 lesson about writing arms before measuring them.
//!
//! **Every occurrence of the name has to be an admitted slot**, not
//! just one of them. The binding materializes a single closure, so a
//! `__this` bound to the no-receiver answer is that closure's answer
//! everywhere it is used; one use that supplies a real receiver would
//! make it wrong. A plain call `f()` is deliberately NOT admitted
//! either, even though it also passes no receiver: under the sloppy
//! goal it answers the global object rather than `undefined`, and
//! sorting that out per use is a distinction this census does not
//! carry.
//!
//! The binding must be `const` and declared once, the same coarse,
//! name-keyed bar `certain_bindings` uses — an over-refusal costs one
//! loud reject.

use super::{Ast, Expr, ExprId, Stmt};

/// The lifted names of function expressions bound to a `const` whose
/// every use is one of `admitted`, paired with the binding's
/// initializer expression.
///
/// `admitted` is the set of argument positions the slot table already
/// proved the spec invokes with no receiver, so this adds an
/// indirection to that proof rather than a new one.
pub(super) fn alias_targets(
    ast: &Ast,
    admitted: &std::collections::HashSet<ExprId>,
) -> Vec<(ExprId, String)> {
    let names = fully_admitted_names(ast, admitted);
    if names.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    collect_const_fn_bindings(&ast.stmts, ast, &names, &mut out);
    out
}

/// The identifier names every one of whose reads sits in an admitted
/// slot. A name with no reads at all is not included — there would be
/// nothing to bind it for.
fn fully_admitted_names(
    ast: &Ast,
    admitted: &std::collections::HashSet<ExprId>,
) -> std::collections::HashSet<String> {
    let mut reads: std::collections::HashMap<&str, (usize, usize)> =
        std::collections::HashMap::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        if let Expr::Ident(n) = e {
            let slot = admitted.contains(&ExprId(i as u32));
            let entry = reads.entry(n.as_str()).or_insert((0, 0));
            entry.0 += 1;
            if slot {
                entry.1 += 1;
            }
        }
    }
    reads
        .into_iter()
        .filter(|(_, (total, in_slot))| *in_slot > 0 && total == in_slot)
        .map(|(n, _)| n.to_string())
        .collect()
}

/// The `const <name> = function () {…}` declarations for those names,
/// requiring the name to be declared exactly once anywhere.
fn collect_const_fn_bindings(
    stmts: &[Stmt],
    ast: &Ast,
    names: &std::collections::HashSet<String>,
    out: &mut Vec<(ExprId, String)>,
) {
    let mut decls: Vec<(String, Option<ExprId>)> = Vec::new();
    walk_decls(stmts, &mut decls);
    for (name, init) in &decls {
        if !names.contains(name) || decls.iter().filter(|(n, _)| n == name).count() != 1 {
            continue;
        }
        let Some(init) = *init else { continue };
        if let Some(fn_name) = super::fnexpr_this_default::unclaimed_fnexpr(ast, init) {
            out.push((init, fn_name));
        }
    }
}

/// Every `let` / `const` declaration in the tree, paired with its
/// initializer for the immutable ones. A mutable declaration records
/// `None`, which keeps it in the count (so it still disqualifies a
/// duplicated name) while never qualifying itself.
fn walk_decls(stmts: &[Stmt], out: &mut Vec<(String, Option<ExprId>)>) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                mutable,
                name,
                init,
                ..
            } => out.push((name.clone(), (!*mutable).then_some(*init))),
            Stmt::FnDecl { body, .. } => walk_decls(body, out),
            Stmt::Block(inner) | Stmt::Multi(inner) => walk_decls(inner, out),
            _ => {}
        }
    }
}
