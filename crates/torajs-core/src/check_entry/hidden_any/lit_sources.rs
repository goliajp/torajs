//! The function declarations an object literal's member values are, as
//! far as the source shows them (`super::method_lits`).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The function declarations an object literal's member value is: a
/// closure, a function declared once whose return a round may retype,
/// a `const` binding declared once and never assigned whose initializer
/// is one, or a conditional both of whose arms are.
pub(super) struct Sources {
    consts: HashMap<String, ExprId>,
    fns: HashSet<String>,
}

impl Sources {
    pub(super) fn of(ast: &Ast) -> Self {
        let mut count: HashMap<String, usize> = HashMap::new();
        let mut consts = HashMap::new();
        let mut fns = HashSet::new();
        crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
            Stmt::LetDecl {
                name,
                mutable,
                init,
                ..
            } => {
                *count.entry(name.clone()).or_default() += 1;
                if !mutable {
                    consts.insert(name.clone(), *init);
                }
            }
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                is_generator,
                ..
            } => {
                *count.entry(name.clone()).or_default() += 1;
                for p in params {
                    *count.entry(p.name.clone()).or_default() += 1;
                }
                let scalar = |a: &str| matches!(a, "number" | "string" | "boolean");
                if !is_generator
                    && type_params.is_empty()
                    && !ast.async_fns.contains(name)
                    && return_type.as_deref().is_none_or(scalar)
                {
                    fns.insert(name.clone());
                }
            }
            Stmt::ForOf { var_name, .. }
            | Stmt::ForOfSplitIter { var_name, .. }
            | Stmt::UsingDecl { name: var_name, .. }
            | Stmt::Try {
                catch_param: Some(var_name),
                ..
            } => *count.entry(var_name.clone()).or_default() += 1,
            _ => {}
        });
        for e in &ast.exprs {
            if let Expr::Assign { target, .. } = e
                && let Expr::Ident(n) = ast.get_expr(*target)
            {
                *count.entry(n.clone()).or_default() += 1;
            }
        }
        let once = |n: &String| count.get(n) == Some(&1);
        consts.retain(|n, _| once(n));
        fns.retain(once);
        Sources { consts, fns }
    }

    /// The functions the value `v` is, or `None` when it may be another
    /// function value.
    pub(super) fn of_value(&self, ast: &Ast, v: ExprId) -> Option<Vec<String>> {
        let mut out = Vec::new();
        let mut todo = vec![v];
        // a chain of `const` bindings is no longer than the bindings
        let mut budget = self.consts.len() + 1;
        while let Some(v) = todo.pop() {
            match ast.get_expr(v) {
                Expr::Closure { fn_name, .. } => out.push(fn_name.clone()),
                Expr::Ident(n) if self.fns.contains(n) => out.push(n.clone()),
                Expr::Ident(n) if budget > 0 => {
                    budget -= 1;
                    todo.push(*self.consts.get(n)?);
                }
                Expr::Ternary {
                    then_branch,
                    else_branch,
                    ..
                } => todo.extend([*then_branch, *else_branch]),
                _ => return None,
            }
        }
        Some(out)
    }
}
