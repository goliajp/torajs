//! The expression half of the nested-fn ident rewrite: every operand
//! position under one expression, and the arrow bodies that see the
//! renamed binding.

use super::super::{Ast, Expr, ExprId};
use super::{arrow_rebinds, rewrite_idents_in_body};
use std::collections::HashMap;

pub(in crate::ast) fn rewrite_idents_in_expr(
    ast: &mut Ast,
    eid: ExprId,
    renames: &HashMap<String, String>,
    arrows: bool,
) {
    use std::collections::HashSet;
    let mut seen: HashSet<ExprId> = HashSet::new();
    let mut stack: Vec<ExprId> = vec![eid];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let new_name = if let Expr::Ident(n) = &ast.exprs[id.0 as usize] {
            renames.get(n).cloned()
        } else {
            None
        };
        if let Some(nm) = new_name {
            ast.exprs[id.0 as usize] = Expr::Ident(nm);
            continue;
        }
        match ast.exprs[id.0 as usize].clone() {
            Expr::BinOp { left, right, .. } => {
                stack.push(left);
                stack.push(right);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::PostIncr { target: expr, .. } => {
                stack.push(expr);
            }
            Expr::Call { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                stack.push(obj);
            }
            Expr::OptIndex { obj, index } | Expr::Index { obj, index } => {
                stack.push(obj);
                stack.push(index);
            }
            Expr::OptCall { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            Expr::Assign { target, value } => {
                stack.push(target);
                stack.push(value);
            }
            Expr::Array(els) => {
                for e in els {
                    stack.push(e);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    stack.push(e);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                stack.push(cond);
                stack.push(then_branch);
                stack.push(else_branch);
            }
            Expr::Nullish { lhs, rhs }
            | Expr::Sequence {
                left: lhs,
                right: rhs,
            } => {
                stack.push(lhs);
                stack.push(rhs);
            }
            // `new F()` reaches here as `NewDynamic` whenever F is
            // neither a class nor an intercepted builtin
            // (`route_non_class_new`, which runs first, rewrites it) —
            // which is every lifted nested function. Its callee is an
            // operand, so all that was missing is the descent; without
            // it the constructor position kept the pre-lift spelling,
            // loud as `unknown identifier` when nothing else bound the
            // name. The `Expr::New` arm below stays operand-only: the
            // only spellings that survive that rewrite are classes and
            // intercepted builtins, and a lifted `function` never ends
            // up in this map under either — a shadowed intrinsic is
            // taken over by `desugar_builtin_new` long before the lift
            // (a separate hole, registered).
            Expr::NewDynamic { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            // A cast and a `delete` each wrap one operand. `new (F as
            // any)()` is the spelling a TS fixture needs when the
            // constructor is a plain function.
            Expr::As { expr, .. } | Expr::Delete { expr } => {
                stack.push(expr);
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    stack.push(a);
                }
            }
            Expr::InstanceOf { expr, rhs } => {
                stack.push(expr);
                stack.push(rhs);
            }
            // An arrow body is a separate scope, but a nested scope
            // SEES its enclosing bindings — and renaming a nested
            // `function f` is a rename of one, WHEN the rename is a
            // hoist into the enclosing function scope (`arrows`).
            // Pass 2's module-top-level blocks are the other case: in
            // strict mode `{ function f(){} }` at global scope is
            // block-scoped and nothing outside the block resolves it,
            // so the descent must NOT happen there — doing it made
            // `assert.throws(ReferenceError, function() { f; })`
            // resolve and stop throwing (test262
            // `global-code/{block,switch-case,switch-dflt}-decl-strict`).
            // Skipping the descent
            // left `(x) => step(x)` calling a `step` that no longer
            // exists under that name once the declaration lifted to
            // `__nested_<parent>_step_N`; `lift_arrow_fns` moves the
            // arrow to top level before this pass runs, so nothing
            // downstream was in a position to fix it either
            // ("handled by their own pass" was not true of this
            // case). Names the arrow rebinds are dropped from the map
            // first — those references are its own.
            Expr::ArrowFn { params, body, .. } if arrows => {
                let sub: HashMap<String, String> = renames
                    .iter()
                    .filter(|(k, _)| !arrow_rebinds(&params, &body, k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if !sub.is_empty() {
                    let mut inner = body;
                    rewrite_idents_in_body(ast, &mut inner, &sub, arrows);
                    if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[id.0 as usize] {
                        *slot = inner;
                    }
                }
            }
            _ => {}
        }
    }
}
