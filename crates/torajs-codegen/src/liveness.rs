//! SSA value liveness analysis — Linear Scan input.
//!
//! Phase 1 of Linear Scan (Poletto & Sarkar 1999): walk the function
//! in block order, assign every inst slot a unique global index, and
//! record `Interval { start, end }` per `ValueId`:
//!
//!   - `start` = the inst index at which the value is defined.
//!     Function params live from index 0 (before any inst slot).
//!   - `end`   = the maximum inst index that references the value
//!     as an operand. If the value is never used, `end == start`
//!     (a dead-after-def slot — the allocator can still pick a
//!     register for the def site but the slot expires immediately).
//!
//! Phase 2 (active set + register allocation) and Phase 3 (spill to
//! alloca region) land in follow-up sub-steps S5-gap-2-{2,3,4} —
//! they consume the `HashMap<u32, Interval>` produced here.
//!
//! ## Index numbering
//!
//! Each block contributes `(block.insts.len() + 1)` indices: one per
//! inst slot, then one for the terminator. A 3-block function with
//! 2/0/4 insts numbers as
//!
//! ```text
//!   block 0: insts at 0, 1; terminator at 2
//!   block 1: terminator at 3
//!   block 2: insts at 4, 5, 6, 7; terminator at 8
//! ```
//!
//! Terminator slots own their own index so operands referenced by
//! `Ret(Some(v))` / `CondBr { cond: v, .. }` extend the value's
//! interval through the end of the block.
//!
//! ## Cross-block use
//!
//! The current SSA has no `Phi` instruction — multi-block control
//! flow shares ValueIds directly: block 0 defines `v0`, block 1
//! uses `v0`. The linear index walk naturally captures this — when
//! block 1's use is reached, `v0`'s interval `end` advances past
//! block 1's index without any block-boundary special case.
//!
//! That makes intervals "live-set safe" for the trivial multi-block
//! shapes the rest of codegen already handles via `BranchFixup`.
//! Loops and more elaborate join patterns would need a fixed-point
//! data-flow pass instead — see the alloca-result carve-out below
//! for the only loop-relevant case the lowerer currently emits.
//!
//! ## Loop-aware extension (Phase 1 single-pass + Phase 2 CFG fix-up)
//!
//! Single-pass liveness assumes execution follows textual order —
//! true for straight-line code and forward branches, broken by
//! backedges. A loop body's load that reads from an alloca slot
//! defined in the entry block looks "expired" once the linear walk
//! moves past the last textual use, even though the next iteration
//! will re-read it. The allocator then hands the entry-block
//! register to a `cset` inside the loop body, and the backedge
//! delivers garbage to the next load.
//! `ssa_lower_str::lower_array_index_of_inline_emit` (Array
//! `indexOf` / `lastIndexOf` / `includes`) hits this canonically.
//!
//! Fix: after the linear pass, run a textbook live-in / live-out
//! fix-point over the block CFG (use[B], def[B], live_in[B] =
//! use[B] ∪ (live_out[B] \ def[B]), live_out[B] =
//! ∪{live_in[s] : s ∈ succ(B)}). Any `ValueId` live across a block
//! has its interval extended to that block's end. Backedges
//! naturally close the loop: `live_out[loop_body]` pulls in
//! `live_in[loop_header]`, which propagates the header's
//! invariants (param-derived loads, alloca-addr loads) back over
//! the body.
//!
//! Param ValueIds get the same treatment via the carve-out below —
//! AAPCS64-arg ValueIds are not stored back to SSA `Store/Load`
//! pairs in every shape (some lowers consume them in place), so
//! pinning `func.params` to `[def, function_end]` covers the case
//! the fix-point can't see (an arg used only after the loop body
//! still needs to survive its `cset` clobber).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Operand, Terminator, ValueId};

/// Inclusive live interval `[start, end]` over the global inst-slot
/// numbering returned by [`compute_intervals`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    pub start: u32,
    pub end: u32,
}

impl Interval {
    /// `true` iff `idx` lies within `[start, end]`.
    pub fn contains(&self, idx: u32) -> bool {
        idx >= self.start && idx <= self.end
    }

    /// `true` iff this interval shares any index with `other`.
    /// Linear Scan's "active set" expiry test uses the negation:
    /// an active interval whose `end < new.start` no longer
    /// overlaps with anything and can release its register.
    pub fn overlaps(&self, other: &Interval) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// Walk `func` in block order and compute the live interval of every
/// `ValueId` referenced (as a `Function::params` entry, an `Inst.result`,
/// or an `Operand::Value` operand of an inst or terminator).
///
/// Returned keys are `ValueId.0` (the raw u32) — matching the rest of
/// `regalloc::Assignment`'s map layout for drop-in use.
pub fn compute_intervals(func: &Function) -> HashMap<u32, Interval> {
    let mut out: HashMap<u32, Interval> = HashMap::new();
    // Collected during the walk and patched at the end — see the
    // "Alloca-result carve-out" section in the module docs.
    let mut alloca_results: Vec<u32> = Vec::new();

    // Params are live from before any inst executes — model that as
    // index 0. Any subsequent use will extend `end` past 0; a param
    // that's never used keeps `end == 0` (immediately dead, but the
    // allocator still needs an AAPCS64 arg-lane register for the
    // call-frame entry, which `regalloc::allocate_trivial` already
    // handles via the param pre-walk in S5-gap-1).
    for &p in &func.params {
        out.insert(p.0, Interval { start: 0, end: 0 });
    }

    let mut idx: u32 = 0;
    for block in &func.blocks {
        for inst in &block.insts {
            // Visit operands first so any value used by this inst has
            // its `end` extended to the current index *before* its
            // own def (if any) lands. Practically: an inst whose
            // result reuses one of its own operand registers (a fold
            // the trivial allocator never does, but a real allocator
            // might) sees the operand as "still alive at idx".
            visit_inst_operands(&inst.kind, |v| {
                if let Some(it) = out.get_mut(&v.0) {
                    if idx > it.end {
                        it.end = idx;
                    }
                }
                // Operand references a value the lowerer never defined?
                // Likely an upstream bug, but liveness is best-effort —
                // the allocator will surface it as a hard error later.
            });

            if let Some(result) = inst.result {
                // `entry().or_insert(...)` rather than `insert` to
                // tolerate SSA shapes that name the same ValueId
                // multiple times (the SSA the lowerer produces is
                // single-assignment, but the data structure doesn't
                // statically enforce it — defensive).
                out.entry(result.0).or_insert(Interval {
                    start: idx,
                    end: idx,
                });
                if matches!(inst.kind, InstKind::Alloca(_) | InstKind::AllocaBytes(_)) {
                    alloca_results.push(result.0);
                }
            }
            idx += 1;
        }

        // Terminator slot has its own index. Operands extend through
        // it; Br / Unreachable contribute none, Ret/CondBr each
        // contribute one.
        visit_terminator_operands(&block.term, |v| {
            if let Some(it) = out.get_mut(&v.0) {
                if idx > it.end {
                    it.end = idx;
                }
            }
        });
        idx += 1;
    }

    let last_idx = idx.saturating_sub(1);

    // Phase 2 — CFG live-in/live-out fix-point. Pins any ValueId
    // that crosses a block boundary (especially backedges) to a
    // live interval covering every block where it stays live.
    // See module docs for the full algorithm.
    extend_intervals_loop_aware(func, &mut out);

    // Alloca-result + function-param carve-out — see module docs.
    // Pure data-flow can't always see params (some lowers don't
    // store-and-reload them, so the param's textual last-use precedes
    // the loop body that consumes a reloaded copy), so pin both
    // classes to `[def, last_idx]` regardless.
    for v in alloca_results {
        if let Some(it) = out.get_mut(&v)
            && last_idx > it.end
        {
            it.end = last_idx;
        }
    }
    for &p in &func.params {
        if let Some(it) = out.get_mut(&p.0)
            && last_idx > it.end
        {
            it.end = last_idx;
        }
    }

    out
}

/// Run Phase 2 — a textbook backward live-in / live-out fix-point
/// over the block CFG — and extend every ValueId's `end` to cover
/// every block where it stays live. See module docs.
fn extend_intervals_loop_aware(func: &Function, out: &mut HashMap<u32, Interval>) {
    let n = func.blocks.len();
    if n == 0 {
        return;
    }

    // Index-aligned bookkeeping over `func.blocks`.
    let mut block_start_idx: Vec<u32> = vec![0; n];
    let mut block_end_idx: Vec<u32> = vec![0; n];
    let mut block_uses: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let mut block_defs: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let mut block_succs: Vec<Vec<usize>> = vec![Vec::new(); n];

    // Map BlockId.0 → position in `func.blocks` (BlockId may be
    // non-contiguous in pathological shapes).
    let mut block_id_to_pos: HashMap<u32, usize> = HashMap::new();
    for (i, block) in func.blocks.iter().enumerate() {
        block_id_to_pos.insert(block.id.0, i);
    }

    // Walk to compute per-block use/def and end_idx in the same
    // global numbering as `compute_intervals`.
    let mut cursor: u32 = 0;
    for (i, block) in func.blocks.iter().enumerate() {
        block_start_idx[i] = cursor;
        for inst in &block.insts {
            visit_inst_operands(&inst.kind, |v| {
                if !block_defs[i].contains(&v.0) {
                    block_uses[i].insert(v.0);
                }
            });
            if let Some(result) = inst.result {
                block_defs[i].insert(result.0);
            }
            cursor += 1;
        }
        visit_terminator_operands(&block.term, |v| {
            if !block_defs[i].contains(&v.0) {
                block_uses[i].insert(v.0);
            }
        });
        block_end_idx[i] = cursor; // terminator owns this slot
        cursor += 1;

        match &block.term {
            Terminator::Br(t) => {
                if let Some(&p) = block_id_to_pos.get(&t.0) {
                    block_succs[i].push(p);
                }
            }
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                if let Some(&p) = block_id_to_pos.get(&then_blk.0) {
                    block_succs[i].push(p);
                }
                if let Some(&p) = block_id_to_pos.get(&else_blk.0) {
                    block_succs[i].push(p);
                }
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }

    // Fix-point: live_in[B] = use[B] ∪ (live_out[B] \ def[B]);
    // live_out[B] = ∪{live_in[s] : s ∈ succ(B)}.
    let mut live_in: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let mut live_out: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    loop {
        let mut changed = false;
        for b in (0..n).rev() {
            let mut new_out: HashSet<u32> = HashSet::new();
            for &s in &block_succs[b] {
                for v in &live_in[s] {
                    new_out.insert(*v);
                }
            }
            if new_out != live_out[b] {
                live_out[b] = new_out;
                changed = true;
            }
            let mut new_in = block_uses[b].clone();
            for v in &live_out[b] {
                if !block_defs[b].contains(v) {
                    new_in.insert(*v);
                }
            }
            if new_in != live_in[b] {
                live_in[b] = new_in;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Extend every value's end to the last block where it stays
    // live (live_in covers "used inside this block from before",
    // live_out covers "passes through this block to a successor"),
    // and pull its start back to the first block where it is
    // live-in. The pull-back matters when block layout order runs
    // against the CFG (a `continue`-arm block laid out AFTER the
    // loop latch that reads its def): the use's linear index is then
    // SMALLER than the def's, and an interval keyed on the def index
    // alone would leave the use uncovered — the allocator could hand
    // the register to someone else right over it. First hit by
    // phi_promote's Copy defs (slot round-trips never produced
    // cross-block uses before), but the property is generic.
    for b in 0..n {
        let start = block_start_idx[b];
        let end = block_end_idx[b];
        for v in live_in[b].iter().chain(live_out[b].iter()) {
            if let Some(it) = out.get_mut(v) {
                if end > it.end {
                    it.end = end;
                }
                if live_in[b].contains(v) && start < it.start {
                    it.start = start;
                }
            }
        }
    }
}

/// Invoke `f` on every `ValueId` referenced as an operand of `kind`.
/// Mirrors the InstKind dispatch in `compile::emit_inst` so adding a
/// new variant there forces this match to be updated too.
pub(crate) fn visit_inst_operands(kind: &InstKind, mut f: impl FnMut(ValueId)) {
    let mut visit = |op: &Operand| {
        if let Operand::Value(v) = op {
            f(*v);
        }
    };
    match kind {
        InstKind::BinOp(_, a, b) => {
            visit(a);
            visit(b);
        }
        InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) => {
            visit(a);
            visit(b);
        }
        InstKind::BitCastF64ToI64(a) | InstKind::BitCastI64ToF64(a) => visit(a),
        InstKind::SiToFp(a) | InstKind::FpToSi(a) => visit(a),
        InstKind::ZExtBoolToI64(a) | InstKind::ZExtI32ToI64(a) | InstKind::TruncI64ToBool(a) => {
            visit(a)
        }
        InstKind::PtrToInt(a) | InstKind::IntToPtr(a) => visit(a),
        InstKind::Neg(a) => visit(a),
        InstKind::Ctpop(a) => visit(a),
        InstKind::Copy(_, a) => visit(a),
        InstKind::Select(_, c, t, e) => {
            visit(c);
            visit(t);
            visit(e);
        }
        InstKind::CtpopRangeSum(s, b, a) => {
            visit(s);
            visit(b);
            visit(a);
        }
        InstKind::Alloca(_) | InstKind::AllocaBytes(_) => {}
        InstKind::Load(_, ptr, _) => visit(ptr),
        InstKind::Store(val, ptr, _) => {
            visit(val);
            visit(ptr);
        }
        InstKind::LoadDyn(_, base, off) => {
            visit(base);
            visit(off);
        }
        InstKind::StoreDyn(val, base, off) => {
            visit(val);
            visit(base);
            visit(off);
        }
        InstKind::LoadDynScaled8(_, base, idx) => {
            visit(base);
            visit(idx);
        }
        InstKind::LoadU8Dyn(base, idx) => {
            visit(base);
            visit(idx);
        }
        InstKind::StoreDynScaled8(val, base, idx) => {
            visit(val);
            visit(base);
            visit(idx);
        }
        InstKind::Call(_, args) => {
            for a in args {
                visit(a);
            }
        }
        InstKind::CallIndirect(_, fp, args) => {
            visit(fp);
            for a in args {
                visit(a);
            }
        }
        // Leaf "address materialize" ops carry no Operand inputs.
        InstKind::GlobalRef(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_) => {}
        // P-OPT Phase 1 — `Identity` is dropped by egraph elaborate
        // before liveness ever runs. If it survives, liveness would
        // see a "result with no real def site" and produce nonsense;
        // signal the bug instead of silently accepting it.
        InstKind::Identity(_) => unreachable!(
            "InstKind::Identity reached liveness — egraph elaborate must \
             drop it via set_opt_value before this point"
        ),
    }
}

/// Invoke `f` on every `ValueId` referenced by a terminator's operands.
pub(crate) fn visit_terminator_operands(term: &Terminator, mut f: impl FnMut(ValueId)) {
    let mut visit = |op: &Operand| {
        if let Operand::Value(v) = op {
            f(*v);
        }
    };
    match term {
        Terminator::Ret(Some(op)) => visit(op),
        Terminator::Ret(None) => {}
        Terminator::Br(_) => {}
        Terminator::CondBr { cond, .. } => visit(cond),
        Terminator::Unreachable => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    /// Smallest case — `fn f() -> i64 { 1 + 2 }`:
    ///   slot 0: v0 = BinOp(Add, 1, 2)
    ///   slot 1: Ret(v0)
    /// v0's interval: defined at 0, last-used at 1.
    #[test]
    fn one_plus_two_interval() {
        let v0 = ValueId(0);
        let func = Function {
            name: "f".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        };
        let ivs = compute_intervals(&func);
        assert_eq!(ivs.len(), 1);
        assert_eq!(ivs[&0], Interval { start: 0, end: 1 });
    }

    /// `fn identity(x: i64) -> i64 { x }` — param x is live from
    /// index 0 (params start) through the terminator at index 0
    /// (single-block: terminator owns slot 0 because no body insts).
    #[test]
    fn param_only_no_body_interval() {
        let x = ValueId(0);
        let func = Function {
            name: "identity".into(),
            params: vec![x],
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("x".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: Vec::new(),
                term: Terminator::Ret(Some(Operand::Value(x))),
            }],
            current_origin: None,
        };
        let ivs = compute_intervals(&func);
        assert_eq!(ivs[&0], Interval { start: 0, end: 0 });
    }

    /// mem2reg φ-destruction contract — the SAME ValueId defined by
    /// `Copy` on two predecessors (entry + back-edge) must merge into
    /// ONE interval that covers from the first def through the
    /// back-edge block's end, so regalloc gives every def the same
    /// home:
    ///   bb0: %0 = copy i64 #5        ; idx 0  (entry def)
    ///        br bb1                   ; idx 1
    ///   bb1: %1 = add %0, 1          ; idx 2  (use)
    ///        cond_br %1, bb2, bb3     ; idx 3
    ///   bb2: %0 = copy i64 %1        ; idx 4  (back-edge re-def)
    ///        br bb1                   ; idx 5
    ///   bb3: ret                      ; idx 6
    /// %0 ∈ live_in(bb1) and bb2 → bb1, so live_out(bb2) holds %0 and
    /// the CFG fix-point extends its end through bb2 (≥ idx 5).
    #[test]
    fn copy_multi_def_merges_into_one_interval() {
        let v0 = ValueId(0);
        let v1 = ValueId(1);
        let mk = |ty: Type| ValueInfo { ty, name: None };
        let func = Function {
            name: "loopcopy".into(),
            params: Vec::new(),
            ret: Type::Void,
            values: vec![mk(Type::I64), mk(Type::I64)],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![Inst {
                        result: Some(v0),
                        kind: InstKind::Copy(Type::I64, Operand::ConstI64(5)),
                        origin: None,
                    }],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![Inst {
                        result: Some(v1),
                        kind: InstKind::BinOp(BinOp::Add, Operand::Value(v0), Operand::ConstI64(1)),
                        origin: None,
                    }],
                    term: Terminator::CondBr {
                        cond: Operand::Value(v1),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                },
                Block {
                    id: BlockId(2),
                    insts: vec![Inst {
                        result: Some(v0),
                        kind: InstKind::Copy(Type::I64, Operand::Value(v1)),
                        origin: None,
                    }],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(3),
                    insts: Vec::new(),
                    term: Terminator::Ret(None),
                },
            ],
            current_origin: None,
        };
        let ivs = compute_intervals(&func);
        let iv = ivs[&0];
        assert_eq!(iv.start, 0, "first def wins the interval start");
        assert!(
            iv.end >= 5,
            "back-edge keeps %0 live through bb2's end (got end={})",
            iv.end
        );
    }

    /// Layout order against the CFG: entry jumps to bb2 (laid out
    /// last), which defines %0 and branches to bb1 (laid out FIRST),
    /// which uses it. The use's linear index is smaller than the
    /// def's — the interval must pull its start back to bb1's block
    /// start or the allocator hands %0's register away over the use.
    /// (The shape phi_promote's continue-arm Copy defs produce.)
    #[test]
    fn layout_reversed_cross_block_use_pulls_start_back() {
        let v0 = ValueId(0);
        let v1 = ValueId(1);
        let mk = |ty: Type| ValueInfo { ty, name: None };
        let func = Function {
            name: "revuse".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![mk(Type::I64), mk(Type::I64)],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: Vec::new(),
                    term: Terminator::Br(BlockId(2)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![Inst {
                        result: Some(v1),
                        kind: InstKind::BinOp(BinOp::Add, Operand::Value(v0), Operand::ConstI64(1)),
                        origin: None,
                    }],
                    term: Terminator::Ret(Some(Operand::Value(v1))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![Inst {
                        result: Some(v0),
                        kind: InstKind::BinOp(
                            BinOp::Add,
                            Operand::ConstI64(2),
                            Operand::ConstI64(3),
                        ),
                        origin: None,
                    }],
                    term: Terminator::Br(BlockId(1)),
                },
            ],
            current_origin: None,
        };
        // linear indices: bb0 term = 0; bb1 add = 1, ret = 2; bb2 add = 3, br = 4.
        let ivs = compute_intervals(&func);
        let iv = ivs[&0];
        assert!(
            iv.start <= 1,
            "start must cover bb1's use at idx 1 (got start={})",
            iv.start
        );
        assert!(iv.end >= 3, "end must cover the def (got end={})", iv.end);
    }

    /// `fn dead(): void { _ = 1 + 2; ret }` — v0 defined at slot 0,
    /// never used downstream. Interval `[0, 0]` (def-only).
    #[test]
    fn unused_def_interval_is_def_only() {
        let v0 = ValueId(0);
        let func = Function {
            name: "dead".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                    origin: None,
                }],
                // No use of v0 anywhere.
                term: Terminator::Ret(None),
            }],
            current_origin: None,
        };
        let ivs = compute_intervals(&func);
        assert_eq!(ivs[&0], Interval { start: 0, end: 0 });
    }

    /// `fn chain() -> i64 { let a = 1 + 2; let b = a + a; ret b }` —
    /// a's interval extends to slot 1 where b uses it; b's
    /// interval is [1, 2] (def at 1, ret at 2).
    #[test]
    fn chain_two_defs_two_uses() {
        let a = ValueId(0);
        let b = ValueId(1);
        let func = Function {
            name: "chain".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("a".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("b".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(a),
                        kind: InstKind::BinOp(
                            BinOp::Add,
                            Operand::ConstI64(1),
                            Operand::ConstI64(2),
                        ),
                        origin: None,
                    },
                    Inst {
                        result: Some(b),
                        kind: InstKind::BinOp(BinOp::Add, Operand::Value(a), Operand::Value(a)),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(Some(Operand::Value(b))),
            }],
            current_origin: None,
        };
        let ivs = compute_intervals(&func);
        assert_eq!(ivs[&0], Interval { start: 0, end: 1 });
        assert_eq!(ivs[&1], Interval { start: 1, end: 2 });
    }

    /// Multi-block: `fn cross() -> i64 { block0: v0 = 1+2; B block1; block1: ret v0 }`.
    /// v0 defined at slot 0 (block0's only inst), used at slot 2
    /// (block1's terminator after block0's Br at slot 1).
    #[test]
    fn cross_block_value_use() {
        let v0 = ValueId(0);
        let func = Function {
            name: "cross".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![Inst {
                        result: Some(v0),
                        kind: InstKind::BinOp(
                            BinOp::Add,
                            Operand::ConstI64(1),
                            Operand::ConstI64(2),
                        ),
                        origin: None,
                    }],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: Vec::new(),
                    term: Terminator::Ret(Some(Operand::Value(v0))),
                },
            ],
            current_origin: None,
        };
        let ivs = compute_intervals(&func);
        // block0 inst @ 0, Br @ 1; block1 Ret @ 2.
        assert_eq!(ivs[&0], Interval { start: 0, end: 2 });
    }

    /// CondBr's operand counts as a use that extends the interval
    /// through the terminator slot.
    #[test]
    fn condbr_operand_extends_interval() {
        let v0 = ValueId(0);
        let func = Function {
            name: "cond".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::Bool,
                name: Some("v0".into()),
            }],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![Inst {
                        result: Some(v0),
                        kind: InstKind::TruncI64ToBool(Operand::ConstI64(1)),
                        origin: None,
                    }],
                    term: Terminator::CondBr {
                        cond: Operand::Value(v0),
                        then_blk: BlockId(1),
                        else_blk: BlockId(1),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: Vec::new(),
                    term: Terminator::Ret(None),
                },
            ],
            current_origin: None,
        };
        let ivs = compute_intervals(&func);
        // v0 def @ 0, CondBr @ 1 (last use), Ret @ 2 (no v0 use).
        assert_eq!(ivs[&0], Interval { start: 0, end: 1 });
    }

    /// Interval helpers — contains + overlaps.
    #[test]
    fn interval_helpers() {
        let a = Interval { start: 0, end: 5 };
        let b = Interval { start: 3, end: 7 };
        let c = Interval { start: 6, end: 10 };
        // contains
        assert!(a.contains(0));
        assert!(a.contains(5));
        assert!(!a.contains(6));
        // overlaps — symmetric
        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
        // a vs c — touches at 5..6 boundary, no overlap (5 < 6).
        assert!(!a.overlaps(&c));
        // b vs c — shares [6, 7].
        assert!(b.overlaps(&c));
    }

    /// Call operand list — every arg + the SSA call site itself
    /// extends the args' intervals.
    #[test]
    fn call_args_extend_intervals() {
        let a = ValueId(0);
        let b = ValueId(1);
        let c = ValueId(2);
        use torajs_core::ssa::FuncId;
        let func = Function {
            name: "use_args".into(),
            params: vec![a, b],
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("a".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("b".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("c".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(c),
                    kind: InstKind::Call(FuncId(7), vec![Operand::Value(a), Operand::Value(b)]),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(c))),
            }],
            current_origin: None,
        };
        let ivs = compute_intervals(&func);
        // a and b are function params — the carve-out pins them to
        // the whole function so a loop-body load after a backedge
        // can't read from a register the linear walk thought was
        // free (see module docs). Last idx = 1 here (1 inst slot +
        // 1 terminator slot, terminator owns slot 1).
        assert_eq!(ivs[&0], Interval { start: 0, end: 1 });
        assert_eq!(ivs[&1], Interval { start: 0, end: 1 });
        // c def at slot 0, used by Ret at slot 1.
        assert_eq!(ivs[&2], Interval { start: 0, end: 1 });
    }

    /// Phase-2 loop-aware liveness regression — the canonical
    /// `Array.prototype.indexOf` inline-emit shape that triggered
    /// chunk 9a. Three blocks: entry → header → body → header
    /// (backedge). Value `v0` is defined in entry and re-loaded
    /// from `[v0, #8]` inside body on every iteration. Single-pass
    /// liveness would end v0 at body's load (the textual last
    /// use); the CFG fix-point must extend v0 through body's
    /// terminator (the backedge slot) so the allocator keeps a
    /// register reserved for it across `cset`-shaped intra-body
    /// defs.
    ///
    /// Block layout (`block_end_idx` in parens, terminators own
    /// their own slot):
    ///   B0 entry: idx 0 (Alloca), 1 (Br header)
    ///   B1 header: idx 2 (Load v0+0 → cond), 3 (CondBr body/exit)
    ///   B2 body: idx 4 (Load v0+8), 5 (Br header) ← backedge
    ///   B3 exit: idx 6 (Ret)
    #[test]
    fn loop_aware_extends_through_backedge() {
        let v0 = ValueId(0);
        let v_cond = ValueId(1);
        let v_body = ValueId(2);
        let func = Function {
            name: "loop_body".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v0".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("cond".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("body".into()),
                },
            ],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![Inst {
                        result: Some(v0),
                        kind: InstKind::Alloca(Type::I64),
                        origin: None,
                    }],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![Inst {
                        result: Some(v_cond),
                        kind: InstKind::Load(Type::I64, Operand::Value(v0), 0),
                        origin: None,
                    }],
                    term: Terminator::CondBr {
                        cond: Operand::Value(v_cond),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                },
                Block {
                    id: BlockId(2),
                    insts: vec![Inst {
                        result: Some(v_body),
                        kind: InstKind::Load(Type::I64, Operand::Value(v0), 8),
                        origin: None,
                    }],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(3),
                    insts: Vec::new(),
                    term: Terminator::Ret(None),
                },
            ],
            current_origin: None,
        };

        let ivs = compute_intervals(&func);
        // v0 is alloca-result + loop-invariant; the carve-out alone
        // pins it to the whole function. Verify it survives at
        // least through the body's backedge (idx 5).
        assert!(
            ivs[&0].end >= 5,
            "v0 (loop-invariant alloca) should stay live through backedge at idx 5, got end={}",
            ivs[&0].end
        );
        // cond is defined in header (idx 2) and used by header's
        // terminator (idx 3). It does NOT cross to body, so the
        // fix-point should leave it at end=3.
        assert_eq!(
            ivs[&1].end, 3,
            "cond should expire at header's terminator slot 3"
        );
        // body-load result is dead immediately (no use).
        assert_eq!(ivs[&2].start, ivs[&2].end);
    }
}
