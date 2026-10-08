//! Where the conversion test of a call from outside the family runs.
//!
//! Outside any loop it runs at the call, once per call. Inside a loop
//! it runs in front of the outermost loop that does not change the
//! arguments it converts, and the call only branches on its outcome;
//! a call whose arguments change on every trip keeps calling the
//! original, since it would pay the whole conversion on each one.
//!
//! The hoisted test is branch-free, so it can sit where the loop is
//! entered whether or not the loop then makes the call. An argument
//! passes when truncating it and converting back gives the same bits
//! (NaN, -0, a fraction, an infinity and anything past 2^63 do not)
//! and the integer lies inside the window.

use crate::id_hash::HashMap;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};
use torajs_core::ssa::{BinOp, BlockId, Function, IPred, InstKind, Operand, Type, ValueId};

use super::Window;
use crate::cfg_edit::emit;

/// Where a call's test goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
    /// at the call: it is in no loop
    AtCall,
    /// at the end of this block, the idom of the outermost loop header
    /// whose loop leaves the arguments alone
    Before(BlockId),
    /// nowhere: the innermost loop around the call changes an argument
    Varies,
}

pub(super) struct Loops {
    dom: DominatorTree,
    /// outermost first
    loops: Vec<NaturalLoop>,
}

impl Loops {
    pub(super) fn compute(f: &Function) -> Self {
        let dom = DominatorTree::compute(f);
        let mut loops = LoopAnalysis::compute(f, &dom).loops().to_vec();
        loops.sort_by_key(|l| (std::cmp::Reverse(l.body.len()), l.header.0));
        Loops { dom, loops }
    }

    pub(super) fn placement(&self, f: &Function, at: BlockId, sources: &[ValueId]) -> Placement {
        let mut around = self.loops.iter().filter(|l| l.contains(&at)).peekable();
        if around.peek().is_none() {
            return Placement::AtCall;
        }
        // where each source is written: a parameter nowhere, a value
        // once; one written more than once is a cell, never fixed
        let mut defs: HashMap<ValueId, (BlockId, u32)> = HashMap::default();
        for block in &f.blocks {
            for r in block.insts.iter().filter_map(|i| i.result) {
                if sources.contains(&r) {
                    defs.entry(r).or_insert((block.id, 0)).1 += 1;
                }
            }
        }
        for l in around {
            let fixed = sources.iter().all(|s| match defs.get(s) {
                None => f.params.contains(s),
                Some(&(d, n)) => n == 1 && !l.contains(&d),
            });
            if !fixed {
                continue;
            }
            if let Some(pre) = self.dom.immediate_dominator(l.header)
                && !l.contains(&pre)
            {
                return Placement::Before(pre);
            }
        }
        Placement::Varies
    }
}

/// The test for `needs` (source, window) appended to `pre`: a bool
/// that holds when every argument passes, and each source's integer.
pub(super) fn emit_test(
    f: &mut Function,
    pre: BlockId,
    needs: &[(ValueId, Window)],
) -> (Operand, HashMap<ValueId, ValueId>) {
    let mut ints = HashMap::default();
    let mut bad: Option<Operand> = None;
    for &(source, w) in needs {
        let v = Operand::Value(source);
        let t = emit(f, pre, Type::I64, InstKind::FpToSi(v));
        let back = emit(f, pre, Type::F64, InstKind::SiToFp(t));
        let bits = emit(f, pre, Type::I64, InstKind::BitCastF64ToI64(v));
        let back_bits = emit(f, pre, Type::I64, InstKind::BitCastF64ToI64(back));
        let differs = emit(
            f,
            pre,
            Type::I64,
            InstKind::BinOp(BinOp::Xor, bits, back_bits),
        );
        let outside = outside_window(f, pre, t, w);
        let this = emit(
            f,
            pre,
            Type::I64,
            InstKind::BinOp(BinOp::Or, differs, outside),
        );
        bad = Some(match bad {
            Some(b) => emit(f, pre, Type::I64, InstKind::BinOp(BinOp::Or, b, this)),
            None => this,
        });
        let Operand::Value(t) = t else {
            unreachable!("emit answers a value");
        };
        ints.insert(source, t);
    }
    let bad = bad.expect("a hoisted test has a need");
    let passes = emit(
        f,
        pre,
        Type::Bool,
        InstKind::ICmp(IPred::Eq, bad, Operand::ConstI64(0)),
    );
    (passes, ints)
}

/// Nonzero when `t` is outside `w`: a shift for a window `[0, 2^k)`,
/// one unsigned comparison of the offset otherwise.
fn outside_window(f: &mut Function, pre: BlockId, t: Operand, w: Window) -> Operand {
    let span = w.hi - w.lo;
    if w.lo == 0 && (span + 1).count_ones() == 1 {
        let k = (span + 1).trailing_zeros() as i64;
        return emit(
            f,
            pre,
            Type::I64,
            InstKind::BinOp(BinOp::LShr, t, Operand::ConstI64(k)),
        );
    }
    let off = emit(
        f,
        pre,
        Type::I64,
        InstKind::BinOp(BinOp::Sub, t, Operand::ConstI64(w.lo)),
    );
    let above = emit(
        f,
        pre,
        Type::Bool,
        InstKind::ICmp(IPred::Uge, off, Operand::ConstI64(span + 1)),
    );
    emit(f, pre, Type::I64, InstKind::ZExtBoolToI64(above))
}
