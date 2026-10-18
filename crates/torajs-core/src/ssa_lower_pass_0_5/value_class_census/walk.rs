//! The whitelist walk: every expression whose checker type
//! holds an instance sits where its shape is known.
//!
//! The walk goes down from the statements, so each expression is met
//! with the place its parent puts it in ([`Ctx`]). Afterwards every
//! typed expression of the program must have been met (an expression
//! reached some other way is judged as a failure), and every read of
//! a view binding must have been counted by the scan of its scope
//! ([`super::view`]).

use std::collections::HashMap;

use crate::ast::ExprId;

use super::Candidate;
use super::program::{Cat, Holder, Program};

mod fns;
mod rules;
mod scope;
mod stmt;

use fns::Watched;
use scope::Scope;

/// The place an expression sits in.
#[derive(Clone, Copy)]
enum Ctx<'a> {
    /// An expression statement, or a `for` update.
    Discard,
    Let {
        mutable: bool,
        ann: Option<&'a str>,
    },
    Return,
    MemberObj {
        name: &'a str,
        role: Role,
    },
    IndexObj {
        index: ExprId,
        role: Role,
    },
    /// The receiver of a `push` whose argument passed.
    PushRecv,
    PushArg,
    /// An argument; `want` is the parameter's reading when the callee
    /// is a declared function that names the class.
    Arg {
        want: Option<Cat>,
    },
    Callee,
    AssignValue {
        assign: ExprId,
    },
    AssignTarget,
    /// The operand of `delete`, `++` / `--` or an optional chain.
    Mutate,
    Other,
}

/// What the parent does with the member or element it reads.
#[derive(Clone, Copy, PartialEq)]
enum Role {
    Read,
    Write,
    Call,
    Other,
}

fn role_of(ctx: &Ctx<'_>) -> Role {
    match ctx {
        Ctx::AssignTarget => Role::Write,
        Ctx::Callee => Role::Call,
        Ctx::Mutate => Role::Other,
        _ => Role::Read,
    }
}

struct Walk<'p, 'a> {
    prog: &'p Program<'a>,
    h: &'p Holder<'a>,
    watched: HashMap<&'a str, Watched>,
    visited: Vec<bool>,
    /// The function being walked, with its return reading when watched.
    cur_fn: Option<&'a str>,
    cur_ret: Option<Cat>,
    /// No checks: the class's own factory and constructor, a
    /// container factory's seed literal.
    exempt: bool,
    err: Option<String>,
    /// Reads of view bindings: met by the walk, counted by the scans.
    ident_reads: HashMap<&'a str, usize>,
    view_reads: HashMap<&'a str, usize>,
    scopes: Vec<Scope<'a>>,
}

pub(super) fn judge(prog: &Program<'_>, c: &Candidate<'_>) -> Result<(), String> {
    if let Some(r) = prog.road {
        return Err(format!("the program spells `{r}`"));
    }
    let h = Holder::new(prog, c)?;
    let watched = fns::watched(prog, &h)?;
    let mut w = Walk {
        prog,
        h: &h,
        watched,
        visited: vec![false; prog.ast.exprs.len()],
        cur_fn: None,
        cur_ret: None,
        exempt: false,
        err: None,
        ident_reads: HashMap::new(),
        view_reads: HashMap::new(),
        scopes: Vec::new(),
    };
    w.enter_scope(&[], &prog.ast.stmts);
    w.stmts(&prog.ast.stmts);
    if let Some(e) = w.err {
        return Err(e);
    }
    for (id, ty) in prog.types {
        if !w.visited.get(id.0 as usize).is_some_and(|v| *v)
            && !prog.dead.contains(*id)
            && h.classify(ty).is_some()
        {
            return Err(format!(
                "an expression typed {ty:?} outside every statement"
            ));
        }
    }
    for (n, k) in &w.ident_reads {
        if w.view_reads.get(n) != Some(k) {
            return Err(format!("a read of `{n}` outside the scope of its view"));
        }
    }
    Ok(())
}

impl<'p, 'a> Walk<'p, 'a> {
    fn fail(&mut self, why: String) {
        self.err.get_or_insert(why);
    }

    fn cat(&self, e: ExprId) -> Option<Cat> {
        self.h.cat_of(self.prog, e)
    }

    fn expr(&mut self, e: ExprId, ctx: Ctx<'a>) {
        if self.err.is_some() || self.prog.dead.contains(e) {
            return;
        }
        self.visited[e.0 as usize] = true;
        let x = self.prog.ast.get_expr(e);
        if !self.exempt
            && !matches!(ctx, Ctx::Callee)
            && let Some(cat) = self.cat(e)
            && let Err(why) = self.allowed(e, x, cat, &ctx)
        {
            self.fail(why);
            return;
        }
        self.children(e, x, &ctx);
    }
}
