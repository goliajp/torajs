//! A string's length and encoding bit, read once in front of a loop
//! whose null and `undefined` cell tests of the string already sit there
//! (`loop_guard.rs`).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::loop_analysis::NaturalLoop;
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
    rewrite_value_operands, visit_value_operands,
};

/// `IS_LATIN1` in a string's packed header word (flags u16 at +6, bit 1).
pub(super) const STR_IS_LATIN1: i64 = 0x0002 << 48;
/// Byte offset of a string's length word (an `I32`).
pub(super) const STR_LEN_OFF: u64 = 8;

/// Move the loop's reads of `v`'s length and encoding bit into `pre`;
/// answers how many instructions left the loop.
pub(super) fn hoist_header_reads(
    func: &mut Function,
    lp: &NaturalLoop,
    pre: BlockId,
    v: ValueId,
    multi: &HashSet<ValueId>,
) -> u32 {
    if func.value_type(v) != Type::Str || multi.contains(&v) {
        return 0;
    }
    let body: Vec<usize> = lp.body.iter().map(|b| b.0 as usize).collect();
    let body_insts = || body.iter().flat_map(|&b| &func.blocks[b].insts);
    // the loop only reads through `v`: nothing in it is handed the
    // string, stores through it or releases it
    let reads_only = body_insts().all(|inst| {
        let mut names = false;
        visit_value_operands(&inst.kind, |o| names |= o == v);
        !names
            || matches!(
                &inst.kind,
                InstKind::Load(_, Operand::Value(p), _)
                | InstKind::LoadU8Dyn(Operand::Value(p), _)
                | InstKind::LoadU16Dyn(Operand::Value(p), _) if *p == v
            )
            || matches!(inst.kind, InstKind::ICmp(..) | InstKind::RcInc(_))
    });
    if !reads_only || body_insts().any(|i| i.result == Some(v)) {
        return 0;
    }
    let mut uses: HashMap<ValueId, u32> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |o| *uses.entry(o).or_insert(0) += 1);
        }
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(c))) = &block.term
        {
            *uses.entry(*c).or_insert(0) += 1;
        }
    }
    let single = |x: ValueId| uses.get(&x) == Some(&1);

    // what each moved or folded instruction's result is from now on
    let mut same: HashMap<ValueId, ValueId> = HashMap::new();
    let (mut len, mut wide_len, mut latin1): (Option<ValueId>, Option<ValueId>, Option<ValueId>) =
        (None, None, None);
    let mut out: Vec<Inst> = Vec::new();
    let mut gone = 0u32;
    for &b in &body {
        let insts = std::mem::take(&mut func.blocks[b].insts);
        let mut keep: Vec<Inst> = Vec::with_capacity(insts.len());
        let mut i = 0;
        while i < insts.len() {
            let inst = &insts[i];
            let r = inst.result;
            let slot = match &inst.kind {
                InstKind::Load(Type::I32, Operand::Value(p), STR_LEN_OFF) if *p == v => {
                    Some((&mut len, 1))
                }
                InstKind::ZExtI32ToI64(Operand::Value(x))
                    if len.is_some() && same.get(x).or(Some(x)) == len.as_ref() =>
                {
                    Some((&mut wide_len, 1))
                }
                InstKind::Load(Type::I64, Operand::Value(p), 0)
                    if *p == v && is_latin1_test(&insts[i..], &single) =>
                {
                    Some((&mut latin1, 3))
                }
                _ => None,
            };
            match (slot, r) {
                (Some((first, n)), Some(_)) => {
                    let last = insts[i + n - 1].result.expect("a value");
                    match *first {
                        Some(kept) => {
                            same.insert(last, kept);
                        }
                        None => {
                            *first = Some(last);
                            out.extend(insts[i..i + n].iter().cloned());
                        }
                    }
                    gone += n as u32;
                    i += n;
                }
                _ => {
                    keep.push(inst.clone());
                    i += 1;
                }
            }
        }
        func.blocks[b].insts = keep;
    }
    if gone == 0 {
        return 0;
    }
    func.blocks[pre.0 as usize].insts.extend(out);
    if !same.is_empty() {
        let to = |x: ValueId| same.get(&x).copied().unwrap_or(x);
        for block in func.blocks.iter_mut() {
            for inst in block.insts.iter_mut() {
                rewrite_value_operands(&mut inst.kind, to);
            }
            if let Terminator::CondBr {
                cond: Operand::Value(c),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(c))) = &mut block.term
            {
                *c = to(*c);
            }
        }
    }
    gone
}

/// `insts` opens with `t = load i64 v+0; m = and t, IS_LATIN1;
/// c = icmp ne m, 0`, each the only reader of the one before.
fn is_latin1_test(insts: &[Inst], single: &dyn Fn(ValueId) -> bool) -> bool {
    let [load, and, cmp, ..] = insts else {
        return false;
    };
    let (Some(t), Some(m)) = (load.result, and.result) else {
        return false;
    };
    single(t)
        && single(m)
        && cmp.result.is_some()
        && matches!(&and.kind,
            InstKind::BinOp(BinOp::And, Operand::Value(a), Operand::ConstI64(STR_IS_LATIN1)) if *a == t)
        && matches!(&cmp.kind,
            InstKind::ICmp(IPred::Ne, Operand::Value(a), Operand::ConstI64(0)) if *a == m)
}
