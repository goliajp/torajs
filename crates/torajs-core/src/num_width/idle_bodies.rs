//! The expressions that never run: the bodies of the method twins no
//! call can enter (`TwinBodies::idle`), and of the `any`-widened clones
//! (`<f>$$anyw<k>…`, `check_monomorph_any_widen`) every call of which
//! sits in such a body. Such a clone is entered only through the call
//! sites retargeted to it, so a clone whose sites are all idle is idle
//! too, and a clone it calls in turn may then be.
//!
//! A cast of an `any` value in an idle body never copies a dynamic
//! object into a cell, so it seeds no field face (`any_object_seed`):
//! the widened constructor a twin's `new C(..)` calls would otherwise
//! give every number field of `C` the F64 face for writes that cannot
//! happen.

use std::collections::HashMap;

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, ExprId, Stmt};
use crate::check::Type;
use crate::ssa_lower_pass_0_5::arr_head_census::TwinBodies;

pub(super) struct IdleBodies(Vec<bool>);

impl IdleBodies {
    pub(super) fn contains(&self, e: ExprId) -> bool {
        self.0.get(e.0 as usize).is_some_and(|b| *b)
    }

    pub(super) fn of(
        ast: &Ast,
        retargets: &HashMap<ExprId, String>,
        expr_types: &HashMap<ExprId, Type>,
    ) -> Self {
        let twins = TwinBodies::of(ast).idle(expr_types);
        let mut idle = IdleBodies(
            (0..ast.exprs.len())
                .map(|i| twins.contains(ExprId(i as u32)))
                .collect(),
        );
        if !idle.0.contains(&true) {
            return idle;
        }
        let mut sites: HashMap<&str, Vec<ExprId>> = HashMap::new();
        for (eid, name) in retargets {
            if is_widened_clone(ast, name) {
                sites.entry(name.as_str()).or_default().push(*eid);
            }
        }
        loop {
            let settled: Vec<&str> = (sites.iter())
                .filter(|(_, at)| at.iter().all(|e| idle.contains(*e)))
                .map(|(name, _)| *name)
                .collect();
            if settled.is_empty() {
                return idle;
            }
            for name in settled {
                sites.remove(name);
                for s in &ast.stmts {
                    idle.mark_fn(ast, s, name);
                }
            }
        }
    }

    fn mark_fn(&mut self, ast: &Ast, s: &Stmt, want: &str) {
        match s {
            Stmt::FnDecl {
                name, params, body, ..
            } if name == want => {
                let mut mark = |e: ExprId| self.mark_tree(ast, e);
                for d in params.iter().filter_map(|p| p.default) {
                    mark(d);
                }
                for st in body {
                    visit_stmt_exprs(st, &mut mark);
                }
            }
            Stmt::Multi(v) | Stmt::Block(v) => {
                for st in v {
                    self.mark_fn(ast, st, want);
                }
            }
            _ => {}
        }
    }

    fn mark_tree(&mut self, ast: &Ast, e: ExprId) {
        if !std::mem::replace(&mut self.0[e.0 as usize], true) {
            visit_expr_children(ast, e, &mut |c| self.mark_tree(ast, c));
        }
    }
}

/// A clone the checker minted for the `any` arguments of one call site,
/// which only that site's retarget names: `$$anyw` and a parameter
/// index. The variadic (`$$anywv`) and escape (`$$anyws`) faces are also
/// entered through dispatch rows and boxed adapters.
fn is_widened_clone(ast: &Ast, name: &str) -> bool {
    ast.any_widen_clones.contains(name)
        && name
            .rsplit_once("$$anyw")
            .is_some_and(|(_, k)| k.starts_with(|c: char| c.is_ascii_digit()))
}
