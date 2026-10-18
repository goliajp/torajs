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
            Stmt::Block(v) | Stmt::Multi(v) => self.seq(&v.iter().collect::<Vec<_>>()),
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
                if let Some(i) = init {
                    self.one(i);
                }
                self.region(2, &mut |s| {
                    if let Some(c) = cond {
                        s.expr(*c);
                    }
                    s.one(body);
                    if let Some(st) = step {
                        s.expr(*st);
                    }
                });
                false
            }
            Stmt::ForOf {
                elem_expr,
                forin_obj,
                body,
                ..
            } => {
                if let Some(o) = forin_obj {
                    self.expr(*o);
                }
                self.region(2, &mut |s| {
                    s.set_hazard();
                    s.expr(*elem_expr);
                    s.one(body);
                });
                false
            }
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                self.expr(*parent);
                self.expr(*sep);
                self.region(2, &mut |s| {
                    s.set_hazard();
                    s.one(body);
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
                self.region(1, &mut |s| {
                    for c in cases {
                        s.hazard |= s.any;
                        s.expr(c.value);
                        s.seq(&c.body.iter().collect::<Vec<_>>());
                    }
                    if let Some(d) = default {
                        s.hazard |= s.any;
                        s.seq(&d.iter().collect::<Vec<_>>());
                    }
                });
                false
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                self.region(1, &mut |s| {
                    s.seq(&body.iter().collect::<Vec<_>>());
                    s.hazard |= s.any;
                    s.seq(&catch_body.iter().collect::<Vec<_>>());
                    s.hazard |= s.any;
                    if let Some(f) = finally_body {
                        s.seq(&f.iter().collect::<Vec<_>>());
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
