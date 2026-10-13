//! Where a branch to a block really lands.
//!
//! Two kinds of block emit nothing but their own B: an empty forwarder
//! (no instructions, `Br`), and a hollow block — a `Br` whose every
//! instruction is a copy the allocator coalesced (both sides share one
//! home), such as one arm of a join that picks a value on each path.
//! A branch to either goes straight to the end of the
//! chain, and a branch whose chain ends at the next block in layout is
//! a fall-through.
//!
//! A block opens with the assignment and frame of its side of the
//! shrink-wrap save point.

use torajs_core::ssa::id_hash::HashMap;

use torajs_core::ssa::{Block, BlockId, Function, Terminator};

use super::entry::emit_param_entry_moves;
use super::operand::copy_emits_nothing;
use crate::frame::FrameLayout;
use crate::regalloc::Assignment;
use crate::shrink_wrap::ShrinkWrap;

pub(super) struct Landing<'a> {
    pub(super) blocks_by_id: HashMap<u32, &'a Block>,
    hollow: HashMap<u32, BlockId>,
    /// layout positions of the hollow blocks
    hollow_at: Vec<usize>,
}

impl<'a> Landing<'a> {
    /// The blocks of `func` under the assignment each one runs with: a
    /// block ahead of the shrink-wrap save point uses the frameless
    /// assignment, and the save point itself opens the frame, so it is
    /// never hollow.
    pub(super) fn of(func: &'a Function, alloc: &Assignment, shrink: &Option<ShrinkWrap>) -> Self {
        let mut hollow = HashMap::default();
        let mut hollow_at = Vec::new();
        for (at, block) in func.blocks.iter().enumerate() {
            let Terminator::Br(t) = block.term else {
                continue;
            };
            let Some((alloc, _)) = block_alloc(shrink, block.id, alloc) else {
                continue;
            };
            if t != block.id && block.insts.iter().all(|i| copy_emits_nothing(i, alloc)) {
                hollow.insert(block.id.0, t);
                hollow_at.push(at);
            }
        }
        Landing {
            blocks_by_id: func.blocks.iter().map(|b| (b.id.0, b)).collect(),
            hollow,
            hollow_at,
        }
    }

    pub(super) fn is_hollow(&self, block: BlockId) -> bool {
        self.hollow.contains_key(&block.0)
    }

    /// Which of `blocks`, the function's in layout order, are emitted.
    /// A hollow block that every branch to it lands past is not, the
    /// entry block excepted: the block before it then falls through
    /// to, or branches to, where it would have jumped, for no more than
    /// its own B cost. Kept, it would stand between that block and the
    /// code after it, so a loop exit laid out behind a hollow latch
    /// could not fall through.
    pub(super) fn emitted(&self, blocks: &[Block]) -> Vec<bool> {
        let mut emitted = vec![true; blocks.len()];
        for &at in &self.hollow_at {
            emitted[at] = at == 0 || self.is_hollow(self.resolve(blocks[at].id));
        }
        emitted
    }

    /// The block a branch to `start` lands on. The walk takes one step
    /// more than there are hollow blocks, so only a cycle of them (a
    /// degenerate infinite loop) stops it on a hollow block, which is
    /// then emitted: a chain of copies out of a long join is walked to
    /// its end.
    pub(super) fn resolve(&self, start: BlockId) -> BlockId {
        let mut id = start;
        for _ in 0..=self.hollow.len() {
            id = resolve_forwarded_target(&self.blocks_by_id, id);
            match self.hollow.get(&id.0) {
                Some(&t) => id = t,
                None => break,
            }
        }
        id
    }
}

/// Follow chains of empty forwarder blocks (`insts: [], term: Br(t)`)
/// to the branch's real destination, so a taken branch lands on work
/// instead of another unconditional B. Bounded walk — a cycle of
/// empty blocks (degenerate infinite loop) stops forwarding and keeps
/// the last id reached.
pub(super) fn resolve_forwarded_target(
    blocks_by_id: &HashMap<u32, &torajs_core::ssa::Block>,
    start: BlockId,
) -> BlockId {
    let mut id = start;
    for _ in 0..16 {
        match blocks_by_id.get(&id.0) {
            Some(b) if b.insts.is_empty() => match b.term {
                Terminator::Br(t) if t != id => id = t,
                _ => break,
            },
            _ => break,
        }
    }
    id
}

/// The assignment `block` runs with, and whether it runs frameless
/// ahead of the shrink-wrap save point; None at the save point, which
/// opens the frame and runs with `alloc`.
fn block_alloc<'a>(
    shrink: &'a Option<ShrinkWrap>,
    block: BlockId,
    alloc: &'a Assignment,
) -> Option<(&'a Assignment, bool)> {
    match shrink {
        Some(sw) if sw.save == block => None,
        Some(sw) if sw.pre.contains(&block.0) => Some((&sw.pre_alloc, true)),
        _ => Some((alloc, false)),
    }
}

/// The register assignment and frame a block runs under, opening the
/// frame where shrink-wrapping put its save point: the blocks ahead of
/// it run frameless (see shrink_wrap.rs).
pub(super) fn open_block<'a>(
    bytes: &mut Vec<u8>,
    block: &Block,
    shrink: &'a Option<ShrinkWrap>,
    alloc: &'a Assignment,
    frame: &'a FrameLayout,
    frameless: &'a FrameLayout,
) -> (&'a Assignment, &'a FrameLayout) {
    match block_alloc(shrink, block.id, alloc) {
        Some((alloc, pre)) => (alloc, if pre { frameless } else { frame }),
        None => {
            frame.emit_prologue(bytes);
            emit_param_entry_moves(bytes, alloc);
            (alloc, frame)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ModuleFacts, compile_function_with};
    use super::*;
    use crate::reg::{Gpr, Reg};
    use torajs_core::ssa::{IPred, Inst, InstKind, Operand, Type};

    /// `r = p != 0 ? 5 : p` as mem2reg leaves it, laid out `b0 k j h`:
    /// k and h each copy into `r`, and the allocator gave `p` and `r`
    /// one register, so h's copy is coalesced and k's constant is not.
    /// h sits after the join, so its own Br is a real B.
    fn join() -> (Function, Assignment) {
        let mut f = Function::new("j", Type::I64);
        let b0 = f.add_block();
        let k = f.add_block();
        let j = f.add_block();
        let h = f.add_block();
        let p = f.add_param(Type::I64, "p");
        let c = f.append_inst(
            b0,
            InstKind::ICmp(IPred::Ne, Operand::Value(p), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let copy = InstKind::Copy(Type::I64, Operand::Value(p));
        let r = f.append_inst(h, copy, Type::I64, None);
        let k_block = f.blocks.iter_mut().find(|b| b.id == k).unwrap();
        k_block.insts.push(Inst {
            result: Some(r),
            kind: InstKind::Copy(Type::I64, Operand::ConstI64(5)),
            origin: None,
        });
        f.set_term(
            b0,
            Terminator::CondBr {
                cond: Operand::Value(c),
                then_blk: k,
                else_blk: h,
            },
        );
        f.set_term(k, Terminator::Br(j));
        f.set_term(h, Terminator::Br(j));
        f.set_term(j, Terminator::Ret(Some(Operand::Value(r))));
        let homes = [(p, Gpr::X0), (c, Gpr::X1), (r, Gpr::X0)];
        let homes = homes.iter().map(|(v, g)| (v.0, Reg::Gpr(*g))).collect();
        let alloc = Assignment::from_parts(homes, HashMap::default(), 0, 0, false);
        (f, alloc)
    }

    #[test]
    fn a_coalesced_copy_block_is_hollow() {
        let (f, alloc) = join();
        let landing = Landing::of(&f, &alloc, &None);
        assert!(landing.is_hollow(BlockId(3)));
        assert!(!landing.is_hollow(BlockId(1)));
        assert_eq!(landing.resolve(BlockId(3)), BlockId(2));
        assert_eq!(landing.resolve(BlockId(1)), BlockId(1));
    }

    #[test]
    fn no_branch_lands_on_a_branch() {
        let (f, alloc) = join();
        let bytes = compile_function_with(&f, alloc, &ModuleFacts::default()).bytes;
        let words: Vec<u32> = (bytes.as_chunks::<4>().0.iter())
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        let is_b = |w: u32| w & 0xFC00_0000 == 0x1400_0000;
        for (i, &w) in words.iter().enumerate() {
            // B (imm26); B.cond and CBZ / CBNZ (imm19)
            let disp = if is_b(w) {
                ((w << 6) as i32) >> 6
            } else if w & 0xFF00_0010 == 0x5400_0000 || w & 0x7E00_0000 == 0x3400_0000 {
                ((w << 8) as i32) >> 13
            } else {
                continue;
            };
            let target = words[(i as i32 + disp) as usize];
            assert!(!is_b(target), "{i}: lands on a B: {words:08x?}");
        }
    }

    #[test]
    fn forwarder_chain_resolves_and_cycle_stops() {
        let empty_br = |id: u32, t: u32| Block {
            id: BlockId(id),
            insts: vec![],
            term: Terminator::Br(BlockId(t)),
        };
        let blocks = [empty_br(0, 1), empty_br(1, 2), empty_br(2, 2)];
        let by_id: HashMap<u32, &Block> = blocks.iter().map(|b| (b.id.0, b)).collect();
        // 0 → 1 → 2; 2 self-loops so the walk parks there
        assert_eq!(resolve_forwarded_target(&by_id, BlockId(0)), BlockId(2));
    }
}
