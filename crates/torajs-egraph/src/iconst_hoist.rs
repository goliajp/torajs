//! Wide integer-constant loop hoisting — the integer twin of
//! [`crate::fconst_hoist`], from the rotation 623 fill-loop reading.
//!
//! `icmp x, #imm` encodes only 0..4096 as the CMP-immediate alias;
//! any other `ConstI64` operand of a compare is rematerialized at
//! every use (compile/cmp.rs — a `movz` / `movk` pair for a
//! 10,000,000 bound). The pre-reserved fill loop's 12-instruction body
//! spends 2 of them rebuilding its trip count on every iteration,
//! and every counted loop over a wide literal bound pays the same.
//!
//! This pass gives each such constant a value: for every natural
//! loop, each distinct non-encodable `ConstI64` used by a loop-body
//! `ICmp` is minted once as `Copy(I64, ConstI64(c))` at the tail of
//! the loop header's immediate dominator, and the compares rewrite to
//! the minted value. Register allocation keeps it resident across the
//! loop; a spill decision degrades to a 1-inst `ldr`, still cheaper
//! than the pair. Loops whose header has no strict dominator outside
//! the body are skipped, as in the f64 twin.
//!
//! Multiplies, divides and remainders get the same treatment late in
//! the pipeline ([`hoist_binop_consts`]): codegen has no immediate
//! form for them (compile/binop.rs), so a constant operand other than
//! a multiplier with a shift-and-add or power-of-two form is rebuilt
//! on every use (the digit loop's `v * 10` paid `mov x, #10` every
//! iteration). Late, inside block layout on the analyses it builds,
//! because the passes before read these constants to recognise their
//! shapes. Adds, subtracts, shifts and bitwise ops
//! keep their constants: whole-program judges read them (the boxing
//! offset added to a double is how a value is proven a number, and
//! hiding it pulled the dynamic runtime into the link). Blocks that
//! call out are skipped: they are a loop's cold arms (an array's grow
//! path), and a constant held for them takes a register from the hot
//! path. A loop that calls out on every iteration is skipped whole,
//! and each constant goes to its innermost loop: both keep a held
//! constant from pushing the loop's own values to the stack.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BinOp, Block, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId, ValueInfo,
};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IconstHoistStats {
    /// Distinct (loop, constant) pairs minted as a preheader Copy.
    pub consts_hoisted: u32,
    /// ConstI64 operands rewritten to a minted value.
    pub uses_rewritten: u32,
    /// Functions the late pass looked at.
    pub functions_analysed: u32,
}

/// The CMP-immediate alias covers exactly this range (compile/cmp.rs).
fn cmp_imm_encodes(c: i64) -> bool {
    (0..4096).contains(&c)
}

pub fn hoist_int_consts(module: &mut Module) -> IconstHoistStats {
    let mut stats = IconstHoistStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let dom = DominatorTree::compute(func);
        let loops = LoopAnalysis::compute(func, &dom);
        hoist_fn(func, &dom, &loops, &mut stats, rewrite_cmp, false);
    }
    stats
}

pub fn hoist_binop_consts(module: &mut Module) -> IconstHoistStats {
    let mut stats = IconstHoistStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() || !has_candidate(func) {
            continue;
        }
        let dom = DominatorTree::compute(func);
        let loops = LoopAnalysis::compute(func, &dom);
        hoist_binop_consts_in(func, &dom, &loops, &mut stats);
    }
    stats
}

/// Block layout's call: a loop's multiply and divide constants into its
/// preheader, on the analyses layout already built (the copies leave the
/// control flow alone). `TORAJS_BINOP_CONST_HOIST_OFF=1` clears `on`.
pub(crate) fn hoist_in_layout(
    on: bool,
    func: &mut Function,
    dom: &DominatorTree,
    la: &LoopAnalysis,
    stats: &mut IconstHoistStats,
) {
    if on && has_candidate(func) {
        hoist_binop_consts_in(func, dom, la, stats);
    }
}

/// Some block that does not call out holds a constant the late pass
/// would take.
pub(crate) fn has_candidate(func: &Function) -> bool {
    func.blocks
        .iter()
        .any(|b| !calls(b) && b.insts.iter().any(|i| has_paid_const(&i.kind)))
}

/// The late pass on one function, with the dominator tree and loops
/// its caller already holds: block layout runs it on the analyses it
/// builds anyway, so the pass costs no analysis of its own. The copies
/// it adds leave the control flow, and so both analyses, unchanged.
pub(crate) fn hoist_binop_consts_in(
    func: &mut Function,
    dom: &DominatorTree,
    loops: &LoopAnalysis,
    stats: &mut IconstHoistStats,
) {
    stats.functions_analysed += 1;
    hoist_fn(func, dom, loops, stats, rewrite_binop, true);
}

type Rewrite = fn(&mut InstKind, &mut dyn FnMut(i64) -> Operand) -> u32;

/// A block that calls out (or traps): the cold arm of a loop, such as
/// an array's grow path, whose constants are not worth a register held
/// across the whole loop.
fn calls(block: &Block) -> bool {
    matches!(block.term, Terminator::Unreachable)
        || block.insts.iter().any(|i| {
            matches!(
                i.kind,
                InstKind::Call(..) | InstKind::CallIndirect(..) | InstKind::RcDecAny(_)
            )
        })
}

/// Where the minted copies go in the preheader: its end, or, for the
/// late pass, before a last compare its branch reads.
fn mint_at(pre: &Block, late: bool) -> usize {
    let n = pre.insts.len();
    let feeds_branch = matches!(
        (&pre.term, pre.insts.last()),
        (Terminator::CondBr { cond: Operand::Value(c), .. }, Some(last)) if last.result == Some(*c)
    );
    if late && feeds_branch { n - 1 } else { n }
}

fn hoist_fn(
    func: &mut Function,
    dom: &DominatorTree,
    loops: &LoopAnalysis,
    stats: &mut IconstHoistStats,
    rewrite: Rewrite,
    hot_only: bool,
) {
    let n = func.blocks.len();
    let mut order = loops.loops().to_vec();
    // late, a constant goes to its innermost loop only: held across an
    // outer loop it takes a register from every loop inside, and the
    // outer loops then find the inner uses already rewritten
    if hot_only {
        order.sort_by_key(|lp| lp.body.len());
    }
    for lp in order {
        let Some(pre) = dom.immediate_dominator(lp.header) else {
            continue;
        };
        if pre == lp.header || lp.contains(&pre) || pre.0 as usize >= n {
            continue;
        }
        let mut seen: Vec<i64> = Vec::new();
        for &b in &lp.body {
            let bi = b.0 as usize;
            if bi >= n || hot_only && calls(&func.blocks[bi]) {
                continue;
            }
            for inst in &func.blocks[bi].insts {
                if hot_only && !has_paid_const(&inst.kind) {
                    continue;
                }
                let mut kind = inst.kind.clone();
                rewrite(&mut kind, &mut |c| {
                    if !seen.contains(&c) {
                        seen.push(c);
                    }
                    Operand::ConstI64(c)
                });
            }
        }
        if seen.is_empty() {
            continue;
        }
        // a call every iteration makes leaves only the callee-saved
        // registers to hold values across it: a constant held there
        // pushes the loop's own values to the stack
        let calls_every_trip = || {
            lp.body.iter().any(|&b| {
                (b.0 as usize) < n
                    && calls(&func.blocks[b.0 as usize])
                    && lp.back_edge_sources.iter().all(|&l| dom.dominates(b, l))
            })
        };
        if hot_only && calls_every_trip() {
            continue;
        }
        let mut minted: HashMap<i64, ValueId> = HashMap::default();
        // late, the preheader's compare already sits right before the
        // branch it feeds, which codegen fuses only while nothing
        // separates them: the copies go in front of it
        let first = mint_at(&func.blocks[pre.0 as usize], hot_only);
        for (at, c) in (first..).zip(&seen) {
            let vid = ValueId(func.values.len() as u32);
            func.values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("iconst_{}", vid.0)),
            });
            func.blocks[pre.0 as usize].insts.insert(
                at,
                Inst {
                    result: Some(vid),
                    kind: InstKind::Copy(Type::I64, Operand::ConstI64(*c)),
                    origin: None,
                },
            );
            minted.insert(*c, vid);
            stats.consts_hoisted += 1;
        }
        for &b in &lp.body {
            let bi = b.0 as usize;
            if bi >= n || hot_only && calls(&func.blocks[bi]) {
                continue;
            }
            for inst in &mut func.blocks[bi].insts {
                stats.uses_rewritten +=
                    rewrite(&mut inst.kind, &mut |c| Operand::Value(minted[&c]));
            }
        }
    }
}

/// Rewrite every non-encodable `ConstI64` operand of an `ICmp`
/// through `lookup`; returns how many operands were visited.
fn rewrite_cmp(kind: &mut InstKind, lookup: &mut dyn FnMut(i64) -> Operand) -> u32 {
    let mut hits = 0;
    if let InstKind::ICmp(_, a, b) = kind {
        for op in [a, b] {
            if let Operand::ConstI64(c) = op
                && !cmp_imm_encodes(*c)
            {
                *op = lookup(*c);
                hits += 1;
            }
        }
    }
    hits
}

/// Which constant operands of a multiply, divide or remainder codegen
/// materializes into a register per use: (left, right).
fn materialized(op: &BinOp, a: &Operand, b: &Operand) -> (bool, bool) {
    match op {
        BinOp::Mul => {
            let free = |o: &Operand| {
                matches!(o, Operand::ConstI64(3 | 5 | 9))
                    || matches!(o, Operand::ConstI64(c) if *c > 0 && (*c as u64).is_power_of_two())
            };
            let paid = !free(a) && !free(b);
            (paid, paid)
        }
        BinOp::SDiv | BinOp::SRem => (true, true),
        _ => (false, false),
    }
}

fn has_paid_const(kind: &InstKind) -> bool {
    let InstKind::BinOp(op, a, b) = kind else {
        return false;
    };
    let (l, r) = materialized(op, a, b);
    (l && matches!(a, Operand::ConstI64(_))) || (r && matches!(b, Operand::ConstI64(_)))
}

/// Rewrite every `ConstI64` operand of an integer `BinOp` that codegen
/// would materialize into a register per use; returns how many.
fn rewrite_binop(kind: &mut InstKind, lookup: &mut dyn FnMut(i64) -> Operand) -> u32 {
    let InstKind::BinOp(op, a, b) = kind else {
        return 0;
    };
    let (lhs, rhs) = materialized(op, a, b);
    let mut hits = 0;
    for (o, wanted) in [(a, lhs), (b, rhs)] {
        if let (Operand::ConstI64(c), true) = (&*o, wanted) {
            *o = lookup(*c);
            hits += 1;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BlockId, IPred};

    fn vi(ty: Type) -> ValueInfo {
        ValueInfo { ty, name: None }
    }
    fn mk(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }
    /// b0(entry) → b1(header: icmp %v0 < 10_000_000, CondBr b2/b3) →
    /// b2(body: %2 = add %v0 1, Br b1) / b3(exit).
    fn counted_loop(bound: i64) -> Module {
        Module {
            funcs: vec![Function {
                name: "f".into(),
                params: vec![],
                ret: Type::I64,
                values: vec![vi(Type::I64), vi(Type::Bool), vi(Type::I64)],
                blocks: vec![
                    Block {
                        id: BlockId(0),
                        insts: vec![],
                        term: Terminator::Br(BlockId(1)),
                    },
                    Block {
                        id: BlockId(1),
                        insts: vec![mk(
                            1,
                            InstKind::ICmp(
                                IPred::Slt,
                                Operand::Value(ValueId(0)),
                                Operand::ConstI64(bound),
                            ),
                        )],
                        term: Terminator::CondBr {
                            cond: Operand::Value(ValueId(1)),
                            then_blk: BlockId(2),
                            else_blk: BlockId(3),
                        },
                    },
                    Block {
                        id: BlockId(2),
                        insts: vec![mk(
                            2,
                            InstKind::BinOp(
                                BinOp::Add,
                                Operand::Value(ValueId(0)),
                                Operand::ConstI64(1),
                            ),
                        )],
                        term: Terminator::Br(BlockId(1)),
                    },
                    Block {
                        id: BlockId(3),
                        insts: vec![],
                        term: Terminator::Ret(None),
                    },
                ],
                current_origin: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_wide_loop_bound_hoists_to_the_dominator() {
        let mut module = counted_loop(10_000_000);
        let stats = hoist_int_consts(&mut module);
        assert_eq!(stats.consts_hoisted, 1);
        assert_eq!(stats.uses_rewritten, 1);
        let f = &module.funcs[0];
        assert_eq!(f.blocks[0].insts.len(), 1);
        let minted = f.blocks[0].insts[0].result.unwrap();
        assert!(matches!(
            f.blocks[0].insts[0].kind,
            InstKind::Copy(Type::I64, Operand::ConstI64(10_000_000))
        ));
        assert!(matches!(
            f.blocks[1].insts[0].kind,
            InstKind::ICmp(_, _, Operand::Value(v)) if v == minted
        ));
        // The body's `add 1` keeps its encodable immediate.
        assert!(matches!(
            f.blocks[2].insts[0].kind,
            InstKind::BinOp(BinOp::Add, _, Operand::ConstI64(1))
        ));
    }

    #[test]
    fn an_encodable_bound_stays_inline() {
        let mut module = counted_loop(100);
        let stats = hoist_int_consts(&mut module);
        assert_eq!(stats.consts_hoisted, 0);
        assert!(matches!(
            module.funcs[0].blocks[1].insts[0].kind,
            InstKind::ICmp(_, _, Operand::ConstI64(100))
        ));
    }

    /// The body's `%2 = mul %0, 10` in place of the counted loop's add.
    fn mul_loop(k: i64) -> Module {
        let mut module = counted_loop(100);
        module.funcs[0].blocks[2].insts[0].kind =
            InstKind::BinOp(BinOp::Mul, Operand::Value(ValueId(0)), Operand::ConstI64(k));
        module
    }

    #[test]
    fn a_multiplier_codegen_cannot_encode_hoists() {
        let mut module = mul_loop(10);
        let stats = hoist_binop_consts(&mut module);
        assert_eq!(stats.consts_hoisted, 1);
        let f = &module.funcs[0];
        let minted = f.blocks[0].insts[0].result.unwrap();
        assert!(matches!(
            f.blocks[2].insts[0].kind,
            InstKind::BinOp(BinOp::Mul, _, Operand::Value(v)) if v == minted
        ));
        // the compare's encodable bound is not this pass's
        assert!(matches!(
            f.blocks[1].insts[0].kind,
            InstKind::ICmp(_, _, Operand::ConstI64(100))
        ));
    }

    #[test]
    fn shift_and_add_and_power_of_two_multipliers_stay_inline() {
        for k in [3, 5, 9, 8] {
            let mut module = mul_loop(k);
            assert_eq!(hoist_binop_consts(&mut module).consts_hoisted, 0, "{k}");
        }
    }

    #[test]
    fn an_add_keeps_even_a_wide_constant() {
        let mut module = counted_loop(100);
        module.funcs[0].blocks[2].insts[0].kind = InstKind::BinOp(
            BinOp::Add,
            Operand::Value(ValueId(0)),
            Operand::ConstI64(100_000),
        );
        assert_eq!(hoist_binop_consts(&mut module).consts_hoisted, 0);
    }
}
