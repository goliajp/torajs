//! Block-terminator emit + the branch-fixup back-patching pass.
//!
//! Terminators are the one emit family that can't finish in a single
//! forward pass: a branch to a block that hasn't been laid out yet
//! has no displacement to encode. Each branch site therefore records
//! a [`BranchFixup`] with a placeholder word, and
//! [`patch_branch`] rewrites the displacement once every block's byte
//! offset is known.
//!
//! Layout-aware shaping lives here too — fall-through elision (drop a
//! `B` whose target is where execution lands anyway) and the CondBr
//! predicate flip that puts the hot successor on the not-taken path.

use torajs_core::ssa::id_hash::HashMap;

use torajs_core::ssa::{BlockId, Terminator};

use super::landing::Landing;
use super::operand::{self, materialize_operand_fpr, materialize_operand_gpr};
use super::throw_reg::THROW_REG;
use super::{OP_SCRATCH_LHS, bittest, brfuse, write_u32};
use crate::enc::{b_imm26, brk_imm16, cbnz_x, cbz_x, mov_d_reg, mov_x_reg, movz_imm, ret};
use crate::frame::FrameLayout;
use crate::reg::{Fpr, Gpr};
use crate::regalloc::Assignment;

#[derive(Debug, Clone, Copy)]
pub(super) enum BranchKind {
    /// B / BL — 26-bit signed displacement at bits 25-0.
    Imm26,
    /// CBZ / CBNZ / B.cond — 19-bit signed displacement at bits 23-5.
    Imm19,
    /// TBZ / TBNZ — 14-bit signed displacement at bits 18-5 (±32 KiB).
    Imm14,
}

/// The block being emitted, and the final identity of the block whose
/// code follows it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Next {
    pub(super) here: BlockId,
    pub(super) resolved: Option<BlockId>,
}

/// What the terminator does with the throw register
/// (compile/throw_reg.rs): branch on it in place of the absorbed
/// probe, or set it after the epilogue of a return that propagates a
/// throw.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ThrowExit {
    pub(super) probe: bool,
    pub(super) signal: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BranchFixup {
    pub(super) site_byte_offset: u32,
    pub(super) target_block: BlockId,
    pub(super) kind: BranchKind,
}

/// One function's branch sites, waiting for their targets. Under
/// `far`, a conditional branch to a block takes the long form: the
/// inverted test skips a B, whose 26 bits reach ±128 MiB where a
/// B.cond / CBZ / CBNZ reaches ±1 MiB.
pub(super) struct Fixups {
    list: Vec<BranchFixup>,
    far: bool,
}

impl Fixups {
    pub(super) fn new(far: bool) -> Self {
        Fixups {
            list: Vec::new(),
            far,
        }
    }

    pub(super) fn push(&mut self, fixup: BranchFixup) {
        self.list.push(fixup);
    }

    /// Emit `word`, a B.cond / CBZ / CBNZ with a zero displacement,
    /// as a branch to `target`.
    pub(super) fn cond_branch(&mut self, bytes: &mut Vec<u8>, word: u32, target: BlockId) {
        let (word, kind) = if self.far {
            // B.cond flips its condition's low bit, CBZ / CBNZ bit 24;
            // the inverted test jumps over the B to the next word
            let inverted = if word & 0xFF00_0010 == 0x5400_0000 {
                word ^ 1
            } else {
                word ^ (1 << 24)
            };
            write_u32(bytes, inverted | (2 << 5));
            (b_imm26(0), BranchKind::Imm26)
        } else {
            (word, BranchKind::Imm19)
        };
        self.push(BranchFixup {
            site_byte_offset: bytes.len() as u32,
            target_block: target,
            kind,
        });
        write_u32(bytes, word);
    }
}

pub(super) fn patch_branch(bytes: &mut [u8], fixup: &BranchFixup, displacement: i32) {
    debug_assert!(
        displacement % 4 == 0,
        "displacement {displacement} must be 4-byte aligned"
    );
    let inst_offset = displacement / 4;
    let offset = fixup.site_byte_offset as usize;
    let original = u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]);
    let patched = match fixup.kind {
        BranchKind::Imm26 => {
            let imm26 = (inst_offset as u32) & 0x03FF_FFFF;
            (original & !0x03FF_FFFF) | imm26
        }
        BranchKind::Imm19 => {
            let imm19 = (inst_offset as u32) & 0x7_FFFF;
            (original & !(0x7_FFFF << 5)) | (imm19 << 5)
        }
        BranchKind::Imm14 => {
            let imm14 = (inst_offset as u32) & 0x3FFF;
            (original & !(0x3FFF << 5)) | (imm14 << 5)
        }
    };
    bytes[offset..offset + 4].copy_from_slice(&patched.to_le_bytes());
}

/// Patch every branch fixup with its real displacement, each landing
/// where [`Landing::resolve`] says (past empty forwarders and hollow
/// blocks). False — nothing patched — when a displacement does not fit
/// its field.
pub(super) fn patch_all(
    bytes: &mut [u8],
    fixups: &Fixups,
    landing: &Landing,
    block_byte_starts: &HashMap<u32, u32>,
) -> bool {
    let mut patches = Vec::with_capacity(fixups.list.len());
    for fixup in &fixups.list {
        let final_target = landing.resolve(fixup.target_block);
        let target_byte_offset = *block_byte_starts
            .get(&final_target.0)
            .unwrap_or_else(|| panic!("Branch target BlockId({}) not in function", final_target.0));
        let displacement = target_byte_offset as i32 - fixup.site_byte_offset as i32;
        let reach = match fixup.kind {
            BranchKind::Imm26 => 1 << 25,
            BranchKind::Imm19 => 1 << 18,
            BranchKind::Imm14 => 1 << 13,
        };
        if !(-reach..reach).contains(&(displacement / 4)) {
            return false;
        }
        patches.push(displacement);
    }
    for (fixup, displacement) in fixups.list.iter().zip(patches) {
        patch_branch(bytes, fixup, displacement);
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_terminator(
    bytes: &mut Vec<u8>,
    fixups: &mut Fixups,
    term: &Terminator,
    frame: &FrameLayout,
    alloc: &Assignment,
    fused: Option<&brfuse::FusedCmp>,
    landing: &Landing,
    next: Next,
    throw: ThrowExit,
) {
    let resolve = |t: BlockId| landing.resolve(t);
    match term {
        Terminator::Ret(ret_op) => {
            // AAPCS64: int/ptr/bool return lands in X0, f64 in D0. The
            // SSA-emit's prior insts left the ret value in its allocated
            // home reg (X0 in trivial cases, or another GPR / spill slot
            // under linear scan); materialize it into the ABI slot before
            // the epilogue tears down the frame. Idempotent: if the
            // allocator already placed it in X0/V0, `materialize_operand_*`
            // returns the same reg and the mov is skipped. None ret_op
            // = `Stmt::Return;` or void-fn fall-through — leave X0/V0 alone
            // (void calls don't read the ret slot).
            //
            // Pre-fix observed silent-wrong: `function g(){return 5};
            // console.log(g())` emitted a single `ret` for g's body (no
            // const materialization), so X0 held _main's pre-call scratch
            // and console.log printed 1 instead of 5. The SSA Ret operand
            // was reaching emit_terminator but being discarded here.
            if let Some(op) = ret_op {
                if operand::operand_is_f64(op, alloc) {
                    let src = materialize_operand_fpr(bytes, op, Fpr::V0, OP_SCRATCH_LHS, alloc);
                    if src != Fpr::V0 {
                        write_u32(bytes, mov_d_reg(Fpr::V0, src));
                    }
                } else {
                    let src = materialize_operand_gpr(bytes, op, Gpr::X0, alloc);
                    if src != Gpr::X0 {
                        write_u32(bytes, mov_x_reg(Gpr::X0, src));
                    }
                }
            }
            frame.emit_epilogue(bytes);
            if throw.signal {
                write_u32(bytes, movz_imm(THROW_REG, 1, 0));
            }
            write_u32(bytes, ret(Gpr::X30));
        }
        Terminator::Br(target_block) => {
            // Fall-through elision: a B whose (forwarder-resolved)
            // target is where execution lands anyway is dead weight —
            // the popcount/collatz loop bodies string 4-5 of these
            // per iteration through pass-split blocks.
            if Some(resolve(*target_block)) == next.resolved {
                return;
            }
            let site = bytes.len() as u32;
            fixups.push(BranchFixup {
                site_byte_offset: site,
                target_block: *target_block,
                kind: BranchKind::Imm26,
            });
            // Emit B with placeholder displacement; patched later.
            write_u32(bytes, b_imm26(0));
        }
        Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } => {
            let (invert, (br_target, other), trailing) =
                cond_plan(landing, next, *then_blk, *else_blk);
            if let Some(fc) = fused {
                if fc.bits.is_some() {
                    let to = (br_target, other);
                    bittest::emit_condbr(bytes, fixups, fc, invert, to, trailing, alloc);
                    return;
                }
                let trailing = trailing.then_some(other);
                brfuse::emit_fused_condbr(bytes, fixups, fc, invert, br_target, trailing, alloc);
                return;
            }
            // Materialize the Bool cond into a GPR scratch. If the
            // cond is a Value, the allocator already placed it in a
            // register and materialize_operand_gpr is a fast lookup.
            let cond_reg = if throw.probe {
                THROW_REG
            } else {
                operand::materialize_operand_gpr(bytes, cond, OP_SCRATCH_LHS, alloc)
            };
            // CBNZ cond, then_blk — branch when cond != 0 (i.e. true);
            // flipped to CBZ cond, else_blk when the branch goes there.
            let word = if invert {
                cbz_x(cond_reg, 0)
            } else {
                cbnz_x(cond_reg, 0)
            };
            fixups.cond_branch(bytes, word, br_target);
            if trailing {
                let b_site = bytes.len() as u32;
                fixups.push(BranchFixup {
                    site_byte_offset: b_site,
                    target_block: other,
                    kind: BranchKind::Imm26,
                });
                write_u32(bytes, b_imm26(0));
            }
        }
        Terminator::Unreachable => {
            // Clang/LLVM convention for `unreachable`.
            write_u32(bytes, brk_imm16(0xFD));
        }
    }
}

/// How a CondBr to `then_blk` / `else_blk` branches: whether on the
/// inverted condition, the (branch target, other successor) pair, and
/// whether a trailing B to the other successor follows.
fn cond_plan(
    landing: &Landing,
    next: Next,
    then_blk: BlockId,
    else_blk: BlockId,
) -> (bool, (BlockId, BlockId), bool) {
    let to_else = landing.resolve(else_blk);
    let else_falls = Some(to_else) == next.resolved;
    // Layout-driven flip: when the then-target is the fall-through
    // block, branch on the inverted predicate to the else-target
    // instead — the hot successor (loop bodies sit right after their
    // header) runs on the not-taken path and the trailing B disappears.
    let then_falls = !else_falls && Some(landing.resolve(then_blk)) == next.resolved;
    let trailing = !then_falls && !else_falls;
    // With no fall-through, a loop's back edge (to this block or to a
    // block on the path into it) takes the conditional branch and the
    // trailing B, run once on the way out, takes the exit: an iteration
    // pays one branch.
    let back_on_else = trailing
        && landing.goes_back(next.here, else_blk)
        && !landing.goes_back(next.here, then_blk);
    let invert = then_falls || back_on_else;
    let pair = if invert {
        (else_blk, then_blk)
    } else {
        (then_blk, else_blk)
    };
    (invert, pair, trailing)
}

/// `true` iff `term` can hand control to the next block in layout
/// without a branch — the same fall-through tests `emit_terminator`
/// uses to elide or flip its branches.
pub(super) fn falls_through(term: &Terminator, landing: &Landing, next: Next) -> bool {
    let lands_next = |t: &BlockId| Some(landing.resolve(*t)) == next.resolved;
    match term {
        Terminator::Ret(_) | Terminator::Unreachable => false,
        Terminator::Br(t) => lands_next(t),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => lands_next(then_blk) || lands_next(else_blk),
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use crate::enc::{nop, ret};
    use crate::reg::Gpr;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    /// `if (n < 0) return 1; return 2;` — the second return's block
    /// follows a `ret`, so it starts on a 16-byte boundary and the pad
    /// in front of it is NOPs behind that `ret`.
    #[test]
    fn block_after_ret_starts_on_a_fetch_boundary() {
        let func = Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None,
                },
                ValueInfo {
                    ty: Type::Bool,
                    name: None,
                },
            ],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![Inst {
                        result: Some(ValueId(1)),
                        kind: InstKind::ICmp(
                            IPred::Slt,
                            Operand::Value(ValueId(0)),
                            Operand::ConstI64(0),
                        ),
                        origin: None,
                    }],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(1)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::ConstI64(1))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::ConstI64(2))),
                },
            ],
            current_origin: None,
        };
        let bytes = compile_function(&func).bytes;
        let words: Vec<u32> = bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let first_ret = words.iter().position(|&w| w == ret(Gpr::X30)).unwrap();
        let second_block = words[first_ret + 1..]
            .iter()
            .position(|&w| w != nop())
            .unwrap()
            + first_ret
            + 1;
        assert_eq!((second_block * 4) % 16, 0, "{words:08x?}");
        assert!(
            words[first_ret + 1..second_block]
                .iter()
                .all(|&w| w == nop())
        );
        assert!(words[..first_ret].iter().all(|&w| w != nop()));
    }

    /// A loop block `b1` testing `n + 1 != 0`, laid out `b0 b1 b2 b3 b4`:
    /// `b2` (`back`) and the `ret`s at `b3` and `b4`.
    fn loop_words(then_blk: u32, else_blk: u32, back: Terminator) -> Vec<u32> {
        let ret = |n| Terminator::Ret(Some(Operand::ConstI64(n)));
        let block = |id, term| Block {
            id: BlockId(id),
            insts: vec![],
            term,
        };
        let step = InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(0)), Operand::ConstI64(1));
        let test = Block {
            id: BlockId(1),
            insts: vec![
                Inst {
                    result: Some(ValueId(2)),
                    kind: step,
                    origin: None,
                },
                Inst {
                    result: Some(ValueId(1)),
                    kind: InstKind::ICmp(
                        IPred::Ne,
                        Operand::Value(ValueId(2)),
                        Operand::ConstI64(0),
                    ),
                    origin: None,
                },
            ],
            term: Terminator::CondBr {
                cond: Operand::Value(ValueId(1)),
                then_blk: BlockId(then_blk),
                else_blk: BlockId(else_blk),
            },
        };
        let info = |ty| ValueInfo { ty, name: None };
        let func = Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::I64,
            values: vec![info(Type::I64), info(Type::Bool), info(Type::I64)],
            blocks: vec![
                block(0, Terminator::Br(BlockId(1))),
                test,
                block(2, back),
                block(3, ret(3)),
                block(4, ret(4)),
            ],
            current_origin: None,
        };
        compile_function(&func)
            .bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    /// The one conditional branch of `words` and the B words after it,
    /// as (conditional displacement, B displacements).
    fn branches(words: &[u32]) -> (i32, Vec<i32>) {
        let cb = words
            .iter()
            .position(|&w| w & 0x7E00_0000 == 0x3400_0000)
            .unwrap();
        let imm19 = ((words[cb] >> 5) & 0x7_FFFF) as i32;
        let bs = words[cb + 1..]
            .iter()
            .filter(|&&w| w & 0xFC00_0000 == 0x1400_0000)
            .map(|&w| ((w & 0x03FF_FFFF) as i32) << 6 >> 6)
            .collect();
        (imm19 << 13 >> 13, bs)
    }

    /// A self-loop whose exit is not next takes its back edge with the
    /// conditional branch and leaves by the trailing B.
    #[test]
    fn self_loop_branches_back_conditionally() {
        let words = loop_words(4, 1, Terminator::Ret(Some(Operand::ConstI64(2))));
        let (back, bs) = branches(&words);
        assert!(back < 0 && bs.len() == 1 && bs[0] > 0, "{words:08x?}");
    }

    /// A back edge through an empty block laid out next: the block is
    /// not emitted, the loop branches back once and falls through to
    /// its exit behind it.
    #[test]
    fn back_edge_through_a_forwarder_branches_once() {
        let words = loop_words(2, 3, Terminator::Br(BlockId(1)));
        let (back, bs) = branches(&words);
        assert!(back <= 0 && bs.is_empty(), "{words:08x?}");
    }

    /// `p != 0` into Bool value `v`.
    fn test_inst(v: u32) -> Inst {
        Inst {
            result: Some(ValueId(v)),
            kind: InstKind::ICmp(IPred::Ne, Operand::Value(ValueId(0)), Operand::ConstI64(0)),
            origin: None,
        }
    }

    /// A block with one `test_inst` into `v` that branches on it.
    fn test_block(id: u32, v: u32, then_blk: u32, else_blk: u32) -> Block {
        Block {
            id: BlockId(id),
            insts: vec![test_inst(v)],
            term: Terminator::CondBr {
                cond: Operand::Value(ValueId(v)),
                then_blk: BlockId(then_blk),
                else_blk: BlockId(else_blk),
            },
        }
    }

    /// The words of `blocks` compiled as `f(p)`, with Bool values 1..=`bools`.
    fn compile_words(blocks: Vec<Block>, bools: usize) -> Vec<u32> {
        let info = |ty| ValueInfo { ty, name: None };
        let mut values = vec![info(Type::I64)];
        values.extend((0..bools).map(|_| info(Type::Bool)));
        let func = Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::I64,
            values,
            blocks,
            current_origin: None,
        };
        compile_function(&func)
            .bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn simple(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn ret_n(n: i64) -> Terminator {
        Terminator::Ret(Some(Operand::ConstI64(n)))
    }

    /// A latch whose exit runs through a hollow block it does not fall
    /// into, laid out `b0 b1 b2 b3 b4 b5`: b1 is the header, b2 the
    /// latch (`CondBr(b3, b1)`), b3 an empty block to b5, and b4 / b5
    /// return. The back edge to the header takes the conditional
    /// branch, and the only B after it is the exit, run once.
    #[test]
    fn latch_back_edge_to_an_earlier_header_is_conditional() {
        let words = compile_words(
            vec![
                simple(0, vec![], Terminator::Br(BlockId(1))),
                simple(1, vec![test_inst(1)], Terminator::Br(BlockId(2))),
                test_block(2, 2, 3, 1),
                simple(3, vec![], Terminator::Br(BlockId(5))),
                simple(4, vec![], ret_n(4)),
                simple(5, vec![], ret_n(5)),
            ],
            2,
        );
        let cb = words
            .iter()
            .position(|&w| w & 0x7E00_0000 == 0x3400_0000)
            .unwrap();
        let is_b = |w: &u32| w & 0xFC00_0000 == 0x1400_0000;
        assert!(!words[..cb].iter().any(is_b), "{words:08x?}");
        let (back, bs) = branches(&words);
        assert!(back < 0 && bs.len() == 1 && bs[0] > 0, "{words:08x?}");
        assert!(is_b(&words[cb + 1]), "{words:08x?}");
    }

    /// A branch to a block laid out earlier that is no back edge, laid
    /// out `b0 b1 b2 b4 b3 b6 b5`: header b1 tests into b2 or b3, the latch
    /// b2 goes back to b1 or out to b5, and b3, reached from b1 beside
    /// b2, goes out to b5 or on to b2. No path from b2 reaches b3 but
    /// through the header, so b3's branch to b2 closes no cycle: b3 keeps
    /// its compare on the exit, and only the latch's back edge takes a
    /// conditional branch backwards.
    #[test]
    fn a_forward_edge_to_an_earlier_block_keeps_its_polarity() {
        let words = compile_words(
            vec![
                simple(0, vec![], Terminator::Br(BlockId(1))),
                test_block(1, 1, 2, 3),
                test_block(2, 2, 5, 1),
                simple(4, vec![], ret_n(4)),
                test_block(3, 3, 5, 2),
                simple(6, vec![], ret_n(6)),
                simple(5, vec![], ret_n(5)),
            ],
            3,
        );
        let cbs: Vec<i32> = words
            .iter()
            .filter(|&&w| w & 0x7E00_0000 == 0x3400_0000)
            .map(|&w| (((w >> 5) & 0x7_FFFF) as i32) << 13 >> 13)
            .collect();
        // b1 to b3, b2 back to b1, b3 out to b5
        assert!(cbs.len() == 3, "{words:08x?}");
        assert!(cbs[0] > 0 && cbs[1] < 0 && cbs[2] > 0, "{words:08x?}");
    }

    /// A branch into a chain of twenty empty blocks lands at its end,
    /// and none of the chain is emitted.
    #[test]
    fn long_forwarder_chain_lands_at_its_end() {
        let mut blocks = vec![Block {
            id: BlockId(0),
            insts: vec![Inst {
                result: Some(ValueId(1)),
                kind: InstKind::ICmp(IPred::Ne, Operand::Value(ValueId(0)), Operand::ConstI64(0)),
                origin: None,
            }],
            term: Terminator::CondBr {
                cond: Operand::Value(ValueId(1)),
                then_blk: BlockId(1),
                else_blk: BlockId(22),
            },
        }];
        blocks.extend((1..=20).map(|i| Block {
            id: BlockId(i),
            insts: vec![],
            term: Terminator::Br(BlockId(i + 1)),
        }));
        for (id, n) in [(21, 1), (22, 2)] {
            blocks.push(Block {
                id: BlockId(id),
                insts: vec![],
                term: Terminator::Ret(Some(Operand::ConstI64(n))),
            });
        }
        let info = |ty| ValueInfo { ty, name: None };
        let func = Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::I64,
            values: vec![info(Type::I64), info(Type::Bool)],
            blocks,
            current_origin: None,
        };
        let words: Vec<u32> = compile_function(&func)
            .bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let (to_else, bs) = branches(&words);
        assert!(to_else > 0 && bs.is_empty(), "{words:08x?}");
    }

    /// A conditional branch over a block past the ±1 MiB a B.cond /
    /// CBZ / CBNZ reaches takes the long form, and every branch lands
    /// inside the function.
    #[test]
    fn a_branch_past_one_mib_lands_in_the_function() {
        let mut f = Function::new("far", Type::I64);
        let b0 = f.add_block();
        let b1 = f.add_block();
        let b2 = f.add_block();
        let p = f.add_param(Type::I64, "p");
        let c = f.append_inst(
            b0,
            InstKind::ICmp(IPred::Ne, Operand::Value(p), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        f.set_term(
            b0,
            Terminator::CondBr {
                cond: Operand::Value(c),
                then_blk: b1,
                else_blk: b2,
            },
        );
        let mut v = p;
        for _ in 0..300_000 {
            let add = InstKind::BinOp(BinOp::Add, Operand::Value(v), Operand::ConstI64(1));
            v = f.append_inst(b1, add, Type::I64, None);
        }
        f.set_term(b1, Terminator::Ret(Some(Operand::Value(v))));
        f.set_term(b2, Terminator::Ret(Some(Operand::ConstI64(0))));
        let bytes = compile_function(&f).bytes;
        let words: Vec<u32> = (bytes.as_chunks::<4>().0.iter())
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        assert!(words.len() > 1 << 18);
        for (i, &w) in words.iter().enumerate() {
            let disp = if w & 0xFC00_0000 == 0x1400_0000 {
                ((w << 6) as i32) >> 6
            } else if w & 0xFF00_0010 == 0x5400_0000 || w & 0x7E00_0000 == 0x3400_0000 {
                ((w << 8) as i32) >> 13
            } else {
                continue;
            };
            let target = i as i64 + disp as i64;
            assert!((0..words.len() as i64).contains(&target), "{i}: {w:08x}");
        }
    }
}
