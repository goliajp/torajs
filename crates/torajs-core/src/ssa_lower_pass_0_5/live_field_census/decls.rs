//! The census's construction: the pass over the declarations, the
//! constructor prefixes and the factory seeds, before the pass over
//! the expressions.

use std::collections::HashMap as StdMap;

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, ExprId, Stmt};
use crate::check::Type;
use crate::num_width::WidthTable;
use crate::ssa::id_hash::{HashMap, HashSet};

use super::walk::{ctor_class, ctor_fed_seed, ctor_prefix, nested_stmts, user_params};
use super::{Census, Ctor, ExprMask, Live, bindings, scope};

/// `retargets` names the specialization a generic call enters, which
/// is the function whose parameters its arguments fill.
pub(crate) fn program_live(
    ast: &Ast,
    expr_types: &StdMap<ExprId, Type>,
    (widths, retargets): (&WidthTable, &StdMap<ExprId, String>),
    dead_twins: &ExprMask,
) -> Live {
    let twins_open = !ast.cmany_twins.is_empty() && dead_twins.is_empty();
    let census = Census::collect(
        ast,
        expr_types,
        (widths, retargets),
        (dead_twins, twins_open),
    );
    census.solve(!twins_open && !census.road)
}

/// Each expression's checked type, by arena slot.
fn type_table<'a>(ast: &Ast, expr_types: &'a StdMap<ExprId, Type>) -> Vec<Option<&'a Type>> {
    let mut types = vec![None; ast.exprs.len()];
    for (e, t) in expr_types {
        if let Some(slot) = types.get_mut(e.0 as usize) {
            *slot = Some(t);
        }
    }
    types
}

impl<'a> Census<'a> {
    fn collect(
        ast: &'a Ast,
        expr_types: &'a StdMap<ExprId, Type>,
        (widths, retargets): (&'a WidthTable, &'a StdMap<ExprId, String>),
        (dead, twins_open): (&ExprMask, bool),
    ) -> Self {
        let mut c = Census {
            ast,
            types: type_table(ast, expr_types),
            widths,
            retargets,
            road: false,
            ctors: Vec::new(),
            fns: HashMap::default(),
            let_inits: HashMap::default(),
            bound: HashSet::default(),
            ctor_params: HashMap::default(),
            fn_params: HashMap::default(),
            calls: bindings::Calls::default(),
            scopes: scope::Scopes::default(),
            judged_reads: Vec::new(),
            direct_calls: Vec::new(),
            disc_test: false,
            assigned: HashSet::default(),
            assign_values: HashMap::default(),
            member_writes: HashMap::default(),
            deleted: HashSet::default(),
            foreign: HashSet::default(),
            foreign_classes: ast
                .ctor_return_override
                .iter()
                .map(String::as_str)
                .collect(),
            ctor_exprs: ExprMask::new(ast),
            prefix: Vec::new(),
            construct_args: HashMap::default(),
            unread_seeds: HashMap::default(),
        };
        for s in &ast.stmts {
            c.collect_stmt(s);
        }
        let params = (c.fns.values().map(|(p, _)| p.len()))
            .chain(c.ctors.iter().map(|k| k.params.len()))
            .sum();
        let binders = c.let_inits.len() + c.bound.len();
        c.scopes = scope::Scopes::collect(ast, (c.fns.len() + c.ctors.len(), params, binders));
        c.mark_ctor_exprs();
        c.collect_exprs(dead, twins_open);
        c
    }

    /// Marks the expressions of the constructor bodies of classes
    /// only instances reach.
    fn mark_ctor_exprs(&mut self) {
        for ctor in &self.ctors {
            if self.foreign_classes.contains(ctor.class) {
                continue;
            }
            for s in ctor.body {
                visit_stmt_exprs(s, &mut |e| add_tree(self.ast, e, &mut self.ctor_exprs));
            }
        }
    }

    pub(super) fn ty(&self, e: ExprId) -> Option<&'a Type> {
        self.types.get(e.0 as usize).copied().flatten()
    }

    /// A name some binding other than a function declaration may
    /// stand for.
    pub(super) fn shadowed(&self, name: &str) -> bool {
        self.bound.contains(name)
            || self.let_inits.contains_key(name)
            || self.ctor_params.contains_key(name)
            || self.scopes.param_names.contains(name)
    }

    /// Declarations: functions, constructors, bindings.
    pub(super) fn collect_stmt(&mut self, s: &'a Stmt) {
        match s {
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                if let Some(class) = ctor_class(name) {
                    let user = user_params(params);
                    for (i, p) in user.iter().enumerate() {
                        let at = self.ctor_params.entry(&p.name).or_default();
                        at.push((class, i));
                    }
                    self.ctors.push(Ctor {
                        name,
                        class,
                        params,
                        user,
                        body,
                    });
                } else {
                    // the other parameters are scoped to their function
                    // (`scope::Scopes`)
                    if bindings::judged_fn(name, params) {
                        for (i, p) in params.iter().enumerate() {
                            let at = self.fn_params.entry(&p.name).or_default();
                            at.push((name, i));
                        }
                    }
                    if self.fns.insert(name, (params, body)).is_some() {
                        self.assigned.insert(name);
                    }
                }
                for st in body {
                    self.collect_stmt(st);
                }
            }
            Stmt::LetDecl { name, init, .. } => {
                self.let_inits.entry(name).or_default().push(*init);
            }
            Stmt::UsingDecl { name: n, .. }
            | Stmt::YieldInto { var: n, .. }
            | Stmt::ForOf { var_name: n, .. }
            | Stmt::ForOfSplitIter { var_name: n, .. }
            | Stmt::Try {
                catch_param: Some(n),
                ..
            } => {
                self.bound.insert(n);
            }
            _ => {}
        }
        nested_stmts(s, &mut |st| self.collect_stmt(st));
    }

    /// The constructor prefix writes, recorded and excluded from the
    /// general write census.
    pub(super) fn prefix_assigns(&mut self) -> ExprMask {
        let mut out = ExprMask::new(self.ast);
        for ctor in &self.ctors {
            if self.foreign_classes.contains(ctor.class) {
                continue;
            }
            for (e, w) in ctor_prefix(self.ast, (ctor.name, ctor.class), ctor.params, ctor.body) {
                out.insert(e);
                self.prefix.push(w);
            }
        }
        out
    }

    /// The factory seeds of the prefix-written fields, by literal.
    pub(super) fn unread_seeds(&self) -> HashMap<ExprId, HashSet<&'a str>> {
        let mut out: HashMap<ExprId, HashSet<&'a str>> = HashMap::default();
        for s in &self.ast.stmts {
            let Stmt::FnDecl { name, body, .. } = s else {
                continue;
            };
            let Some(class) = name.strip_prefix("__new_") else {
                continue;
            };
            if self.foreign_classes.contains(class) {
                continue;
            }
            let Some(seed) = ctor_fed_seed(self.ast, class, body) else {
                continue;
            };
            let fields = (self.prefix.iter())
                .filter(|w| w.class == class)
                .map(|w| w.field);
            out.entry(seed).or_default().extend(fields);
        }
        out
    }
}

fn add_tree(ast: &Ast, e: ExprId, out: &mut ExprMask) {
    if out.insert(e) {
        visit_expr_children(ast, e, &mut |c| add_tree(ast, c, out));
    }
}
