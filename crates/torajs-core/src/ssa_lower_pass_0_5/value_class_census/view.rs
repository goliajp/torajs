//! A `const t = xs[i]` view stays valid until its last read.
//!
//! An inline element is an address into the array's data buffer, and
//! the buffer moves when the array grows or dies. So from the binding
//! to each read of `t` nothing may run that can push to, reassign or
//! release an array of the class: no call (the class's own factory
//! excepted, which only builds a cell), no store into a slot holding
//! one, no delete, no read that can run user code (an accessor, a
//! `valueOf` behind an operator). The walk follows evaluation order;
//! a branch that returns or throws does not reach the code after the
//! `if`; a loop body, a `try`, a `switch` or a labelled block is
//! entered again (or left) with everything its body may have done.

use crate::ast::Stmt;

use super::program::{Holder, Program};

mod expr;

/// How many reads of `name` the rest of its scope makes, or why one of
/// them may see a moved buffer.
pub(super) fn reads(
    prog: &Program<'_>,
    h: &Holder<'_>,
    name: &str,
    rest: &[&Stmt],
) -> Result<usize, String> {
    let mut s = Scan {
        prog,
        h,
        name,
        hazard: false,
        any: false,
        reads: 0,
        stale: false,
        shadowed: 0,
    };
    s.seq(rest);
    if s.stale {
        return Err(format!(
            "a call or an array store can come between the view `{name}` and its read"
        ));
    }
    Ok(s.reads)
}

struct Scan<'p, 'a> {
    prog: &'p Program<'a>,
    h: &'p Holder<'a>,
    name: &'p str,
    hazard: bool,
    /// A hazard was met anywhere since the innermost region began.
    any: bool,
    reads: usize,
    stale: bool,
    /// How many enclosing blocks rebind `name`: their reads are of
    /// another binding.
    shadowed: usize,
}

/// The names `s` binds in the block it sits in.
pub(super) fn declared(s: &Stmt) -> Vec<&str> {
    match s {
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } | Stmt::FnDecl { name, .. } => {
            vec![name.as_str()]
        }
        Stmt::Multi(v) => v.iter().flat_map(declared).collect(),
        _ => Vec::new(),
    }
}

impl Scan<'_, '_> {
    fn set_hazard(&mut self) {
        self.hazard = true;
        self.any = true;
    }

    /// Run `f` as a region the program may re-enter or leave from any
    /// point: afterwards the state is the entry state joined with every
    /// hazard met inside.
    fn region(&mut self, passes: usize, f: &mut dyn FnMut(&mut Self)) {
        let entry = self.hazard;
        let outer_any = std::mem::replace(&mut self.any, false);
        for _ in 0..passes {
            f(self);
            self.hazard = entry || self.any;
        }
        self.any |= outer_any;
    }

    /// The statements in order; true when the path ends inside.
    fn seq(&mut self, list: &[&Stmt]) -> bool {
        let mut ended = false;
        for s in list {
            ended |= self.stmt(s);
        }
        ended
    }

    fn one(&mut self, s: &Stmt) -> bool {
        self.seq(&[s])
    }

    /// Run `f` inside a scope that binds `names`.
    fn scoped<'s, R>(
        &mut self,
        names: impl IntoIterator<Item = &'s str>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let shadows = names.into_iter().any(|n| n == self.name);
        self.shadowed += usize::from(shadows);
        let r = f(self);
        self.shadowed -= usize::from(shadows);
        r
    }

    /// A block's statements, in a scope of their own.
    fn block(&mut self, v: &[Stmt]) -> bool {
        let list: Vec<&Stmt> = v.iter().collect();
        self.scoped(v.iter().flat_map(declared), |s| s.seq(&list))
    }

    fn stmt(&mut self, s: &Stmt) -> bool {
        match s {
            Stmt::Expr(e) | Stmt::LetDecl { init: e, .. } | Stmt::UsingDecl { init: e, .. } => {
                self.expr(*e);
                false
            }
            Stmt::Yield(e) | Stmt::YieldInto { value: e, .. } => {
                self.expr(*e);
                self.set_hazard();
                false
            }
            Stmt::Return(v) => {
                if let Some(e) = v {
                    self.expr(*e);
                }
                true
            }
            Stmt::Throw(e) => {
                self.expr(*e);
                true
            }
            Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::Block(v) => self.block(v),
            Stmt::Multi(v) => self.seq(&v.iter().collect::<Vec<_>>()),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond);
                let entry = self.hazard;
                let t_end = self.one(then_branch);
                let t_h = self.hazard;
                self.hazard = entry;
                let e_end = match else_branch {
                    Some(e) => self.one(e),
                    None => false,
                };
                let e_h = self.hazard;
                self.hazard = match (t_end, e_end) {
                    (true, false) => e_h,
                    (false, true) => t_h,
                    _ => t_h || e_h,
                };
                t_end && e_end
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.region(2, &mut |s| {
                    s.expr(*cond);
                    s.one(body);
                });
                false
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let head = init.as_deref().map(declared).unwrap_or_default();
                self.scoped(head, |s| {
                    if let Some(i) = init {
                        s.one(i);
                    }
                    s.region(2, &mut |s| {
                        if let Some(c) = cond {
                            s.expr(*c);
                        }
                        s.one(body);
                        if let Some(st) = step {
                            s.expr(*st);
                        }
                    });
                });
                false
            }
            Stmt::ForOf {
                var_name,
                elem_expr,
                forin_obj,
                body,
                ..
            } => {
                if let Some(o) = forin_obj {
                    self.expr(*o);
                }
                self.scoped([var_name.as_str()], |s| {
                    s.region(2, &mut |s| {
                        s.set_hazard();
                        s.expr(*elem_expr);
                        s.one(body);
                    })
                });
                false
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.expr(*parent);
                self.expr(*sep);
                self.scoped([var_name.as_str()], |s| {
                    s.region(2, &mut |s| {
                        s.set_hazard();
                        s.one(body);
                    })
                });
                false
            }
            Stmt::Labeled { body, .. } => {
                self.region(1, &mut |s| {
                    s.one(body);
                });
                false
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee);
                // every case body shares the one scope of the switch
                let names = (cases.iter().flat_map(|c| &c.body))
                    .chain(default.iter().flatten())
                    .flat_map(declared);
                self.scoped(names, |s| {
                    s.region(1, &mut |s| {
                        for c in cases {
                            s.hazard |= s.any;
                            s.expr(c.value);
                            s.seq(&c.body.iter().collect::<Vec<_>>());
                        }
                        if let Some(d) = default {
                            s.hazard |= s.any;
                            s.seq(&d.iter().collect::<Vec<_>>());
                        }
                    })
                });
                false
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.region(1, &mut |s| {
                    s.block(body);
                    s.hazard |= s.any;
                    let param = catch_param.as_deref();
                    s.scoped(param, |s| s.block(catch_body));
                    s.hazard |= s.any;
                    if let Some(f) = finally_body {
                        s.block(f);
                    }
                });
                false
            }
            // a nested body runs some other time; its reads go
            // uncounted, which the caller sees as a mismatch
            _ => false,
        }
    }
}
