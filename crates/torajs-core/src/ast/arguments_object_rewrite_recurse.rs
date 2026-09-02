//! The mechanical copy-on-write recursion arm of
//! [`super::arguments_object_rewrite`]'s expression walker — split to
//! a sibling with the S2 threading (same knife as the stmt half).
//! Verbatim move; `pub(super)` so the parent's catch-all delegates.

use super::arguments_object::ArgcMode;
use super::arguments_object_rewrite::{SloppyCallee, keyed_callee_ref, rewrite_arguments_in_expr};
use super::{Ast, Expr, ExprId};
use crate::ast::PropKey;

/// The mechanical copy-on-write recursion over an expression's
/// children: recurse each child, hand the original `eid` back when
/// nothing moved, mint a rebuilt node when something did.
///
/// r380 file-size — the seventeen arms were one 295-line body. They
/// are split three ways here purely by position, NOT regrouped: the
/// patterns are distinct `Expr` variants, so no arm can shadow
/// another and the relative order carries nothing. Each part keeps
/// its own catch-all, and the leaf answer stays a bare `eid` so a
/// no-op clone never enters the arena.
#[allow(clippy::too_many_arguments)]
pub(super) fn rewrite_recurse_arm(
    ast: &mut Ast,
    eid: ExprId,
    e: Expr,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
    sloppy_callee: SloppyCallee<'_>,
) -> ExprId {
    match &e {
        Expr::PostIncr { .. }
        | Expr::Spread { .. }
        | Expr::Ternary { .. }
        | Expr::Nullish { .. }
        | Expr::OptChain { .. }
        | Expr::OptIndex { .. }
        | Expr::OptCall { .. } => {
            recurse_optional(ast, eid, e, params, argc_mode, is_argv_fn, sloppy_callee)
        }
        Expr::Member { .. } | Expr::Assign { .. } | Expr::ObjectLit { .. } => {
            recurse_place(ast, eid, e, params, argc_mode, is_argv_fn, sloppy_callee)
        }
        _ => recurse_operator(ast, eid, e, params, argc_mode, is_argv_fn, sloppy_callee),
    }
}

/// Operators and construction — the first positional third.
#[allow(clippy::too_many_arguments)]
fn recurse_operator(
    ast: &mut Ast,
    eid: ExprId,
    e: Expr,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
    sloppy_callee: SloppyCallee<'_>,
) -> ExprId {
    match e {
        Expr::BinOp { op, left, right } => {
            let l =
                rewrite_arguments_in_expr(ast, left, params, argc_mode, is_argv_fn, sloppy_callee);
            let r =
                rewrite_arguments_in_expr(ast, right, params, argc_mode, is_argv_fn, sloppy_callee);
            if l == left && r == right {
                return eid;
            }
            ast.add_expr(Expr::BinOp {
                op,
                left: l,
                right: r,
            })
        }
        Expr::Unary { op, expr } => {
            let e2 =
                rewrite_arguments_in_expr(ast, expr, params, argc_mode, is_argv_fn, sloppy_callee);
            if e2 == expr {
                return eid;
            }
            ast.add_expr(Expr::Unary { op, expr: e2 })
        }
        Expr::TypeOf { expr } => {
            let e2 =
                rewrite_arguments_in_expr(ast, expr, params, argc_mode, is_argv_fn, sloppy_callee);
            if e2 == expr {
                return eid;
            }
            ast.add_expr(Expr::TypeOf { expr: e2 })
        }
        // `delete arguments[i]` — §10.4.4.6 unmapped elements are
        // plain data properties, so the delete rides the
        // materialized array's index-delete (hole shadow entry). The
        // missing recursion left the raw `arguments` ident inside
        // the Delete node (10.5-7-b-4-s's ReferenceError).
        // `delete arguments.callee` — §13.5.1.2 step 3.a: deleting a
        // non-configurable own property in strict code throws; the
        // thrower call carries exactly that TypeError (the plain
        // recursion would mint a Call operand — not a property
        // reference — and refuse at compile time).
        Expr::Delete { expr } => {
            rewrite_delete_arm(ast, eid, expr, params, argc_mode, is_argv_fn, sloppy_callee)
        }
        // Constructor arguments are ordinary value positions — the
        // missing arms left the rewrite unable to reach an
        // `arguments` use inside `new Boolean(arguments.length ===
        // 0)` (the walkers' New arms landed the same knife).
        Expr::New {
            class_name,
            args,
            type_args,
        } => {
            let (new_args, changed) =
                rewrite_args_vec(ast, &args, params, argc_mode, is_argv_fn, sloppy_callee);
            if !changed {
                return eid;
            }
            ast.add_expr(Expr::New {
                class_name,
                args: new_args,
                type_args,
            })
        }
        Expr::NewDynamic { callee, args } => {
            let c = rewrite_arguments_in_expr(
                ast,
                callee,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            );
            let (new_args, changed) =
                rewrite_args_vec(ast, &args, params, argc_mode, is_argv_fn, sloppy_callee);
            if c == callee && !changed {
                return eid;
            }
            ast.add_expr(Expr::NewDynamic {
                callee: c,
                args: new_args,
            })
        }
        // Length-write knife — `arguments.length--` (walker-mirror:
        // the scans reach PostIncr targets; without this arm the
        // stale Member leaked to the lowering as "post-incr field on
        // non-obj Ptr"). A length-writing body classifies RealLocal
        // (`__torajs_argc_len--`), LiveLength lands on
        // `__torajs_arguments.length--`.
        // Leaf / opaque shapes — no children to recurse through here.
        // Intentionally returns the original `eid` so we don't bloat
        // the arena with no-op clones.
        _ => eid,
    }
}

/// Post-increment through optional-chaining — the second third.
#[allow(clippy::too_many_arguments)]
fn recurse_optional(
    ast: &mut Ast,
    eid: ExprId,
    e: Expr,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
    sloppy_callee: SloppyCallee<'_>,
) -> ExprId {
    match e {
        Expr::PostIncr { target, is_inc } => {
            let t = rewrite_arguments_in_expr(
                ast,
                target,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            );
            if t == target {
                return eid;
            }
            ast.add_expr(Expr::PostIncr { target: t, is_inc })
        }
        Expr::Spread { expr } => {
            let e2 =
                rewrite_arguments_in_expr(ast, expr, params, argc_mode, is_argv_fn, sloppy_callee);
            if e2 == expr {
                return eid;
            }
            ast.add_expr(Expr::Spread { expr: e2 })
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let c =
                rewrite_arguments_in_expr(ast, cond, params, argc_mode, is_argv_fn, sloppy_callee);
            let t = rewrite_arguments_in_expr(
                ast,
                then_branch,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            );
            let e2 = rewrite_arguments_in_expr(
                ast,
                else_branch,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            );
            if c == cond && t == then_branch && e2 == else_branch {
                return eid;
            }
            ast.add_expr(Expr::Ternary {
                cond: c,
                then_branch: t,
                else_branch: e2,
            })
        }
        Expr::Nullish { lhs, rhs } => {
            let l =
                rewrite_arguments_in_expr(ast, lhs, params, argc_mode, is_argv_fn, sloppy_callee);
            let r =
                rewrite_arguments_in_expr(ast, rhs, params, argc_mode, is_argv_fn, sloppy_callee);
            if l == lhs && r == rhs {
                return eid;
            }
            ast.add_expr(Expr::Nullish { lhs: l, rhs: r })
        }
        Expr::OptChain { obj, name } => {
            let o =
                rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn, sloppy_callee);
            if o == obj {
                return eid;
            }
            ast.add_expr(Expr::OptChain { obj: o, name })
        }
        Expr::OptIndex { obj, index } => {
            let o =
                rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn, sloppy_callee);
            let ix =
                rewrite_arguments_in_expr(ast, index, params, argc_mode, is_argv_fn, sloppy_callee);
            if o == obj && ix == index {
                return eid;
            }
            ast.add_expr(Expr::OptIndex { obj: o, index: ix })
        }
        Expr::OptCall { callee, args } => {
            let c = rewrite_arguments_in_expr(
                ast,
                callee,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            );
            let new_args: Vec<ExprId> = args
                .iter()
                .map(|a| {
                    rewrite_arguments_in_expr(ast, *a, params, argc_mode, is_argv_fn, sloppy_callee)
                })
                .collect();
            if c == callee && new_args == args {
                return eid;
            }
            ast.add_expr(Expr::OptCall {
                callee: c,
                args: new_args,
            })
        }
        // Leaf / opaque shapes — no children to recurse through here.
        // Intentionally returns the original `eid` so we don't bloat
        // the arena with no-op clones.
        _ => eid,
    }
}

/// Member reads, assignment targets and object literals — the third.
#[allow(clippy::too_many_arguments)]
fn recurse_place(
    ast: &mut Ast,
    eid: ExprId,
    e: Expr,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
    sloppy_callee: SloppyCallee<'_>,
) -> ExprId {
    match e {
        Expr::Member { obj, name } => {
            let o =
                rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn, sloppy_callee);
            if o == obj {
                return eid;
            }
            ast.add_expr(Expr::Member { obj: o, name })
        }
        Expr::Assign { target, value } => {
            // `arguments.callee = v` — §13.15.2 PutValue on the
            // %ThrowTypeError% accessor throws; the read-position
            // callee rewrite would have minted a Call in target
            // position ("invalid assignment target", the S10.6_A3
            // regression). RHS still evaluates first (spec order),
            // then the thrower call raises.
            if matches!(ast.get_expr(target), Expr::Member { obj, name }
                if name == "callee"
                    && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments"))
            {
                let v = rewrite_arguments_in_expr(
                    ast,
                    value,
                    params,
                    argc_mode,
                    is_argv_fn,
                    sloppy_callee,
                );
                // S2 sloppy write — callee is an ordinary writable
                // data property: keyed store into the bag entry
                // (materialization forced by the main pass).
                if sloppy_callee != SloppyCallee::Strict {
                    let idx = keyed_callee_ref(ast);
                    return ast.add_expr(Expr::Assign {
                        target: idx,
                        value: v,
                    });
                }
                let callee = ast.add_expr(Expr::Ident("__torajs_arguments_callee".into()));
                let throw_call = ast.add_expr(Expr::Call {
                    callee,
                    args: Vec::new(),
                });
                return ast.add_expr(Expr::Sequence {
                    left: v,
                    right: throw_call,
                });
            }
            let t = rewrite_arguments_in_expr(
                ast,
                target,
                params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            );
            let v =
                rewrite_arguments_in_expr(ast, value, params, argc_mode, is_argv_fn, sloppy_callee);
            if t == target && v == value {
                return eid;
            }
            ast.add_expr(Expr::Assign {
                target: t,
                value: v,
            })
        }
        Expr::ObjectLit { fields } => {
            let new_fields: Vec<(PropKey, ExprId)> = fields
                .iter()
                .map(|(n, e)| {
                    (
                        n.clone(),
                        rewrite_arguments_in_expr(
                            ast,
                            *e,
                            params,
                            argc_mode,
                            is_argv_fn,
                            sloppy_callee,
                        ),
                    )
                })
                .collect();
            if new_fields == fields {
                return eid;
            }
            ast.add_expr(Expr::ObjectLit { fields: new_fields })
        }
        // Leaf / opaque shapes — no children to recurse through here.
        // Intentionally returns the original `eid` so we don't bloat
        // the arena with no-op clones.
        _ => eid,
    }
}

/// Copy-on-write over an argument vector — shared by the New /
/// NewDynamic arms. Answers the (possibly rebuilt) vector plus
/// whether any element moved, so the caller can keep the original
/// node when nothing did.
fn rewrite_args_vec(
    ast: &mut Ast,
    args: &[ExprId],
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
    sloppy_callee: SloppyCallee<'_>,
) -> (Vec<ExprId>, bool) {
    let mut out: Vec<ExprId> = Vec::with_capacity(args.len());
    let mut changed = false;
    for a in args {
        let r = rewrite_arguments_in_expr(ast, *a, params, argc_mode, is_argv_fn, sloppy_callee);
        changed |= r != *a;
        out.push(r);
    }
    (out, changed)
}

/// The Delete arm's body, extracted whole when the New / NewDynamic
/// arms landed (the enclosing match is a registered over-200 debt
/// item — additions pair with a shrink). Semantics unchanged: the
/// callee delete rides the bag entry (sloppy) or the thrower
/// (strict), the length delete rides the materialized array's keyed
/// delete, everything else recurses.
fn rewrite_delete_arm(
    ast: &mut Ast,
    eid: ExprId,
    expr: ExprId,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
    sloppy_callee: SloppyCallee<'_>,
) -> ExprId {
    if matches!(ast.get_expr(expr), Expr::Member { obj, name }
        if name == "callee"
            && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments"))
    {
        // S2 sloppy delete — callee is configurable (S10.6_A3_T3
        // expects `true`): ride the bag entry's keyed delete. A
        // callee write/delete anywhere in the body forces
        // materialization (see the main pass), so
        // `__torajs_arguments` is always bound here.
        if sloppy_callee != SloppyCallee::Strict {
            let idx = keyed_callee_ref(ast);
            return ast.add_expr(Expr::Delete { expr: idx });
        }
        let callee = ast.add_expr(Expr::Ident("__torajs_arguments_callee".into()));
        return ast.add_expr(Expr::Call {
            callee,
            args: Vec::new(),
        });
    }
    // `delete arguments.length` — the length arm's read rewrite
    // would fold the operand to a number ("must be a property
    // reference"); route it as a keyed delete on the materialized
    // array instead, where the arguments-length tombstone kernel
    // answers §10.4.4's configurable delete (S10.6_A5_T3).
    if matches!(ast.get_expr(expr), Expr::Member { obj, name }
        if name == "length"
            && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments"))
    {
        let arr = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
        let key = ast.add_expr(Expr::String("length".into()));
        let idx = ast.add_expr(Expr::Index {
            obj: arr,
            index: key,
        });
        return ast.add_expr(Expr::Delete { expr: idx });
    }
    let e2 = rewrite_arguments_in_expr(ast, expr, params, argc_mode, is_argv_fn, sloppy_callee);
    if e2 == expr {
        return eid;
    }
    ast.add_expr(Expr::Delete { expr: e2 })
}
