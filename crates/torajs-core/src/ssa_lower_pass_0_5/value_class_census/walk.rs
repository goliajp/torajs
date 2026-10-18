//! The whitelist walk: every expression whose checker type
//! holds an instance sits where its shape is known.
//!
//! The walk goes down from the statements, so each expression is met
//! with the place its parent puts it in ([`Ctx`]). Afterwards every
//! typed expression of the program must have been met (an expression
//! reached some other way is judged as a failure), and every read of
//! a view binding must have been counted by the scan of its scope
//! ([`super::view`]). A read is matched to its view by where the view
//! is declared, not by its name, so a block that rebinds the name
//! cannot trade its reads for a read the scan never saw.

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
#[derive(Clone, Copy, Debug)]
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
#[derive(Clone, Copy, PartialEq, Debug)]
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
    /// Reads of view bindings, keyed by the view's init: met by the
    /// walk, counted by the scans.
    ident_reads: HashMap<ExprId, usize>,
    view_reads: HashMap<ExprId, usize>,
    scopes: Vec<Scope<'a>>,
    /// The names each enclosing block, loop head, catch clause or
    /// parameter list binds, innermost last: a view's init, or `None`
    /// for any other binding.
    blocks: Vec<HashMap<&'a str, Option<ExprId>>>,
    /// The expression whose child is being met, for the reasons.
    parent: Option<ExprId>,
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
        blocks: Vec::new(),
        parent: None,
    };
    w.enter_scope(&[], &prog.ast.stmts);
    w.stmts(&prog.ast.stmts);
    if let Some(e) = w.err {
        return Err(e);
    }
    // the first failure in expression order, so the reason is the
    // same on every run
    let missed = (prog.types.iter())
        .filter(|(id, ty)| {
            !w.visited.get(id.0 as usize).is_some_and(|v| *v)
                && !prog.dead.contains(**id)
                && h.classify(ty).is_some()
        })
        .min_by_key(|(id, _)| id.0);
    if let Some((_, ty)) = missed {
        return Err(format!(
            "an expression typed {ty:?} outside every statement"
        ));
    }
    let stray = (w.ident_reads.iter())
        .filter(|(v, k)| w.view_reads.get(*v) != Some(*k))
        .map(|(v, _)| v.0)
        .min();
    if let Some(v) = stray {
        return Err(format!(
            "a read of the view bound to expression {v} outside the scope of its view"
        ));
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

    /// The view a read of `name` resolves to (`None`: the innermost
    /// binding of the name is no view, or there is none).
    fn view_of(&self, name: &str) -> Option<ExprId> {
        self.blocks
            .iter()
            .rev()
            .find_map(|b| b.get(name))
            .copied()
            .flatten()
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
        let up = self.parent.replace(e);
        self.children(e, x, &ctx);
        self.parent = up;
    }
}
