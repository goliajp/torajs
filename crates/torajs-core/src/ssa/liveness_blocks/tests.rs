use std::collections::BTreeSet;

use super::{compute_block_liveness, visit_terminator_values};
use crate::ssa::{
    BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
    visit_value_operands,
};

type Sets = Vec<BTreeSet<u32>>;

/// The round-robin fix-point of the data-flow equations, kept as the
/// oracle the path exploration must match set for set.
fn dense_fix_point(func: &Function) -> (Sets, Sets) {
    let n = func.blocks.len();
    let pos = |id: BlockId| func.blocks.iter().position(|b| b.id == id);
    let mut uses: Sets = vec![BTreeSet::new(); n];
    let mut defs: Sets = vec![BTreeSet::new(); n];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                if !defs[i].contains(&v.0) {
                    uses[i].insert(v.0);
                }
            });
            if let Some(r) = inst.result {
                defs[i].insert(r.0);
            }
        }
        visit_terminator_values(&block.term, |v| {
            if !defs[i].contains(&v.0) {
                uses[i].insert(v.0);
            }
        });
        match &block.term {
            Terminator::Br(t) => succs[i].extend(pos(*t)),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                succs[i].extend(pos(*then_blk));
                succs[i].extend(pos(*else_blk));
            }
            _ => {}
        }
    }
    let mut live_in: Sets = vec![BTreeSet::new(); n];
    let mut live_out: Sets = vec![BTreeSet::new(); n];
    loop {
        let mut changed = false;
        for b in (0..n).rev() {
            let out: BTreeSet<u32> = succs[b].iter().flat_map(|&s| live_in[s].clone()).collect();
            let inn: BTreeSet<u32> = uses[b].union(&(&out - &defs[b])).copied().collect();
            changed |= out != live_out[b] || inn != live_in[b];
            live_out[b] = out;
            live_in[b] = inn;
        }
        if !changed {
            return (live_in, live_out);
        }
    }
}

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

/// Random CFG with loops, unreachable blocks, dangling targets,
/// non-contiguous block ids and values redefined in several blocks
/// (φ cells), so every kill and seed rule gets exercised.
fn random_function(rng: &mut Lcg) -> Function {
    let blocks = 1 + rng.below(24) as u32;
    let values = 1 + rng.below(40) as u32;
    let id = |b: u32| BlockId(b * 3 + 1);
    let val = |rng: &mut Lcg| ValueId(rng.below(values as u64) as u32);
    let mut out = Vec::new();
    for b in 0..blocks {
        let mut insts = Vec::new();
        for _ in 0..rng.below(5) {
            let (a, c) = (Operand::Value(val(rng)), Operand::Value(val(rng)));
            let kind = match rng.below(3) {
                0 => InstKind::Copy(Type::I64, a),
                1 => InstKind::BinOp(BinOp::Add, a, c),
                _ => InstKind::Store(a, c, 0),
            };
            let result = (!matches!(kind, InstKind::Store(..))).then(|| val(rng));
            insts.push(Inst {
                result,
                kind,
                origin: None,
            });
        }
        // targets past the last block are dangling and must be ignored
        let target = |rng: &mut Lcg| id(rng.below(blocks as u64 + 2) as u32);
        let term = match rng.below(4) {
            0 => Terminator::Ret(Some(Operand::Value(val(rng)))),
            1 => Terminator::Br(target(rng)),
            _ => Terminator::CondBr {
                cond: Operand::Value(val(rng)),
                then_blk: target(rng),
                else_blk: target(rng),
            },
        };
        out.push(Block {
            id: id(b),
            insts,
            term,
        });
    }
    Function {
        name: "random".into(),
        params: Vec::new(),
        ret: Type::Void,
        values: (0..values)
            .map(|_| ValueInfo {
                ty: Type::I64,
                name: None,
            })
            .collect(),
        blocks: out,
        current_origin: None,
    }
}

#[test]
fn path_exploration_matches_the_dense_fix_point() {
    let mut rng = Lcg(0x5eed);
    // a generator whose sets are all empty would match vacuously
    let (mut seen_in, mut seen_out) = (0, 0);
    for _ in 0..2000 {
        let func = random_function(&mut rng);
        let got = compute_block_liveness(&func);
        let (live_in, live_out) = dense_fix_point(&func);
        for b in 0..func.blocks.len() {
            let gi: Vec<u32> = got.live_in[b].iter().collect();
            let go: Vec<u32> = got.live_out[b].iter().collect();
            assert_eq!(gi, live_in[b].iter().copied().collect::<Vec<_>>());
            assert_eq!(go, live_out[b].iter().copied().collect::<Vec<_>>());
            seen_in += gi.len();
            seen_out += go.len();
        }
    }
    assert!(
        seen_in > 10_000 && seen_out > 10_000,
        "{seen_in} {seen_out}"
    );
}
