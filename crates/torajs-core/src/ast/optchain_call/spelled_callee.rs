//! `(a, f)(x)` and `(c ? f : g)(x)` — a callee spelled through a
//! comma or a conditional, distributed into the call.
//!
//! §13.3.6.2: the callee of such a call is no Reference, so the
//! function runs with an `undefined` receiver — exactly what a direct
//! call through the bare name passes (a binding's environment record
//! supplies no `this`). The rewrite moves the call onto the operand
//! that produces the callee:
//!
//! ```text
//! (a, f)(x)          → (a, f(x))
//! (c ? f : g)(x)     → c ? f(x) : g(x)
//! (a, f).call(t)     → (a, f.call(t))      (also .apply / .bind)
//! (a ?? g)(x)        → a == null ? g(x) : a(x)   (`a` a name)
//! f?.(x)             → f == null ? undefined : f(x)   (see [`opt_call`])
//! ```
//!
//! Evaluation order is kept — the operands before the callee, then the
//! callee, then the arguments, then the call — and in the conditional
//! form the arguments sit in two branches of which one runs. What it
//! buys is that every pass keyed on a direct call (the receiver seed of
//! a function expression reading `this`, the static call lanes) sees
//! one; until now the program was refused with "fnexpr this in
//! unclaimed receiver position".
//!
//! Only a bare name is called at a leaf (an `as` cast around it
//! stays): `(0, o.m)()` passes `undefined` where `o.m()` passes `o`.
//! `eval` is not a name here — `(0, eval)(s)` is an indirect eval.
//! The conditional form copies the arguments into its second branch,
//! so it declines when one of them holds a function body or a tagged
//! template. An optional call (`(c ? f : g)?.(x)`) stays as written:
//! its guard tests the callee value as a whole. A callee none of whose
//! leaves names a function expression reading `this` stays as written
//! too (see [`receiver_names`]), unless the call carries explicit type
//! arguments, which a union-typed call cannot apply. Runs after
//! `desugar_with` (a name inside `with` no longer stands for an object
//! property by then).

use crate::ast::clone_body::BodyCloner;
use crate::ast::{Ast, BinOp, Expr, ExprId};

mod opt_call;
mod receiver_names;
mod shape;

use shape::{
    holds_site, leaf_count, leaves_are_names, names_a_leaf, neutralize, nullish_name, spelled,
};

pub fn distribute_spelled_callees(ast: &mut Ast) {
    // the census walks the whole program; most programs spell no
    // callee this way and never pay for it
    let mut wanted = None;
    for i in 0..ast.exprs.len() {
        if let Expr::OptCall { callee, .. } = &ast.exprs[i] {
            if let Expr::Ident(_) = ast.get_expr(*callee) {
                let callee = *callee;
                let wanted = wanted.get_or_insert_with(|| receiver_names::Receivers::of(ast));
                if wanted.names(ast, callee) {
                    opt_call::guard(ast, i);
                }
            }
            continue;
        }
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        // `(c ? f : g)?.call(t)` — the optional-chain guard is keyed on
        // this call and tests the whole callee value; distributing the
        // call would leave the guard nothing to short-circuit
        if ast.optchain_calls.contains_key(&ExprId(i as u32)) {
            continue;
        }
        let (callee, args) = (*callee, args.clone());
        let (base, method) = match ast.get_expr(callee) {
            Expr::Member { obj, name } if matches!(name.as_str(), "call" | "apply" | "bind") => {
                (*obj, Some(name.clone()))
            }
            _ => (callee, None),
        };
        // explicit type arguments have no union-call reading: each
        // leaf instantiates them on its own
        let typed = ast.call_type_args.contains_key(&ExprId(i as u32));
        if !spelled(ast, base) || !leaves_are_names(ast, base) {
            continue;
        }
        if !typed {
            let wanted = wanted.get_or_insert_with(|| receiver_names::Receivers::of(ast));
            if !names_a_leaf(ast, base, wanted) {
                continue;
            }
        }
        let branches = leaf_count(ast, base);
        if branches > 1 && args.iter().any(|&a| holds_site(ast, a)) {
            continue;
        }
        let span = ast.expr_spans[i];
        // `(c ? f : g)<T>(x)` — each distributed call takes the type
        // arguments the program wrote on the original
        let type_args = ast.call_type_args.remove(&ExprId(i as u32));
        let mut rw = Rewrite {
            args,
            used: false,
            method,
            type_args,
            span,
        };
        let root = rw.distribute(ast, base);
        ast.exprs[i] = std::mem::replace(&mut ast.exprs[root.0 as usize], Expr::Null);
        neutralize(ast, base);
        if callee != base {
            ast.exprs[callee.0 as usize] = Expr::Null;
        }
    }
}

struct Rewrite {
    args: Vec<ExprId>,
    /// The original argument nodes went to a leaf already; later
    /// leaves take copies.
    used: bool,
    method: Option<String>,
    type_args: Option<Vec<String>>,
    span: crate::lexer::Span,
}

impl Rewrite {
    fn add(&self, ast: &mut Ast, e: Expr) -> ExprId {
        let id = ast.add_expr(e);
        ast.set_expr_span(id, self.span);
        id
    }

    fn distribute(&mut self, ast: &mut Ast, e: ExprId) -> ExprId {
        match ast.get_expr(e).clone() {
            Expr::Sequence { left, right } => {
                let right = self.distribute(ast, right);
                self.add(ast, Expr::Sequence { left, right })
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                let then_branch = self.distribute(ast, then_branch);
                let else_branch = self.distribute(ast, else_branch);
                self.add(
                    ast,
                    Expr::Ternary {
                        cond,
                        then_branch,
                        else_branch,
                    },
                )
            }
            Expr::Nullish { lhs, rhs } => {
                // `(a ?? g)(x)` → `a == null ? g(x) : a(x)` (§13.15.4:
                // `a` when it is neither undefined nor null)
                let name = nullish_name(ast, lhs).expect("operands admits a named lhs only");
                let read = self.add(ast, Expr::Ident(name));
                let null = self.add(ast, Expr::Null);
                let cond = self.add(
                    ast,
                    Expr::BinOp {
                        op: BinOp::LooseEq,
                        left: read,
                        right: null,
                    },
                );
                let then_branch = self.distribute(ast, rhs);
                let else_branch = self.distribute(ast, lhs);
                self.add(
                    ast,
                    Expr::Ternary {
                        cond,
                        then_branch,
                        else_branch,
                    },
                )
            }
            Expr::As { expr, ty_ann } if spelled(ast, expr) => {
                // `((a, f) as T)(x)` — the cast is type-only and
                // types the callee value, which is the leaf's value
                self.push_cast(ast, expr, &ty_ann);
                self.distribute(ast, expr)
            }
            _ => self.leaf(ast, e),
        }
    }

    /// Wrap every leaf under `e` in `as ty`, in place.
    fn push_cast(&self, ast: &mut Ast, e: ExprId, ty: &str) {
        if spelled(ast, e) {
            for k in shape::operands(ast, e).unwrap_or_default() {
                self.push_cast(ast, k, ty);
            }
            return;
        }
        let inner = ast.add_expr(ast.get_expr(e).clone());
        ast.set_expr_span(inner, ast.expr_spans[e.0 as usize]);
        ast.exprs[e.0 as usize] = Expr::As {
            expr: inner,
            ty_ann: ty.to_string(),
        };
    }

    fn leaf(&mut self, ast: &mut Ast, callee: ExprId) -> ExprId {
        let args = if self.used {
            let mut cloner = BodyCloner::new(ast);
            let copies = self.args.iter().map(|&a| cloner.clone_expr(a)).collect();
            crate::ast::clone_body_tables::migrate(&mut cloner);
            copies
        } else {
            self.used = true;
            self.args.clone()
        };
        let callee = match &self.method {
            Some(name) => self.add(
                ast,
                Expr::Member {
                    obj: callee,
                    name: name.clone(),
                },
            ),
            None => callee,
        };
        let call = self.add(ast, Expr::Call { callee, args });
        if let Some(t) = &self.type_args {
            ast.call_type_args.insert(call, t.clone());
        }
        call
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `const f = function () { return this }`
    fn declare_this_fn(ast: &mut Ast) {
        let this = ast.add_expr(Expr::This);
        let f = ast.add_expr(Expr::ArrowFn {
            params: vec![],
            return_type: None,
            body: vec![crate::ast::Stmt::Return(Some(this))],
        });
        ast.fn_expr_exprs.insert(f);
        ast.stmts.push(crate::ast::Stmt::LetDecl {
            mutable: false,
            name: "f".into(),
            type_ann: None,
            init: f,
            is_var: false,
        });
    }

    /// `(0, <leaf>)(1)` — returns the ast and the call's id.
    fn comma_call(leaf: Expr) -> (Ast, ExprId) {
        let mut ast = Ast::default();
        declare_this_fn(&mut ast);
        let zero = ast.add_expr(Expr::Number(0.0));
        let leaf = ast.add_expr(leaf);
        let seq = ast.add_expr(Expr::Sequence {
            left: zero,
            right: leaf,
        });
        let one = ast.add_expr(Expr::Number(1.0));
        let call = ast.add_expr(Expr::Call {
            callee: seq,
            args: vec![one],
        });
        (ast, call)
    }

    #[test]
    fn a_comma_callee_moves_the_call_onto_its_last_operand() {
        let (mut ast, call) = comma_call(Expr::Ident("f".into()));
        distribute_spelled_callees(&mut ast);
        let Expr::Sequence { left, right } = ast.get_expr(call) else {
            panic!("expected a sequence");
        };
        assert!(matches!(ast.get_expr(*left), Expr::Number(n) if *n == 0.0));
        let Expr::Call { callee, args } = ast.get_expr(*right) else {
            panic!("expected a call");
        };
        assert!(matches!(ast.get_expr(*callee), Expr::Ident(n) if n == "f"));
        assert_eq!(args.len(), 1);
    }

    #[test]
    fn a_member_or_eval_leaf_stays_as_written() {
        let (mut ast, call) = comma_call(Expr::Ident("eval".into()));
        distribute_spelled_callees(&mut ast);
        assert!(matches!(ast.get_expr(call), Expr::Call { .. }));

        let mut ast = Ast::default();
        declare_this_fn(&mut ast);
        let o = ast.add_expr(Expr::Ident("o".into()));
        let m = ast.add_expr(Expr::Member {
            obj: o,
            name: "m".into(),
        });
        let zero = ast.add_expr(Expr::Number(0.0));
        let seq = ast.add_expr(Expr::Sequence {
            left: zero,
            right: m,
        });
        let call = ast.add_expr(Expr::Call {
            callee: seq,
            args: vec![],
        });
        distribute_spelled_callees(&mut ast);
        assert!(matches!(ast.get_expr(call), Expr::Call { callee, .. } if *callee == seq));
    }

    #[test]
    fn a_leaf_without_a_receiver_reading_function_stays_as_written() {
        let (mut ast, call) = comma_call(Expr::Ident("g".into()));
        distribute_spelled_callees(&mut ast);
        assert!(matches!(ast.get_expr(call), Expr::Call { .. }));
    }

    #[test]
    fn a_nullish_callee_becomes_a_test_of_its_left_name() {
        let mut ast = Ast::default();
        declare_this_fn(&mut ast);
        let a = ast.add_expr(Expr::Ident("a".into()));
        let f = ast.add_expr(Expr::Ident("f".into()));
        let callee = ast.add_expr(Expr::Nullish { lhs: a, rhs: f });
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![],
        });
        distribute_spelled_callees(&mut ast);
        let Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } = ast.get_expr(call).clone()
        else {
            panic!("expected a conditional");
        };
        assert!(matches!(
            ast.get_expr(cond),
            Expr::BinOp {
                op: BinOp::LooseEq,
                ..
            }
        ));
        let callee_of = |e: ExprId| match ast.get_expr(e) {
            Expr::Call { callee, .. } => *callee,
            other => panic!("expected a call, got {other:?}"),
        };
        assert_eq!(callee_of(then_branch), f);
        assert_eq!(callee_of(else_branch), a);
    }

    #[test]
    fn an_optional_call_of_the_name_tests_it_first() {
        let mut ast = Ast::default();
        declare_this_fn(&mut ast);
        let f = ast.add_expr(Expr::Ident("f".into()));
        let call = ast.add_expr(Expr::OptCall {
            callee: f,
            args: vec![],
        });
        distribute_spelled_callees(&mut ast);
        let Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } = ast.get_expr(call)
        else {
            panic!("expected a conditional");
        };
        assert!(matches!(ast.get_expr(*then_branch), Expr::Ident(n) if n == "undefined"));
        assert!(matches!(ast.get_expr(*else_branch), Expr::Call { callee, .. } if *callee == f));
    }
}
