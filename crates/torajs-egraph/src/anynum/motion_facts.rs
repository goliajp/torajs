//! The number facts and predecessor lists the two ARC code motion
//! passes ([`super::inc_sink`], then [`super::dec_split`]) read, built
//! once per function for both.
//!
//! Neither pass changes what the facts or the lists describe: the
//! retain sinking only moves `rc_inc_any` (no result, no edge), and
//! the facts of one value read only the values copies join it to, so
//! facts computed for more values answer the same for each. The seeds
//! are the values the first pass to ask for them names (the retained values,
//! for the sinking) and every release in a block's leading run of
//! retains and releases, read from the function as it is then: a
//! superset of the releases that open a block once the retains have
//! moved.

use std::cell::OnceCell;

use torajs_core::ssa::{Function, InstKind, Module, Operand, ValueId};

use crate::anynum::number_facts::NumberFacts;
use crate::anynum::preds::Preds;

/// Per function of one module, by index; filled when a pass first
/// asks, so a function neither pass acts on costs one empty slot.
pub struct MotionFacts {
    funcs: Vec<OnceCell<Box<FuncFacts>>>,
}

#[derive(Default)]
struct FuncFacts {
    facts: OnceCell<Option<NumberFacts>>,
    preds: OnceCell<Preds>,
}

impl MotionFacts {
    pub fn new(module: &Module) -> Self {
        MotionFacts {
            funcs: (0..module.funcs.len()).map(|_| OnceCell::new()).collect(),
        }
    }

    fn slot(&self, f: usize) -> &FuncFacts {
        self.funcs[f].get_or_init(Box::default)
    }

    /// The facts of function `f`, `None` when no branch of it proves a
    /// value a number. `seeds` is read only by the first call for `f`.
    pub(super) fn facts(
        &self,
        f: usize,
        func: &Function,
        mut seeds: Vec<ValueId>,
    ) -> Option<&NumberFacts> {
        self.slot(f)
            .facts
            .get_or_init(|| {
                leading_releases(func, &mut seeds);
                NumberFacts::compute_if_any_edge_proof(func, &seeds)
            })
            .as_ref()
    }

    pub(super) fn preds(&self, f: usize, func: &Function) -> &Preds {
        self.slot(f).preds.get_or_init(|| Preds::of(func))
    }
}

/// The releases in the leading run of retains and releases of each
/// block other than the entry.
fn leading_releases(func: &Function, out: &mut Vec<ValueId>) {
    for block in func.blocks.iter().skip(1) {
        for inst in &block.insts {
            match inst.kind {
                InstKind::RcDecAny(Operand::Value(v)) => out.push(v),
                InstKind::RcIncAny(_) | InstKind::RcDecAny(_) => {}
                _ => break,
            }
        }
    }
}
