//! The catch-type inference's facts (`ast_throw_info::catch_types`),
//! and the lowering's own record of the raises it emits, which decides
//! whether an inferred type is used.
//!
//! Pass 0.5 reads what a `try` body can raise off the AST: the scan's
//! raise rules mirror the lowering's checks site by site, and a site
//! the scan does not know (a builtin constructor's kernel was one) let
//! a TypeError reach a catch that bound a number. So the type is taken
//! only when the lowering agrees: every throw check emitted while the
//! `try` body was lowered follows a call to a function, every such
//! function was lowered before (pass 2 lowers the declarations ahead of
//! `main`, pass 2B the closures after it) and emitted only checks of
//! that kind itself, transitively, and no function so reached has a
//! `throw` of a type other than the inferred one. A check with no
//! callee behind it (a kernel's, a guard's) is a raise of the lowering's
//! own, and leaves the parameter `any`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::Proofs;
use crate::ast::{Ast, ExprId, Stmt};
use crate::ast_throw_info::catch_types::{Thrown, cast_reads};

/// What pass 0.5 inferred, and the log the lowering checks it against.
#[derive(Default)]
pub(crate) struct CatchFacts {
    /// By the first `e as T` read of the catch body: the annotation the
    /// parameter takes and the checker's type of it.
    pub(crate) inferred: HashMap<ExprId, (String, crate::check::Type)>,
    /// Each FnDecl's own `throw`s, not closed over its calls.
    pub(crate) own_thrown: HashMap<String, Thrown>,
    log: RefCell<RaiseLog>,
}

#[derive(Default)]
struct RaiseLog {
    /// Per function, in emission order, its throw checks and the calls
    /// it makes with no check behind them.
    raises: HashMap<String, Vec<Raise>>,
    lowered: HashSet<String>,
}

enum Raise {
    /// A throw check after a call to the callee, `None` for one with no
    /// callee behind it.
    Check(Option<String>),
    /// A call the may-throw set says cannot raise, so no check follows
    /// it: a raise the scan does not know stays pending through it, and
    /// surfaces at the next check, so the callee's own checks count.
    Call(String),
    /// Such a call through a slot, whose body the log cannot name.
    SlotCall,
}

impl CatchFacts {
    /// `name`'s body is lowered: its log is complete.
    pub(crate) fn mark_lowered(&self, name: &str) {
        if !self.inferred.is_empty() {
            self.log.borrow_mut().lowered.insert(name.to_string());
        }
    }
}

impl Proofs {
    /// The inferred type of the parameter `p` of an unannotated catch
    /// whose body is `body`, before the lowering confirmed it.
    pub(crate) fn catch_type(
        &self,
        ast: &Ast,
        body: &[Stmt],
        p: &str,
    ) -> Option<&(String, crate::check::Type)> {
        let catch = &self.facts.catch;
        if catch.inferred.is_empty() {
            return None;
        }
        let (first, _) = cast_reads(ast, body, p)?;
        catch.inferred.get(&first)
    }

    /// Record a throw check `func` emits, after a call to `callee`.
    pub(crate) fn log_raise(&self, func: &str, callee: Option<String>) {
        self.log(func, Raise::Check(callee));
    }

    /// Record a call `func` makes to `callee` with no check after it.
    pub(crate) fn log_call(&self, func: &str, callee: String) {
        self.log(func, Raise::Call(callee));
    }

    /// Record a call `func` makes through a vtable or interface slot
    /// with no check after it.
    pub(crate) fn log_slot_call(&self, func: &str) {
        self.log(func, Raise::SlotCall);
    }

    fn log(&self, func: &str, raise: Raise) {
        let catch = &self.facts.catch;
        if catch.inferred.is_empty() {
            return;
        }
        let mut log = catch.log.borrow_mut();
        log.raises.entry(func.to_string()).or_default().push(raise);
    }

    /// Where the log of `func` stands now.
    pub(crate) fn raise_mark(&self, func: &str) -> usize {
        let log = self.facts.catch.log.borrow();
        log.raises.get(func).map_or(0, Vec::len)
    }

    /// Whether the checks and calls `func` emitted since `mark` reach
    /// only functions lowered already whose checks all follow calls,
    /// transitively, and whose `throw`s behind a check are of `want`.
    pub(crate) fn raises_confirm(
        &self,
        func: &str,
        mark: usize,
        want: &crate::check::Type,
    ) -> bool {
        let catch = &self.facts.catch;
        let log = catch.log.borrow();
        let own = log.raises.get(func).map_or(&[][..], |r| &r[mark..]);
        let mut stack: Vec<&Raise> = own.iter().collect();
        let mut seen: HashSet<(&str, bool)> = HashSet::new();
        while let Some(raise) = stack.pop() {
            let (callee, checked) = match raise {
                Raise::Check(None) | Raise::SlotCall => return false,
                Raise::Check(Some(c)) => (c, true),
                Raise::Call(c) => (c, false),
            };
            if !seen.insert((callee, checked)) {
                continue;
            }
            // a `throw` reaches the caller only through a check
            let only_want = !checked
                || match catch.own_thrown.get(callee.as_str()) {
                    Some(Thrown::Nothing) => true,
                    Some(Thrown::One(t)) => t == want,
                    _ => false,
                };
            if !only_want || !log.lowered.contains(callee) {
                return false;
            }
            stack.extend(log.raises.get(callee).into_iter().flatten());
        }
        true
    }
}
