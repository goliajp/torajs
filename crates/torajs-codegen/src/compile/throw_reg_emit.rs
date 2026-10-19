//! What the block loop of `compile` emits for the throw register
//! (compile/throw_reg.rs): x21 zeroed where compile/throw_reg_zero.rs
//! puts it, and the identical throw returns of a signalling function
//! shared.

use torajs_core::ssa::{Block, BlockId, Function, InstKind, Operand, Terminator};

use super::ModuleFacts;
use super::landing::Landing;
use super::terminator::{BranchFixup, BranchKind, Fixups};
use super::throw_reg::{Role, THROW_REG};
use super::throw_reg_zero::Zeroing;
use super::write_u32;
use crate::enc::{b_imm26, movz_imm};
use crate::frame::FrameLayout;

fn zero(bytes: &mut Vec<u8>) {
    write_u32(bytes, movz_imm(THROW_REG, 0, 0));
}

/// What the block loop of `compile` does for the throw register: open
/// a block (a shared throw return, or x21 zeroed on entry) and zero x21
/// after a take.
pub(super) struct BlockHooks<'a> {
    role: Role,
    frame: &'a FrameLayout,
    zeroing: Zeroing,
    rets: SharedThrowRets,
}

impl<'a> BlockHooks<'a> {
    pub(super) fn of(
        func: &Function,
        facts: &ModuleFacts,
        role: Role,
        frame: &'a FrameLayout,
        landing: &Landing,
    ) -> Self {
        let zeroing = if role.reads {
            super::throw_reg_zero::plan(func, &facts.throw_reg, landing, role.guards)
        } else {
            Zeroing::default()
        };
        BlockHooks {
            role,
            frame,
            zeroing,
            rets: SharedThrowRets::default(),
        }
    }

    /// True when `block` was emitted as a branch to an identical throw
    /// return; `frame` is the one it runs in, `start` its offset.
    pub(super) fn open(
        &mut self,
        bytes: &mut Vec<u8>,
        fixups: &mut Fixups,
        block: &Block,
        frame: &FrameLayout,
        start: usize,
    ) -> bool {
        let framed = std::ptr::eq(frame, self.frame);
        if self.role.signals
            && bytes.len() == start
            && self.rets.branch(bytes, fixups, block, framed)
        {
            return true;
        }
        if self.zeroing.entries.contains(&block.id.0) {
            zero(bytes);
        }
        false
    }

    pub(super) fn after_inst(&self, bytes: &mut Vec<u8>, block: &Block, ii: usize) {
        if !self.zeroing.takes.is_empty() && self.zeroing.takes.contains(&(block.id.0, ii)) {
            zero(bytes);
        }
    }
}

/// The throw returns of a signalling function that are the same code
/// (nothing but the mark, the same constant, the same frame): each
/// after the first branches to the first.
#[derive(Default)]
struct SharedThrowRets {
    seen: Vec<(Option<Operand>, bool, BlockId)>,
}

impl SharedThrowRets {
    /// Emit `block` as a branch to the first of its kind, if one was
    /// emitted; `framed` when it returns through the function's frame.
    fn branch(
        &mut self,
        bytes: &mut Vec<u8>,
        fixups: &mut Fixups,
        block: &Block,
        framed: bool,
    ) -> bool {
        let Some(first) = self.target(block, framed) else {
            return false;
        };
        fixups.push(BranchFixup {
            site_byte_offset: bytes.len() as u32,
            target_block: first,
            kind: BranchKind::Imm26,
        });
        write_u32(bytes, b_imm26(0));
        true
    }

    fn target(&mut self, block: &Block, framed: bool) -> Option<BlockId> {
        let Terminator::Ret(op) = block.term else {
            return None;
        };
        let bare = !block.insts.is_empty()
            && (block.insts.iter()).all(|i| matches!(i.kind, InstKind::ThrowSignal));
        if !bare || matches!(op, Some(Operand::Value(_))) {
            return None;
        }
        let same = |&&(o, f, _): &&(Option<Operand>, bool, BlockId)| o == op && f == framed;
        if let Some(&(_, _, first)) = self.seen.iter().find(same) {
            return Some(first);
        }
        self.seen.push((op, framed, block.id));
        None
    }
}
