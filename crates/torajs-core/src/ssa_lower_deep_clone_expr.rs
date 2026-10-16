//! Deep-clone Expr helpers — the expression half of
//! [`crate::ssa_lower_deep_clone`], displaced verbatim (rotation 637)
//! so the statement half has room for its missing arms. Same
//! contract: used only by generic monomorphization, allocates fresh
//! ExprIds by appending to `Ast::exprs`, and records every (old, new)
//! pair in `map`.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ssa_lower_deep_clone::deep_clone_stmt;

/// Leaf variants — no child ExprIds, so a shallow field clone IS the
/// deep clone (chunk 771 extraction; `deep_clone_expr`'s match had
/// drifted past the 200-line fn limit). Reached as the composite
/// match's catch-all; a NEW composite variant landing here panics
/// loudly instead of silently shallow-cloning its children.
fn clone_leaf(e: &Expr) -> Expr {
    match e {
        Expr::Ident(n) => Expr::Ident(n.clone()),
        Expr::String(s) => Expr::String(s.clone()),
        Expr::Number(n) => Expr::Number(*n),
        Expr::BigInt { digits, radix } => Expr::BigInt {
            digits: digits.clone(),
            radix: *radix,
        },
        Expr::Bool(b) => Expr::Bool(*b),
        Expr::Null => Expr::Null,
        Expr::Uninit => Expr::Uninit,
        Expr::Elision => Expr::Elision,
        Expr::Regex { pattern, flags } => Expr::Regex {
            pattern: pattern.clone(),
            flags: flags.clone(),
        },
        Expr::This => Expr::This,
        Expr::NewTarget => Expr::NewTarget,
        // Closure captures are (name, ExprId) pairs minted by the
        // lift — the construction-site ids are shared on purpose.
        Expr::Closure { fn_name, captures } => Expr::Closure {
            fn_name: fn_name.clone(),
            captures: captures.clone(),
        },
        other => panic!("deep_clone_expr: composite variant fell through to clone_leaf: {other:?}"),
    }
}

pub(crate) fn deep_clone_expr(
    ast: &mut Ast,
    map: &mut Vec<(ExprId, ExprId)>,
    eid: ExprId,
) -> ExprId {
    if let Some(e) = clone_container_expr(ast, map, eid) {
        let new_id = ast.add_expr(e);
        map.push((eid, new_id));
        return new_id;
    }
    if let Some(e) = clone_tail_expr(ast, map, eid) {
        let new_id = ast.add_expr(e);
        map.push((eid, new_id));
        return new_id;
    }
    // A use of a same-name binding that is NOT the promoted fn-expr
    // stays that in every copy; a fresh id would read as promoted.
    if ast.fnexpr_recv_foreign_uses.contains(&eid) {
        let new_id = ast.add_expr(clone_leaf(ast.get_expr(eid)));
        ast.fnexpr_recv_foreign_uses.insert(new_id);
        map.push((eid, new_id));
        return new_id;
    }
    let new_expr = match ast.get_expr(eid) {
        Expr::BinOp { op, left, right } => {
            let op = *op;
            let l = *left;
            let r = *right;
            Expr::BinOp {
                op,
                left: deep_clone_expr(ast, map, l),
                right: deep_clone_expr(ast, map, r),
            }
        }
        Expr::Unary { op, expr } => {
            let op = *op;
            let e = *expr;
            Expr::Unary {
                op,
                expr: deep_clone_expr(ast, map, e),
            }
        }
        Expr::Member { obj, name } => {
            let o = *obj;
            let name = name.clone();
            Expr::Member {
                obj: deep_clone_expr(ast, map, o),
                name,
            }
        }
        Expr::Call { callee, args } => {
            let c = *callee;
            let args = args.clone();
            Expr::Call {
                callee: deep_clone_expr(ast, map, c),
                args: args
                    .into_iter()
                    .map(|a| deep_clone_expr(ast, map, a))
                    .collect(),
            }
        }
        Expr::Assign { target, value } => {
            let t = *target;
            let v = *value;
            Expr::Assign {
                target: deep_clone_expr(ast, map, t),
                value: deep_clone_expr(ast, map, v),
            }
        }
        Expr::Index { obj, index } => {
            let o = *obj;
            let i = *index;
            Expr::Index {
                obj: deep_clone_expr(ast, map, o),
                index: deep_clone_expr(ast, map, i),
            }
        }
        Expr::New {
            class_name,
            args,
            type_args,
        } => {
            let class_name = class_name.clone();
            let args = args.clone();
            let type_args = type_args.clone();
            Expr::New {
                class_name,
                args: args
                    .into_iter()
                    .map(|a| deep_clone_expr(ast, map, a))
                    .collect(),
                type_args,
            }
        }
        Expr::Super { args } => {
            let args = args.clone();
            Expr::Super {
                args: args
                    .into_iter()
                    .map(|a| deep_clone_expr(ast, map, a))
                    .collect(),
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = *cond;
            let t = *then_branch;
            let e = *else_branch;
            Expr::Ternary {
                cond: deep_clone_expr(ast, map, c),
                then_branch: deep_clone_expr(ast, map, t),
                else_branch: deep_clone_expr(ast, map, e),
            }
        }
        Expr::TypeOf { expr } => {
            let e = *expr;
            Expr::TypeOf {
                expr: deep_clone_expr(ast, map, e),
            }
        }
        Expr::Delete { expr } => {
            let e = *expr;
            Expr::Delete {
                expr: deep_clone_expr(ast, map, e),
            }
        }
        Expr::NewDynamic { callee, args } => {
            let c = *callee;
            let a = args.clone();
            Expr::NewDynamic {
                callee: deep_clone_expr(ast, map, c),
                args: a
                    .into_iter()
                    .map(|arg| deep_clone_expr(ast, map, arg))
                    .collect(),
            }
        }
        Expr::InstanceOf { expr, rhs } => {
            let e = *expr;
            let r = *rhs;
            Expr::InstanceOf {
                expr: deep_clone_expr(ast, map, e),
                rhs: deep_clone_expr(ast, map, r),
            }
        }
        Expr::Spread { expr } => {
            let e = *expr;
            Expr::Spread {
                expr: deep_clone_expr(ast, map, e),
            }
        }
        leaf => clone_leaf(leaf),
    };
    let new_id = ast.add_expr(new_expr);
    map.push((eid, new_id));
    new_id
}

/// Optional-chain / postfix / sequence tail arms — same
/// copy-then-recurse shape as the main match; an `Option`
/// fall-through sibling of [`clone_container_expr`], extracted when
/// the main fn regrew past the 200-line hard limit (file-size debt,
/// rotation 343).
fn clone_tail_expr(ast: &mut Ast, map: &mut Vec<(ExprId, ExprId)>, eid: ExprId) -> Option<Expr> {
    Some(match ast.get_expr(eid) {
        Expr::Nullish { lhs, rhs } => {
            let l = *lhs;
            let r = *rhs;
            Expr::Nullish {
                lhs: deep_clone_expr(ast, map, l),
                rhs: deep_clone_expr(ast, map, r),
            }
        }
        Expr::OptChain { obj, name } => {
            let o = *obj;
            let name = name.clone();
            Expr::OptChain {
                obj: deep_clone_expr(ast, map, o),
                name,
            }
        }
        Expr::OptIndex { obj, index } => {
            let (o, i) = (*obj, *index);
            Expr::OptIndex {
                obj: deep_clone_expr(ast, map, o),
                index: deep_clone_expr(ast, map, i),
            }
        }
        Expr::OptCall { callee, args } => {
            let (c, args) = (*callee, args.clone());
            Expr::OptCall {
                callee: deep_clone_expr(ast, map, c),
                args: args
                    .into_iter()
                    .map(|a| deep_clone_expr(ast, map, a))
                    .collect(),
            }
        }
        Expr::PostIncr {
            target,
            is_inc,
            is_prefix,
        } => {
            let t = *target;
            let (is_inc, is_prefix) = (*is_inc, *is_prefix);
            Expr::PostIncr {
                target: deep_clone_expr(ast, map, t),
                is_inc,
                is_prefix,
            }
        }
        Expr::As { expr, ty_ann } => {
            let e = *expr;
            let ty_ann = ty_ann.clone();
            Expr::As {
                expr: deep_clone_expr(ast, map, e),
                ty_ann,
            }
        }
        Expr::Sequence { left, right } => {
            let l = *left;
            let r = *right;
            Expr::Sequence {
                left: deep_clone_expr(ast, map, l),
                right: deep_clone_expr(ast, map, r),
            }
        }
        _ => return None,
    })
}

/// Container arms — `Expr::Array` / `Expr::ObjectLit` / `Expr::ArrowFn`
/// — share the "clone the outer container and deep-clone each element
/// / field-value / body-stmt" shape. Extracted so the main
/// `deep_clone_expr` match keeps its arm-per-variant scan under the
/// 200-line hard limit. Returns `Some(new_expr)` on hit; `None` when
/// `eid` names any other Expr variant (main fn falls through to the
/// remaining arm-per-variant match).
fn clone_container_expr(
    ast: &mut Ast,
    map: &mut Vec<(ExprId, ExprId)>,
    eid: ExprId,
) -> Option<Expr> {
    match ast.get_expr(eid) {
        Expr::Array(els) => {
            let els = els.clone();
            Some(Expr::Array(
                els.into_iter()
                    .map(|e| deep_clone_expr(ast, map, e))
                    .collect(),
            ))
        }
        Expr::ObjectLit { fields } => {
            let fields = fields.clone();
            Some(Expr::ObjectLit {
                fields: fields
                    .into_iter()
                    .map(|(n, e)| (n, deep_clone_expr(ast, map, e)))
                    .collect(),
            })
        }
        Expr::ArrowFn {
            params,
            return_type,
            body,
        } => {
            let params = params.clone();
            let return_type = return_type.clone();
            let body: Vec<Stmt> = body.to_vec();
            // Arrow fn body stmts may carry ExprIds — but at this point
            // arrows are already lifted by lift_arrow_fns in normal
            // pipeline. Defensive: deep-clone each stmt.
            Some(Expr::ArrowFn {
                params,
                return_type,
                body: body.iter().map(|s| deep_clone_stmt(ast, map, s)).collect(),
            })
        }
        _ => None,
    }
}
