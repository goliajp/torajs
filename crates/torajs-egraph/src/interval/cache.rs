//! Which bodies the pipeline's facts still describe as analyzed.
//!
//! The analysis is a pure function of a function body and the module's
//! function names. A later reader of a body no pass has changed since
//! takes the facts as they are; a changed body is analyzed again, for
//! the passes in between can make it more precise (a guard float_demote
//! inserts bounds the values it dominates, a loop ctpop collapsed has
//! no recurrence left to widen). Each body is fingerprinted when it is
//! analyzed rather than cloned: a reader hashes the body it holds and
//! compares. A collision hands a changed body the facts of the old one;
//! they hold only because every pass keeps facts id-valid (see
//! `interval::run`: a value redefined in place takes a subset of its old
//! values, a value whose meaning changes loses its fact).

use std::borrow::Cow;
use std::hash::{Hash, Hasher};

use crate::id_hash::{HashMap, IdHasher};

use torajs_core::ssa::{Function, Module, Terminator, ValueId};

use super::NumFact;

/// The facts a point evaluator can start from instead of analyzing.
pub type Seed<'a> = Option<&'a HashMap<ValueId, NumFact>>;

pub struct FactCache {
    names: Vec<String>,
    /// index-aligned with `Module::funcs`: the fingerprint of the body
    /// analyzed, `None` for a declaration (no reader asks for one)
    bodies: Vec<Option<u64>>,
}

impl FactCache {
    /// The cache for `module` as analyzed under `names`, its function
    /// names.
    pub fn new(module: &Module, names: Vec<String>) -> Self {
        let bodies = module
            .funcs
            .iter()
            .map(|f| (!f.is_declaration()).then(|| fingerprint(f)))
            .collect();
        FactCache { names, bodies }
    }

    /// The function names the analysis ran under.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// `module`'s function names, and `cache` when it was built under
    /// those names (a pass that adds, drops or renames a function
    /// invalidates every entry). A matching cache lends its own copy
    /// of the names.
    pub fn for_module<'a>(
        cache: Option<&'a FactCache>,
        module: &Module,
    ) -> (Option<&'a FactCache>, Cow<'a, [String]>) {
        match cache {
            Some(c) if c.names.iter().eq(module.funcs.iter().map(|f| &f.name)) => {
                (Some(c), Cow::Borrowed(c.names.as_slice()))
            }
            _ => (
                None,
                Cow::Owned(module.funcs.iter().map(|f| f.name.clone()).collect()),
            ),
        }
    }

    /// `facts`' entry for `module.funcs[idx]` when that body is the one
    /// analyzed.
    pub fn get<'f>(
        &self,
        facts: Option<&'f [HashMap<ValueId, NumFact>]>,
        idx: usize,
        func: &Function,
    ) -> Seed<'f> {
        let analyzed = (*self.bodies.get(idx)?)?;
        if analyzed != fingerprint(func) {
            return None;
        }
        facts?.get(idx)
    }
}

/// A hash of everything the analysis reads from a body: its parameters,
/// value types, instructions and terminators. Value names and source
/// origins do not enter it.
fn fingerprint(func: &Function) -> u64 {
    let mut h = IdHasher::default();
    func.params.hash(&mut h);
    h.write_usize(func.values.len());
    for v in &func.values {
        v.ty.hash(&mut h);
    }
    h.write_usize(func.blocks.len());
    for block in &func.blocks {
        block.id.hash(&mut h);
        h.write_usize(block.insts.len());
        for inst in &block.insts {
            inst.result.hash(&mut h);
            inst.kind.hash(&mut h);
        }
        match &block.term {
            Terminator::Br(b) => {
                h.write_u8(0);
                b.hash(&mut h);
            }
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => {
                h.write_u8(1);
                cond.hash(&mut h);
                then_blk.hash(&mut h);
                else_blk.hash(&mut h);
            }
            Terminator::Ret(op) => {
                h.write_u8(2);
                op.hash(&mut h);
            }
            Terminator::Unreachable => h.write_u8(3),
        }
    }
    h.finish()
}
