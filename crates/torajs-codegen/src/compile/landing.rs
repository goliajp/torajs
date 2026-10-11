//! Where a branch to a block really lands, and the assignment and
//! frame a block opens with.

use torajs_core::ssa::id_hash::HashMap;

use torajs_core::ssa::{BlockId, Terminator};

use super::entry::emit_param_entry_moves;
use crate::frame::FrameLayout;
use crate::regalloc::Assignment;

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

/// The register assignment and frame a block runs under, opening the
/// frame where shrink-wrapping put its save point: the blocks ahead of
/// it run frameless (see shrink_wrap.rs).
pub(super) fn open_block<'a>(
    bytes: &mut Vec<u8>,
    block: &torajs_core::ssa::Block,
    shrink: &'a Option<crate::shrink_wrap::ShrinkWrap>,
    alloc: &'a Assignment,
    frame: &'a FrameLayout,
    frameless: &'a FrameLayout,
) -> (&'a Assignment, &'a FrameLayout) {
    match shrink {
        Some(sw) if sw.save == block.id => {
            frame.emit_prologue(bytes);
            emit_param_entry_moves(bytes, alloc);
            (alloc, frame)
        }
        Some(sw) if sw.pre.contains(&block.id.0) => (&sw.pre_alloc, frameless),
        _ => (alloc, frame),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::Block;

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
