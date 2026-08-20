//! The no-receiver slot proved by READING THE CALLEE — a function
//! expression handed to a locally declared function whose parameter
//! that argument lands in is only ever *called*.
//!
//! Every other entry in the slot table
//! ([`super::fnexpr_this_default_slots`]) is read off spec text,
//! because the callee is a builtin whose body we cannot see. When the
//! callee is written in the same program the proof is stronger than a
//! citation: `function run(t: () => void) { t() }` invokes its
//! parameter with `t()`, and §13.3.6.1 EvaluateCall passes no receiver
//! for a callee that is not a Reference. So `run(function () { … this
//! … })` answers exactly what the table's other entries answer —
//! `undefined` under the strict goal, the global object under the
//! sloppy one, which is the split
//! [`super::fnexpr_this_default::bind_fnexpr_this_default`] already
//! makes.
//!
//! `assert.throws(TypeError, function () { … this … })` is the
//! spelling this exists for: the test262 harness rewrite turns it into
//! `__t262_throws_runtime(<fn-expr>, msg)`, whose `thunk: () => void`
//! parameter is called and nothing else. A concrete function
//! signature is exactly the slot the *promote* knives must decline (a
//! typed indirect call does not shift argv on
//! `FLAG_CLOSURE_RECV_FIRST`), so that lane can never claim it — and
//! before this module the program had no answer at all.
//!
//! **CALL-ONLY is the whole proof, and it is a refutation walk.** A
//! parameter qualifies when every occurrence of its name inside the
//! callee — nested blocks, nested functions, and arrow bodies
//! included — stands as the callee of a plain call. Anything else
//! refutes it: `t.call(o)` and `t.apply(o)` pick their own receiver,
//! `o.m = t` makes the value a method, `return t` lets an unseen call
//! site decide, and a nested closure that CAPTURES the name carries it
//! somewhere this walk cannot follow (captures are recorded as strings
//! on the `Expr::Closure` node, not as identifier nodes, so they are
//! checked explicitly rather than fallen over).
//!
//! The census is name-keyed and deliberately coarse, like the rest of
//! this family: a second declaration of the same name, an assignment
//! to it, or a spread at the call site all drop the candidate, and a
//! parameter name shadowed inside the body is judged on the shadow's
//! uses too. Every one of those is an over-refusal, which costs a loud
//! reject and never a wrong `undefined`.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Param, Stmt};

/// The argument positions of `f(a…)` whose parameter the callee only
/// ever calls — the no-receiver slots this module proves.
pub(super) fn userfn_thunk_slots(ast: &Ast) -> Vec<ExprId> {
    let mut decls: std::collections::HashMap<&str, Option<(&[Param], &[Stmt])>> =
        std::collections::HashMap::new();
    collect_decls(&ast.stmts, &mut decls);
    drop_reassigned(ast, &mut decls);
    if decls.is_empty() {
        return Vec::new();
    }
    let call_only: std::collections::HashMap<&str, Vec<bool>> = decls
        .iter()
        .filter_map(|(name, sig)| {
            let (params, body) = (*sig)?;
            Some((
                *name,
                params
                    .iter()
                    .map(|p| !p.is_rest && call_only_name(ast, body, &p.name))
                    .collect(),
            ))
        })
        .collect();
    let mut out = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(fname) = &ast.exprs[callee.0 as usize] else {
            continue;
        };
        let Some(flags) = call_only.get(fname.as_str()) else {
            continue;
        };
        // A spread breaks the index alignment the slot is keyed on.
        if args
            .iter()
            .any(|a| matches!(&ast.exprs[a.0 as usize], Expr::Spread { .. }))
        {
            continue;
        }
        for (i, a) in args.iter().enumerate() {
            if flags.get(i).copied().unwrap_or(false) {
                out.push(*a);
            }
        }
    }
    out
}

/// Every function declaration in the program by name; a repeated name
/// poisons the entry, since a by-name call cannot tell which body
/// runs. A body whose first parameter is `__env` is a LIFTED closure
/// rather than a user declaration — its call sites do not spell the
/// env argument, so the index alignment this module keys on would be
/// off by one.
fn collect_decls<'a>(
    stmts: &'a [Stmt],
    out: &mut std::collections::HashMap<&'a str, Option<(&'a [Param], &'a [Stmt])>>,
) {
    for s in stmts {
        match s {
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                let sig = (params.first().is_some_and(|p| p.name == "__env"))
                    .then_some(None)
                    .unwrap_or(Some((params.as_slice(), body.as_slice())));
                out.entry(name.as_str())
                    .and_modify(|e| *e = None)
                    .or_insert(sig);
                collect_decls(body, out);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => collect_decls(inner, out),
            _ => {}
        }
    }
}

/// A function declaration's binding is writable, so `f = other` would
/// leave the name pointing at a body this census never read.
fn drop_reassigned(
    ast: &Ast,
    decls: &mut std::collections::HashMap<&str, Option<(&[Param], &[Stmt])>>,
) {
    for e in &ast.exprs {
        if let Expr::Assign { target, .. } = e
            && let Expr::Ident(n) = &ast.exprs[target.0 as usize]
        {
            decls.remove(n.as_str());
        }
    }
}

/// `true` when every occurrence of `name` in `body` is the callee of a
/// plain call.
fn call_only_name(ast: &Ast, body: &[Stmt], name: &str) -> bool {
    let mut ok = true;
    scan_list(ast, body, name, &mut ok);
    ok
}

fn scan_list(ast: &Ast, stmts: &[Stmt], name: &str, ok: &mut bool) {
    for s in stmts {
        if !*ok {
            return;
        }
        for root in stmt_exprs(s) {
            scan_expr(ast, root, name, ok);
        }
        // `stmt_children_ref` stops at a nested function body on
        // purpose. This walk enters it: a name-keyed census that
        // skipped nested bodies would miss the very uses that refute.
        if let Stmt::FnDecl { body, .. } = s {
            scan_list(ast, body, name, ok);
        }
        for child in stmt_children_ref(s) {
            scan_list(ast, std::slice::from_ref(child), name, ok);
        }
    }
}

fn scan_expr(ast: &Ast, eid: ExprId, name: &str, ok: &mut bool) {
    if !*ok {
        return;
    }
    match ast.get_expr(eid) {
        Expr::Ident(n) if n == name => {
            *ok = false;
            return;
        }
        Expr::Closure { captures, .. } if captures.iter().any(|c| c == name) => {
            *ok = false;
            return;
        }
        // The one admitted position: `name(args…)`. The callee node is
        // skipped, the arguments are not.
        Expr::Call { callee, args } | Expr::OptCall { callee, args } if matches!(&ast.exprs[callee.0 as usize], Expr::Ident(n) if n == name) =>
        {
            for a in args {
                scan_expr(ast, *a, name, ok);
            }
            return;
        }
        Expr::ArrowFn { body, .. } => {
            scan_list(ast, body, name, ok);
            return;
        }
        _ => {}
    }
    for c in expr_children(ast, eid) {
        scan_expr(ast, c, name, ok);
    }
}
