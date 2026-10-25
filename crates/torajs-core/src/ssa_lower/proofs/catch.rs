//! The catch-type inference's facts (`ast_throw_info::catch_types`),
//! and the lowering's own reading of what a `try` body can raise, which
//! decides whether an inferred type is used.
//!
//! Pass 0.5 reads what a `try` body can raise off the AST: the scan's
//! raise rules mirror the lowering's checks site by site, and a site
//! the scan does not know (a builtin constructor's kernel was one) let
//! a TypeError reach a catch that bound a number. So the type is taken
//! only when the code the lowering emitted agrees. Every raise is a
//! call: a `throw` hands its value to `__torajs_throw_set`, and every
//! other raise is a runtime kernel or a user function recording one.
//! The body is read call by call, transitively through the user
//! functions it calls, and the type stands only when
//!
//! - every runtime call is to a kernel that never records a raise
//!   ([`CLEAN`]), or to a class's release function;
//! - every user function reached was lowered before (pass 2 lowers the
//!   declarations ahead of `main` and records them; a lifted closure's
//!   body is never recorded, so a call to one leaves the type unused)
//!   and has no `throw` of a type other than the inferred one;
//! - nothing is called through a value or a slot, and nothing suspends.
//!
//! Reading the calls rather than the throw checks keeps a raise the
//! lowering records with no check behind it (the pending throw then
//! surfaces at a later check) from going unseen.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::Proofs;
use crate::ast::{Ast, ExprId, Stmt};
use crate::ast_throw_info::catch_types::{Thrown, cast_reads};
use crate::ssa::{BlockId, FuncId, Function, Inst, InstKind};

/// The runtime kernels that never record a raise, each read for it:
/// the throw itself (its value is a `throw` operand the inference
/// typed), the pending slot's readers, allocation, a Map's empty
/// construction and size, and release (which runs no user code and
/// raises nothing: `calls` skips `RcDecAny` on that premise, so a drop
/// path that could raise must change both). A kernel left out only
/// costs the typed binding.
const CLEAN: &[&str] = &[
    "__torajs_throw_set",
    "__torajs_throw_take",
    "__torajs_throw_take_tag",
    "__torajs_obj_alloc",
    "__torajs_str_drop",
    "__torajs_map_create",
    "__torajs_map_mark_num",
    "__torajs_map_size",
    "__torajs_map_drop",
];

/// What pass 0.5 inferred, and the lowered bodies it is checked against.
#[derive(Default)]
pub(crate) struct CatchFacts {
    /// By the first `e as T` read of the catch body: the annotation the
    /// parameter takes and the checker's type of it.
    pub(crate) inferred: HashMap<ExprId, (String, crate::check::Type)>,
    /// Each FnDecl's own `throw`s, not closed over its calls.
    pub(crate) own_thrown: HashMap<String, Thrown>,
    bodies: RefCell<HashMap<String, Calls>>,
    callees: std::cell::OnceCell<Callees>,
}

/// The callees a body may call and still be read clean, by id.
struct Callees {
    /// The user functions, by their names.
    user: HashMap<FuncId, String>,
    /// The [`CLEAN`] kernels and the release functions.
    quiet: HashSet<FuncId>,
}

/// The calls of a lowered body or of a stretch of one.
struct Calls {
    /// Every call is to a user function, a [`CLEAN`] kernel or a
    /// release function.
    clean: bool,
    /// The user functions called.
    user: Vec<String>,
}

/// Where a `try` body's code starts: the block it starts in, how many
/// instructions that block had, and how many blocks the function had.
#[derive(Clone, Copy)]
pub(crate) struct BodyStart {
    block: BlockId,
    insts: usize,
    blocks: usize,
}

/// What a call site needs to tell the callee apart.
pub(crate) struct CallNames<'a> {
    pub(crate) fn_table: &'a HashMap<String, FuncId>,
    pub(crate) drops: &'a HashMap<u32, FuncId>,
}

impl<'a> CallNames<'a> {
    pub(crate) fn new(
        fn_table: &'a HashMap<String, FuncId>,
        drops: &'a HashMap<u32, FuncId>,
    ) -> Self {
        CallNames { fn_table, drops }
    }
}

impl CatchFacts {
    /// `name` is lowered to `f`: record its calls.
    pub(crate) fn record_body(&self, name: &str, f: &Function, names: &CallNames<'_>) {
        if self.inferred.is_empty() {
            return;
        }
        let insts = f.blocks.iter().flat_map(|b| b.insts.iter());
        let calls = self.calls(insts, names);
        self.bodies.borrow_mut().insert(name.to_string(), calls);
    }

    fn calls<'i>(&self, insts: impl Iterator<Item = &'i Inst>, names: &CallNames<'_>) -> Calls {
        let ids = self.callees.get_or_init(|| Callees {
            user: (self.own_thrown.keys())
                .filter_map(|n| Some((*names.fn_table.get(n)?, n.clone())))
                .collect(),
            quiet: (CLEAN.iter())
                .filter_map(|n| names.fn_table.get(*n).copied())
                .chain(names.drops.values().copied())
                .collect(),
        });
        let mut out = Calls {
            clean: true,
            user: Vec::new(),
        };
        for inst in insts {
            match &inst.kind {
                InstKind::Call(fid, _) => match ids.user.get(fid) {
                    Some(n) => out.user.push(n.clone()),
                    None => out.clean &= ids.quiet.contains(fid),
                },
                InstKind::CallIndirect(..) | InstKind::Suspend(..) => out.clean = false,
                _ => {}
            }
        }
        out
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

    /// Where a `try` body lowered into `f` from `block` on starts.
    pub(crate) fn body_start(&self, f: &Function, block: BlockId) -> BodyStart {
        BodyStart {
            block,
            insts: f.blocks[block.0 as usize].insts.len(),
            blocks: f.blocks.len(),
        }
    }

    /// Whether the `try` body lowered into `f` since `start` (and up to
    /// `end` blocks) raises only `throw`s of `want`, by its calls.
    pub(crate) fn body_raises_only(
        &self,
        (f, start, end): (&Function, BodyStart, usize),
        names: &CallNames<'_>,
        want: &crate::check::Type,
    ) -> bool {
        let catch = &self.facts.catch;
        let head = f.blocks[start.block.0 as usize]
            .insts
            .iter()
            .skip(start.insts);
        let rest = f.blocks[start.blocks..end]
            .iter()
            .flat_map(|b| b.insts.iter());
        let body = catch.calls(head.chain(rest), names);
        if !body.clean {
            return false;
        }
        let bodies = catch.bodies.borrow();
        let mut stack: Vec<&str> = body.user.iter().map(String::as_str).collect();
        let mut seen: HashSet<&str> = HashSet::new();
        while let Some(callee) = stack.pop() {
            if !seen.insert(callee) {
                continue;
            }
            let only_want = match catch.own_thrown.get(callee) {
                Some(Thrown::Nothing) => true,
                Some(Thrown::One(t)) => t == want,
                _ => false,
            };
            let Some(calls) = bodies.get(callee).filter(|c| only_want && c.clean) else {
                return false;
            };
            stack.extend(calls.user.iter().map(String::as_str));
        }
        true
    }
}
