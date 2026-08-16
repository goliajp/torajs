//! One reference, one two-armed guard (RFC 20260814 刀 1) — the
//! minting half of `desugar_with`, split out at the 500-line cap in
//! rotation 417. The parent decides WHICH occurrences get rewritten;
//! this file decides what each one becomes.
//!
//! Three of the four rewrites replace a node whose CHILD the guard
//! does not reuse — both arms mint their own callee / operand /
//! target — so each of those tombstones the child it consumed. A
//! parentless `Ident` left behind still reads as a use of that
//! binding to every whole-arena analysis, and the one that noticed
//! was the knife-2 receiver promotion: a `this`-using function
//! expression called from inside a `with` body refused to compile.
//! `rewrite_read` is the exception and needs no tombstone — it writes
//! over the `Ident`'s own arena slot.

use super::super::{Ast, Expr, ExprId};
use super::WITH_HAS_FN;

/// `n` -> `(__torajs_with_has(w, "n") ? w.n : n)`, written OVER the
/// original arena slot so no parent link has to be found or rebuilt.
/// The fall-through branch is a fresh `Ident` node carrying the same
/// name, which is what the outer `with` of a nested pair rewrites.
pub(super) fn rewrite_read(ast: &mut Ast, eid: ExprId, w: &str, n: &str) {
    let cond = has_call(ast, w, n);
    let then_branch = with_member(ast, w, n);
    let else_branch = ast.add_expr(Expr::Ident(n.to_string()));
    ast.with_fallthrough_idents.insert(else_branch);
    ast.exprs[eid.0 as usize] = Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    };
}

/// `n(a…)` -> `(has ? w.n(a…) : n(a…))`. The whole CALL is replaced,
/// not just its callee: §9.1.1.2.3 WithBaseObject makes the object the
/// receiver when the name came from it, and a rewritten callee alone
/// would call it with `undefined`. The argument subtrees are cloned
/// rather than shared — only one branch runs, but the arena is a tree
/// everywhere else and a shared node is a shape no other pass expects.
///
/// The ORIGINAL callee node is consumed by this rewrite and has to be
/// tombstoned. Both branches mint their own callee, so the old node is
/// left with no parent — and a parentless `Ident` still reads as a use
/// of that binding to every whole-arena analysis, which is what the
/// sibling `rewrite_read` avoids for free by writing over the Ident's
/// own slot. Left alive it looked like a use of the name in no
/// recognised position, so `function`-expression bindings called from
/// inside a `with` body lost the knife-2 receiver promotion and their
/// `this` became an unbound `__this` capture — the same failure the
/// value-shaped-heritage lane tombstones its consumed heritage node
/// to avoid.
pub(super) fn rewrite_call(ast: &mut Ast, call: ExprId, callee: ExprId, w: &str, n: &str) {
    let Expr::Call { args, .. } = ast.get_expr(call) else {
        return;
    };
    let args = args.clone();
    let cond = has_call(ast, w, n);
    let recv = with_member(ast, w, n);
    let then_branch = ast.add_expr(Expr::Call {
        callee: recv,
        args: args.clone(),
    });
    let mut cloner = super::super::clone_body::BodyCloner::new(ast);
    let cloned: Vec<ExprId> = args.iter().map(|a| cloner.clone_expr(*a)).collect();
    migrate_side_tables(&mut cloner);
    let plain = ast.add_expr(Expr::Ident(n.to_string()));
    ast.with_fallthrough_idents.insert(plain);
    let else_branch = ast.add_expr(Expr::Call {
        callee: plain,
        args: cloned,
    });
    ast.tombstone_expr(callee);
    ast.exprs[call.0 as usize] = Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    };
}

/// `typeof n` / `n++` / `n--` / `delete n`: the whole WRAPPING node is
/// replaced with a guard whose two arms are the same operator applied
/// to `w.n` and to `n`. Nothing has to be cloned — the operand is the
/// only child, and each arm mints its own — so unlike a compound
/// assignment this shape keeps §9.1.1.2.1 HasBinding evaluated exactly
/// ONCE, which is what the spec's single ResolveBinding does.
///
/// `typeof` is the shape that must not answer through the fall-through
/// alone: §13.5.3 answers `"undefined"` for an unresolvable name
/// instead of throwing, and the object arm has to be consulted before
/// that rule applies. `delete` is the same argument one step further:
/// §13.5.1.2 evaluates the reference first, and a reference the object
/// supplies is a PROPERTY reference, so the object arm must really
/// remove `w.n` where the fall-through only answers a boolean.
pub(super) fn rewrite_wrapping(ast: &mut Ast, outer: ExprId, w: &str, n: &str) {
    // Consumed, like the callee in `rewrite_call` — both arms mint
    // their own operand, so the original is left parentless and would
    // still read as a use of the binding.
    let consumed = match ast.get_expr(outer) {
        Expr::TypeOf { expr } | Expr::Delete { expr } => Some(*expr),
        Expr::PostIncr { target, .. } => Some(*target),
        _ => None,
    };
    let cond = has_call(ast, w, n);
    let obj_operand = with_member(ast, w, n);
    let plain_operand = ast.add_expr(Expr::Ident(n.to_string()));
    ast.with_fallthrough_idents.insert(plain_operand);
    let (then_branch, else_branch) = match ast.get_expr(outer) {
        Expr::TypeOf { .. } => (
            Expr::TypeOf { expr: obj_operand },
            Expr::TypeOf {
                expr: plain_operand,
            },
        ),
        Expr::Delete { .. } => (
            Expr::Delete { expr: obj_operand },
            Expr::Delete {
                expr: plain_operand,
            },
        ),
        Expr::PostIncr { is_inc, .. } => {
            let is_inc = *is_inc;
            (
                Expr::PostIncr {
                    target: obj_operand,
                    is_inc,
                },
                Expr::PostIncr {
                    target: plain_operand,
                    is_inc,
                },
            )
        }
        _ => return,
    };
    let then_branch = ast.add_expr(then_branch);
    let else_branch = ast.add_expr(else_branch);
    if let Some(c) = consumed {
        ast.tombstone_expr(c);
    }
    ast.exprs[outer.0 as usize] = Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    };
}

/// `n = v` -> `(has ? (w.n = v) : (n = v))`, and the compound form
/// `n op= v` (which reaches here as `n = n op v`) with the cloned left
/// operand filled in per branch: `w.n` on the object side, the plain
/// name on the fall-through side.
///
/// Filling it in rather than guarding it is what keeps §9.1.1.2.1
/// HasBinding evaluated ONCE for the whole compound. The spec resolves
/// the reference a single time and then does GetValue + PutValue on
/// it; two independent tests would disagree if evaluating `v` added or
/// removed the property between them.
///
/// The value expression is cloned for the object arm rather than
/// shared: only one arm ever runs, but the arena is a tree everywhere
/// else and a shared node is a shape no other pass expects.
pub(super) fn rewrite_assign(
    ast: &mut Ast,
    node: ExprId,
    compound_lhs: Option<ExprId>,
    w: &str,
    n: &str,
) {
    let Expr::Assign { target, value } = ast.get_expr(node) else {
        return;
    };
    let (old_target, value) = (*target, *value);
    let cond = has_call(ast, w, n);
    let mut cloner = super::super::clone_body::BodyCloner::new(ast);
    let cloned_value = cloner.clone_expr(value);
    let cloned_lhs = compound_lhs.map(|l| cloner.map[&l]);
    migrate_side_tables(&mut cloner);
    if let (Some(orig), Some(clone)) = (compound_lhs, cloned_lhs) {
        let obj_read = with_member(ast, w, n);
        ast.exprs[clone.0 as usize] = ast.exprs[obj_read.0 as usize].clone();
        // The fall-through arm keeps the plain name, which is already
        // what the node holds — it only needs the diagnostic
        // exemption every fall-through read gets.
        ast.with_fallthrough_idents.insert(orig);
    }
    let then_target = with_member(ast, w, n);
    let then_branch = ast.add_expr(Expr::Assign {
        target: then_target,
        value: cloned_value,
    });
    let else_target = ast.add_expr(Expr::Ident(n.to_string()));
    ast.with_fallthrough_idents.insert(else_target);
    let else_branch = ast.add_expr(Expr::Assign {
        target: else_target,
        value,
    });
    // Same consumed-node rule as `rewrite_call`: both arms mint their
    // own target. The compound form's LEFT OPERAND is a different node
    // and stays alive — it is still reachable through the untouched
    // `value` subtree on the fall-through side.
    ast.tombstone_expr(old_target);
    ast.exprs[node.0 as usize] = Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    };
}

/// Carry the side-table entries of a cloned arm across to the clone.
///
/// A guard's two arms are the same source expression, so whatever a
/// side table says about the original is true of the copy — and every
/// one of those tables is keyed by node id, which the clone does not
/// share. Left out, the copy is a plain tree that has forgotten what
/// it is: a `String(sub)` the parser wrote as machinery becomes an
/// ordinary read of the name `String`, which the NEXT `with` out is
/// then free to answer.
///
/// Migrating all of them rather than the one table this pass knows
/// about is the helper's own contract — it walks every ExprId-keyed
/// table so a new one lands covered instead of silently missing.
fn migrate_side_tables(cloner: &mut super::super::clone_body::BodyCloner<'_>) {
    super::super::clone_body_tables::migrate(cloner);
}

fn has_call(ast: &mut Ast, w: &str, n: &str) -> ExprId {
    let f = ast.add_expr(Expr::Ident(WITH_HAS_FN.to_string()));
    let obj = ast.add_expr(Expr::Ident(w.to_string()));
    let key = ast.add_expr(Expr::String(n.to_string()));
    ast.add_expr(Expr::Call {
        callee: f,
        args: vec![obj, key],
    })
}

fn with_member(ast: &mut Ast, w: &str, n: &str) -> ExprId {
    let obj = ast.add_expr(Expr::Ident(w.to_string()));
    ast.add_expr(Expr::Member {
        obj,
        name: n.to_string(),
    })
}
