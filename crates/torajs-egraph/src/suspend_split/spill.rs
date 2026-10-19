//! Turn every SSA value that crosses a suspension point into memory
//! the frame can carry.
//!
//! A crossing value is either recomputed where it is used (a read of
//! the frame's parameter area, which nothing writes after the wrapper,
//! or an address constant), or demoted to a stack slot: stored right
//! after its definition and loaded before each use outside the block
//! that defines it. Blocks end at suspension points, so a use in the
//! defining block never has one in between. What crosses is then only
//! stack slots, which the park copies into the frame and the landing
//! copies back (or which live in the frame outright, `rewrite`). φ
//! promotion runs after the split and lifts the demoted slots back into
//! registers on every path a suspension does not cut.

use torajs_core::ssa::id_hash::{HashMap, HashSet};
use torajs_core::ssa::{
    Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
    rewrite_value_operands, visit_value_operands,
};

/// How a crossing value travels.
enum Carry {
    /// Re-executed before each use outside its block.
    Remat(InstKind),
    /// Loaded from this slot before each use outside its block.
    Slot(ValueId, Type),
}

pub(super) fn new_value(f: &mut Function, ty: Type) -> ValueId {
    let id = ValueId(f.values.len() as u32);
    f.values.push(ValueInfo { ty, name: None });
    id
}

fn inst(result: Option<ValueId>, kind: InstKind) -> Inst {
    Inst {
        result,
        kind,
        origin: None,
    }
}

/// `frame_params` is the byte range of the frame's parameter area.
fn remat_kind(kind: &InstKind, frame: ValueId, frame_params: (u64, u64)) -> bool {
    match kind {
        InstKind::Load(_, Operand::Value(b), off) => {
            *b == frame && (frame_params.0..frame_params.1).contains(off)
        }
        InstKind::GlobalRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::StringRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_) => true,
        _ => false,
    }
}

/// Demote or rematerialize every crossing value of `crossing` that is
/// not an alloca. Answers the slot each demoted value now lives in.
pub(super) fn carry_values(
    f: &mut Function,
    crossing: &HashSet<ValueId>,
    frame: ValueId,
    frame_params: (u64, u64),
) -> HashMap<ValueId, ValueId> {
    let mut def_block: HashMap<ValueId, usize> = HashMap::default();
    let mut carry: HashMap<ValueId, Carry> = HashMap::default();
    let mut demote: Vec<(ValueId, Type)> = Vec::new();
    for (bi, b) in f.blocks.iter().enumerate() {
        for i in &b.insts {
            let Some(r) = i.result else { continue };
            if !crossing.contains(&r) {
                continue;
            }
            def_block.insert(r, bi);
            if matches!(i.kind, InstKind::Alloca(_) | InstKind::AllocaBytes(_)) {
                continue;
            }
            let ty = f.values[r.0 as usize].ty;
            assert!(
                !matches!(ty, Type::Vec(_) | Type::Inline(_)),
                "internal error: `{}` keeps {} across an await, which no frame word can hold",
                f.name,
                ty.as_str()
            );
            if remat_kind(&i.kind, frame, frame_params) {
                carry.insert(r, Carry::Remat(i.kind.clone()));
            } else {
                demote.push((r, ty));
            }
        }
    }
    let mut slots: HashMap<ValueId, ValueId> = HashMap::default();
    let mut allocas: Vec<Inst> = Vec::new();
    demote.sort_by_key(|(v, _)| v.0);
    for (v, ty) in demote {
        let slot = new_value(f, Type::Ptr);
        allocas.push(inst(Some(slot), InstKind::Alloca(ty)));
        slots.insert(v, slot);
        carry.insert(v, Carry::Slot(slot, ty));
    }
    for bi in 0..f.blocks.len() {
        rewrite_block_uses(f, bi, &carry, &def_block);
    }
    f.blocks[0].insts.splice(0..0, allocas);
    slots
}

/// Give each use in block `bi` of a carried value defined in another
/// block its own copy, made just before the use, and store each value
/// demoted to a slot right after its definition.
fn rewrite_block_uses(
    f: &mut Function,
    bi: usize,
    carry: &HashMap<ValueId, Carry>,
    def_block: &HashMap<ValueId, usize>,
) {
    let foreign = |v: ValueId| carry.contains_key(&v) && def_block.get(&v) != Some(&bi);
    let insts = std::mem::take(&mut f.blocks[bi].insts);
    let mut out = Vec::with_capacity(insts.len());
    for mut i in insts {
        let mut used: Vec<ValueId> = Vec::new();
        visit_value_operands(&i.kind, |v| {
            if foreign(v) && !used.contains(&v) {
                used.push(v)
            }
        });
        if !used.is_empty() {
            let fresh = copies(f, &used, carry, &mut out);
            rewrite_value_operands(&mut i.kind, |v| fresh.get(&v).copied().unwrap_or(v));
        }
        let demoted = match i.result.map(|r| (r, carry.get(&r))) {
            Some((r, Some(Carry::Slot(slot, _)))) => Some((r, *slot)),
            _ => None,
        };
        out.push(i);
        if let Some((r, slot)) = demoted {
            let kind = InstKind::Store(Operand::Value(r), Operand::Value(slot), 0);
            out.push(inst(None, kind));
        }
    }
    let term_use = match &f.blocks[bi].term {
        Terminator::Ret(Some(Operand::Value(v)))
        | Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        } if foreign(*v) => Some(*v),
        _ => None,
    };
    if let Some(v) = term_use {
        let fresh = copies(f, &[v], carry, &mut out);
        let nv = Operand::Value(fresh[&v]);
        match &mut f.blocks[bi].term {
            Terminator::Ret(Some(op)) | Terminator::CondBr { cond: op, .. } => *op = nv,
            _ => unreachable!(),
        }
    }
    f.blocks[bi].insts = out;
}

fn copies(
    f: &mut Function,
    used: &[ValueId],
    carry: &HashMap<ValueId, Carry>,
    out: &mut Vec<Inst>,
) -> HashMap<ValueId, ValueId> {
    let mut fresh = HashMap::default();
    for &v in used {
        let ty = f.values[v.0 as usize].ty;
        let nv = new_value(f, ty);
        let kind = match &carry[&v] {
            Carry::Remat(k) => k.clone(),
            Carry::Slot(slot, ty) => InstKind::Load(*ty, Operand::Value(*slot), 0),
        };
        out.push(inst(Some(nv), kind));
        fresh.insert(v, nv);
    }
    fresh
}
