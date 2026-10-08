//! Which loops `reread` takes for an array, and which header words it
//! keeps in cells there (see the parent module doc).

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, Function, HEADER_FLAGS_COLLECTOR, InstKind, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use super::super::{ARR_CAP_HEAD_OFF, ARR_DATA_PTR_OFF, ARR_LEN_OFF, HEADER_READ_ONLY_CALLS};
use crate::interval::elem::ArrayClasses;
use crate::interval::elem_header::HEADER_WRITE_CALLS;

pub(super) const ARR_WORD_OFF: u64 = 0;

/// What one cell holds: the header word at `off` read as `ty`, or for
/// the flags word, that word under one flag `mask`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Word {
    pub(super) off: u64,
    pub(super) ty: Type,
    pub(super) mask: Option<i64>,
}

pub(super) fn names_array(op: &Operand, a: ValueId, classes: &ArrayClasses) -> bool {
    match op {
        Operand::Value(v) => classes.class.get(v) == Some(&a) || classes.base.get(v) == Some(&a),
        _ => false,
    }
}

pub(super) fn header_kernel(
    kind: &InstKind,
    a: ValueId,
    classes: &ArrayClasses,
    names: &[String],
) -> bool {
    matches!(kind, InstKind::Call(f, args)
        if HEADER_WRITE_CALLS.contains(&callee(names, f.0))
            && args.iter().any(|x| names_array(x, a, classes)))
}

fn callee(names: &[String], f: u32) -> &str {
    names.get(f as usize).map(String::as_str).unwrap_or("")
}

/// The header word `kind` loads from A, as (offset, type).
pub(super) fn word_load(
    kind: &InstKind,
    a: ValueId,
    classes: &ArrayClasses,
) -> Option<(u64, Type)> {
    match kind {
        InstKind::Load(ty, Operand::Value(p), off)
            if classes.class.get(p) == Some(&a)
                && matches!(
                    *off,
                    ARR_WORD_OFF | ARR_LEN_OFF | ARR_CAP_HEAD_OFF | ARR_DATA_PTR_OFF
                ) =>
        {
            Some((*off, *ty))
        }
        _ => None,
    }
}

/// The cells A gets in the loop made of `blocks` (`body` as a set), or
/// `None` when the loop is not this pass's (see the module doc).
pub(super) fn plan(
    func: &Function,
    blocks: &[BlockId],
    body: &HashSet<BlockId>,
    a: ValueId,
    classes: &ArrayClasses,
    names: &[String],
    masks: &HashMap<ValueId, Vec<(BlockId, i64)>>,
) -> Option<Vec<Word>> {
    let mut kernels = 0;
    let mut stored: HashSet<u64> = HashSet::default();
    let mut cells: Vec<Word> = Vec::new();
    let mut unmasked_word = false;
    for &b in blocks {
        for inst in &func.blocks[b.0 as usize].insts {
            match &inst.kind {
                k if header_kernel(k, a, classes, names) => kernels += 1,
                InstKind::Call(f, args) if args.iter().any(|x| names_array(x, a, classes)) => {
                    if !HEADER_READ_ONLY_CALLS.contains(&callee(names, f.0)) {
                        return None;
                    }
                }
                InstKind::CallIndirect(_, c, args)
                    if names_array(c, a, classes)
                        || args.iter().any(|x| names_array(x, a, classes)) =>
                {
                    return None;
                }
                InstKind::RcDecAny(v) if names_array(v, a, classes) => return None,
                // a narrower store (the flags half-word) dirties the
                // whole word it lands in
                InstKind::Store(_, Operand::Value(p), off) if classes.class.get(p) == Some(&a) => {
                    stored.insert(off & !7);
                }
                InstKind::StoreDyn(_, Operand::Value(p), _)
                | InstKind::StoreDynScaled8(_, Operand::Value(p), _)
                    if classes.class.get(p) == Some(&a) =>
                {
                    return None;
                }
                k => {
                    let Some((off, ty)) = word_load(k, a, classes) else {
                        continue;
                    };
                    let mut add = |w: Word| {
                        if !cells.contains(&w) {
                            cells.push(w);
                        }
                    };
                    if off != ARR_WORD_OFF {
                        add(Word {
                            off,
                            ty,
                            mask: None,
                        });
                        continue;
                    }
                    // the flags word, only as masked flag tests in the loop
                    match inst.result.and_then(|r| masks.get(&r)) {
                        Some(uses)
                            if ty == Type::I64 && uses.iter().all(|(u, _)| body.contains(u)) =>
                        {
                            for &(_, k) in uses {
                                add(Word {
                                    off,
                                    ty,
                                    mask: Some(k),
                                });
                            }
                        }
                        _ => unmasked_word = true,
                    }
                }
            }
        }
    }
    cells.retain(|w| !stored.contains(&w.off) && !(w.off == ARR_WORD_OFF && unmasked_word));
    (kernels > 0 && !cells.is_empty()).then_some(cells)
}

/// Values every use of which masks them to flag bits the cycle
/// collector never flips (`and v, K << 48`), with each use's block and
/// mask.
pub(super) fn flag_masks(func: &Function) -> HashMap<ValueId, Vec<(BlockId, i64)>> {
    let flags_only =
        |k: u64| k & 0xffff_ffff_ffff == 0 && (k >> 48) & u64::from(HEADER_FLAGS_COLLECTOR) == 0;
    let mut ok: HashMap<ValueId, Vec<(BlockId, i64)>> = HashMap::default();
    let mut bad: HashSet<ValueId> = HashSet::default();
    for (bi, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            match &inst.kind {
                InstKind::BinOp(BinOp::And, Operand::Value(v), Operand::ConstI64(k))
                    if flags_only(*k as u64) =>
                {
                    ok.entry(*v).or_default().push((BlockId(bi as u32), *k));
                }
                k => visit_value_operands(k, |v| {
                    bad.insert(v);
                }),
            }
        }
        match &block.term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => {
                bad.insert(*v);
            }
            _ => {}
        }
    }
    ok.retain(|v, _| !bad.contains(v));
    ok
}
