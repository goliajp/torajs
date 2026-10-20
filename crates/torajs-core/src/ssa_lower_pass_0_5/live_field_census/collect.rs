//! The census's pass over every expression of the program: the writes,
//! deletions, constructions and casts that open or close a road to a
//! field.

use std::borrow::Cow;

use crate::ast::{BinOp, Expr, ExprId, Stmt};
use crate::check::Type;

use super::bindings::{CallsTally, holds_nullish_in_band};
use super::walk::{derives_from, factory_class, key_is_numeric, never_an_instance};
use super::{Census, ExprMask, ROADS};

impl<'a> Census<'a> {
    /// One expression of the arena (`live` when no dead twin holds it),
    /// for every census its kind feeds. `arrows` takes the arrow
    /// bodies, whose declarations the caller collects after the pass.
    pub(super) fn collect_expr(
        &mut self,
        (id, e): (ExprId, &'a Expr),
        (live, prefix): (bool, &ExprMask),
        calls: &mut CallsTally<'a>,
        arrows: &mut Vec<&'a [Stmt]>,
    ) {
        match e {
            Expr::Ident(n) => {
                self.judge_read(id);
                if live {
                    calls.ident(n, &self.fns);
                    self.road |= ROADS.contains(&n.as_str());
                }
            }
            Expr::Call { callee, args } => {
                let f = match self.ast.get_expr(*callee) {
                    Expr::Ident(f) => Some(f.as_str()),
                    _ => None,
                };
                if f.is_some() {
                    self.direct_calls.push(id);
                }
                self.judge_read(id);
                if let (true, Some(f)) = (live, f) {
                    calls.call((id, f, args), (self.retargets, &self.fns));
                    // `new C(..)` of a user class is a call of its factory
                    if let Some(class) = factory_class(f) {
                        self.construct_args.entry(class).or_default().push(args);
                    }
                }
            }
            Expr::Closure { fn_name, captures } => {
                self.scopes.closure(id, fn_name, captures);
                if live {
                    calls.value_use(fn_name);
                }
            }
            Expr::BinOp {
                op: BinOp::Eq | BinOp::LooseEq,
                left,
                right,
            } => {
                let member = |x: &ExprId| {
                    matches!(self.ast.get_expr(*x),
                        Expr::Member { obj, .. } if matches!(self.ast.get_expr(*obj), Expr::Ident(_)))
                };
                self.disc_test |= member(left) || member(right);
            }
            _ if !live => {}
            Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
                if ROADS.contains(&name.as_str()) {
                    self.road = true;
                }
            }
            Expr::String(s) => {
                if ROADS.iter().any(|n| s == *n) {
                    self.road = true;
                }
            }
            // the constructor prefix writes are recorded apart
            Expr::Assign { .. } if prefix.contains(id) => {}
            Expr::Assign { target, value } => match self.ast.get_expr(*target) {
                // a write through a receiver that is not a class
                // instance by type (`any`, a struct) may store anything
                Expr::Member { obj, name } => {
                    if matches!(self.ty(*obj), Some(Type::ClassRef(_))) {
                        self.member_writes.entry(name).or_default().push(*value);
                    } else {
                        self.foreign.insert(name);
                    }
                }
                Expr::Ident(n) => {
                    self.assigned.insert(n);
                    self.assign_values.entry(n).or_default().push(*value);
                }
                Expr::Index { obj, index } => {
                    if !key_is_numeric(self.ty(*index)) && !never_an_instance(self.ty(*obj)) {
                        self.road = true;
                    }
                }
                _ => self.road = true,
            },
            Expr::Delete { expr } => match self.ast.get_expr(*expr) {
                Expr::Member { name, .. } => {
                    self.deleted.insert(name);
                }
                _ => self.road = true,
            },
            Expr::New {
                class_name, args, ..
            } => {
                calls.value_use(class_name);
                let class = class_name.split('<').next().unwrap_or(class_name);
                self.construct_args.entry(class).or_default().push(args);
            }
            Expr::NewDynamic { .. } => self.road = true,
            Expr::ObjectLit { fields } => {
                let unread = self.unread_seeds.get(&id).cloned();
                for (k, v) in fields {
                    if unread
                        .as_ref()
                        .is_some_and(|u| u.contains(k.lossy().as_ref()))
                    {
                        continue;
                    }
                    match self.ast.get_expr(*v) {
                        Expr::Spread { .. } => self.road = true,
                        // a scalar never fills a pointer-shaped field
                        _ if matches!(
                            self.ty(*v),
                            Some(Type::Number | Type::Boolean | Type::String)
                        ) => {}
                        _ => match k.lossy() {
                            Cow::Borrowed(k) => self.member_writes.entry(k).or_default().push(*v),
                            // a name no identifier spells
                            Cow::Owned(_) => self.road = true,
                        },
                    }
                }
            }
            Expr::As { expr, .. } => {
                if let Some(Type::ClassRef(c)) = self.ty(id) {
                    let c = c.split('<').next().unwrap_or(c);
                    // `x!` and `x as C` over `C | null` pass a null along,
                    // never an object of another shape
                    let from = match self.ty(*expr) {
                        Some(Type::Nullable(t)) => Some(&**t),
                        t => t,
                    };
                    let instance = match from {
                        Some(Type::Any) => true,
                        Some(Type::ClassRef(d)) => {
                            let d = d.split('<').next().unwrap_or(d);
                            d == c || derives_from(self.ast, d, c)
                        }
                        _ => false,
                    };
                    if !instance {
                        self.foreign_classes.insert(c);
                    }
                }
            }
            Expr::ArrowFn { params, body, .. } => {
                for p in params {
                    self.bound.insert(&p.name);
                }
                arrows.push(body);
            }
            _ => {}
        }
    }

    /// A read or call whose value slot spells a nullish value in band,
    /// which the binding census judges.
    fn judge_read(&mut self, id: ExprId) {
        if self.ty(id).is_some_and(holds_nullish_in_band) {
            self.judged_reads.push(id);
        }
    }

    /// The one pass over the expressions: every expression for the
    /// scopes, the reads the census judges and the direct calls; every
    /// live one for the calls; every live one but the constructor
    /// prefix writes, which are recorded apart (only where no twin is
    /// open, since a field is live only there), for the writes and
    /// roads. The declarations of an arrow body join after the pass,
    /// so the calls count the functions declared before it, as the
    /// pass over the declarations found them.
    pub(super) fn collect_exprs(&mut self, dead: &ExprMask, twins_open: bool) {
        let mut prefix_writes = ExprMask::default();
        if !twins_open {
            prefix_writes = self.prefix_assigns();
            self.unread_seeds = self.unread_seeds();
        }
        let ast = self.ast;
        let mut calls = CallsTally::new(&self.fns);
        let mut arrows = Vec::new();
        for (i, e) in ast.exprs.iter().enumerate() {
            let id = ExprId(i as u32);
            let live = !dead.contains(id);
            self.collect_expr((id, e), (live, &prefix_writes), &mut calls, &mut arrows);
        }
        self.calls = calls.finish(ast, dead, (self.retargets, &self.fns));
        for body in arrows {
            body.iter().for_each(|st| self.collect_stmt(st));
        }
    }
}
