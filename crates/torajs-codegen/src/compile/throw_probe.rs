//! The probe of the pending-throw flag that follows a call:
//! `%c = icmp ne (load i64 @throw_active), 0` and a branch on `%c`.
//! A sibling call (compile/sibcall.rs) looks through one, and a reader
//! (compile/throw_reg.rs) branches on x21 in its place after a call to
//! a signalling function.

use torajs_core::ssa::id_hash::{HashMap, HashSet};
use torajs_core::ssa::{
    Block, BlockId, Function, IPred, Inst, InstKind, Operand, THROW_ACTIVE_SYM, Terminator, Type,
    ValueId,
};

use super::throw_reg::ThrowRegPlan;

/// `%g = global_ref @throw_active; %l = load i64, %g; %c = icmp ne %l, 0`.
pub(super) fn is_throw_probe(g: &Inst, l: &Inst, test: &Inst, c: ValueId) -> bool {
    let (Some(gv), Some(lv)) = (g.result, l.result) else {
        return false;
    };
    matches!(&g.kind, InstKind::GlobalRef(name) if name == THROW_ACTIVE_SYM)
        && matches!(&l.kind, InstKind::Load(Type::I64, Operand::Value(p), 0) if *p == gv)
        && matches!(
            &test.kind,
            InstKind::ICmp(IPred::Ne, Operand::Value(x), Operand::ConstI64(0)) if *x == lv
        )
        && test.result == Some(c)
}

/// The blocks of a reader whose tail probe of the flag follows a call
/// to a signalling function and reads x21 instead:
/// `[%p = global_ref @throw_active]` (or a load through an address of
/// the flag defined elsewhere), `%l = load i64, %p`,
/// `%c = icmp ne %l, 0`, `cond_br %c`, with the load and the compare
/// read by nothing else. Maps each block to the index of its first
/// absorbed instruction.
pub(crate) fn probes(
    func: &Function,
    plan: &ThrowRegPlan,
    uses: &HashMap<ValueId, u32>,
) -> HashMap<u32, usize> {
    let mut out = HashMap::default();
    let once = |v: Option<ValueId>| v.is_some_and(|v| uses.get(&v) == Some(&1));
    let chain = Chain::of(func);
    let flags = flag_refs(func);
    for block in &func.blocks {
        let Terminator::CondBr {
            cond: Operand::Value(c),
            ..
        } = block.term
        else {
            continue;
        };
        let n = block.insts.len();
        if n < 2 {
            continue;
        }
        let (load, test) = (&block.insts[n - 2], &block.insts[n - 1]);
        let InstKind::Load(Type::I64, Operand::Value(p), 0) = load.kind else {
            continue;
        };
        let tests_load = matches!(
            test.kind,
            InstKind::ICmp(IPred::Ne, Operand::Value(x), Operand::ConstI64(0)) if Some(x) == load.result
        );
        if !(tests_load && test.result == Some(c) && once(test.result) && once(load.result)) {
            continue;
        }
        let gref = n.checked_sub(3).map(|k| &block.insts[k]);
        let own_ref = gref.is_some_and(|g| {
            g.result == Some(p)
                && matches!(&g.kind, InstKind::GlobalRef(s) if s == THROW_ACTIVE_SYM)
        });
        if !own_ref && !flags.contains(&p) {
            continue;
        }
        // a flag address with other readers stays
        let first = if own_ref && once(gref.and_then(|g| g.result)) {
            n - 3
        } else {
            n - 2
        };
        if chain.follows_signaller(plan, block, n - 2) {
            out.insert(block.id.0, first);
        }
    }
    out
}

/// Each block's predecessors, when every one reaches it by a jump.
struct Chain<'a> {
    by_id: HashMap<u32, &'a Block>,
    preds: HashMap<u32, Option<Vec<u32>>>,
}

impl<'a> Chain<'a> {
    fn of(func: &'a Function) -> Self {
        let mut preds: HashMap<u32, Option<Vec<u32>>> = HashMap::default();
        for b in &func.blocks {
            for t in targets(&b.term) {
                let entry = preds.entry(t.0).or_insert_with(|| Some(Vec::new()));
                match (&b.term, entry) {
                    (Terminator::Br(_), Some(ps)) => ps.push(b.id.0),
                    (_, e) => *e = None,
                }
            }
        }
        let by_id = func.blocks.iter().map(|b| (b.id.0, b)).collect();
        Chain { by_id, preds }
    }

    /// Whether, on every way into `block.insts[end]`, the last call is
    /// to a signalling function with nothing after it but copies and
    /// flag addresses (looking back through jumps, a few blocks deep).
    fn follows_signaller(&self, plan: &ThrowRegPlan, block: &'a Block, end: usize) -> bool {
        self.follows(plan, block, end, 4)
    }

    fn follows(&self, plan: &ThrowRegPlan, b: &'a Block, end: usize, depth: u32) -> bool {
        for i in b.insts[..end].iter().rev() {
            match &i.kind {
                InstKind::Call(g, _) => return plan.signals(*g),
                InstKind::Copy(..) => {}
                InstKind::GlobalRef(s) if s == THROW_ACTIVE_SYM => {}
                _ => return false,
            }
        }
        let Some(Some(ps)) = self.preds.get(&b.id.0) else {
            return false;
        };
        depth > 0
            && !ps.is_empty()
            && ps.iter().all(|p| {
                let pb = self.by_id[p];
                self.follows(plan, pb, pb.insts.len(), depth - 1)
            })
    }
}

pub(super) fn targets(term: &Terminator) -> Vec<BlockId> {
    match *term {
        Terminator::Br(t) => vec![t],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![then_blk, else_blk],
        Terminator::Ret(_) | Terminator::Unreachable => Vec::new(),
    }
}

/// The values that are the address of the pending-throw flag.
pub(super) fn flag_refs(func: &Function) -> HashSet<ValueId> {
    (func.blocks.iter())
        .flat_map(|b| b.insts.iter())
        .filter(|i| matches!(&i.kind, InstKind::GlobalRef(s) if s == THROW_ACTIVE_SYM))
        .filter_map(|i| i.result)
        .collect()
}
