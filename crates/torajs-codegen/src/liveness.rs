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
//! data-flow pass instead; the SSA the lowerer currently produces
//! does not need one.

use std::collections::HashMap;

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

    out
}

/// Invoke `f` on every `ValueId` referenced as an operand of `kind`.
/// Mirrors the InstKind dispatch in `compile::emit_inst` so adding a
/// new variant there forces this match to be updated too.
fn visit_inst_operands(kind: &InstKind, mut f: impl FnMut(ValueId)) {
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
        | InstKind::FnAddr(_) => {}
    }
}

/// Invoke `f` on every `ValueId` referenced by a terminator's operands.
fn visit_terminator_operands(term: &Terminator, mut f: impl FnMut(ValueId)) {
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
        // a and b extend from param start (0) through the Call use
        // at inst slot 0.
        assert_eq!(ivs[&0], Interval { start: 0, end: 0 });
        assert_eq!(ivs[&1], Interval { start: 0, end: 0 });
        // c def at slot 0, used by Ret at slot 1.
        assert_eq!(ivs[&2], Interval { start: 0, end: 1 });
    }
}
