//! The census's pass over every expression of the program: the writes,
//! deletions, constructions and casts that open or close a road to a
//! field.

use std::borrow::Cow;

use crate::ast::{Expr, ExprId};
use crate::check::Type;

use super::walk::{derives_from, factory_class, key_is_numeric, never_an_instance};
use super::{Census, ROADS};

impl<'a> Census<'a> {
    pub(super) fn collect_expr(&mut self, id: ExprId, e: &'a Expr) {
        match e {
            Expr::Ident(n) => {
                if ROADS.contains(&n.as_str()) {
                    self.road = true;
                }
            }
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
            // `new C(..)` of a user class is a call of its factory
            Expr::Call { callee, args } => {
                if let Expr::Ident(n) = self.ast.get_expr(*callee)
                    && let Some(class) = factory_class(n)
                {
                    self.construct_args.entry(class).or_default().push(args);
                }
            }
            Expr::ArrowFn { params, body, .. } => {
                for p in params {
                    self.bound.insert(&p.name);
                }
                for st in body {
                    self.collect_stmt(st);
                }
            }
            _ => {}
        }
    }
}
