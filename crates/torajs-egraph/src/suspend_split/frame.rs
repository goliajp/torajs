//! The frame's slots: which stack slots cross a suspension point, and
//! where each one lives in the frame.
//!
//! A slot read and written only whole, at offset 0, is a value cell:
//! it stays on the stack, the park copies it into its frame word and
//! the landing copies it back (and φ promotion can still lift it into
//! a register afterwards). Any other slot that crosses (a byte buffer
//! read in parts) lives in the frame outright: every use of its address
//! is rewritten to the frame word's. So does every slot whose address
//! escapes (stored as a value, handed to a call, turned into an
//! integer), crossing or not: the address may come back through memory
//! after a suspension, where a stack address would point into a frame
//! that is gone.

use torajs_core::ssa::id_hash::{HashMap, HashSet};
use torajs_core::ssa::{
    BinOp, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, rewrite_value_operands,
    visit_value_operands,
};

use super::spill::new_value;

pub(super) struct Layout {
    /// value cells: alloca → (frame offset, cell type)
    pub(super) copied: HashMap<ValueId, (u64, Type)>,
    /// allocas moved into the frame: alloca → frame offset
    pub(super) moved: HashMap<ValueId, u64>,
    /// the frame's byte size
    pub(super) size: u64,
}

/// The allocas among `crossing`, and those whose address escapes, laid
/// out from `base`.
pub(super) fn lay_out(f: &Function, crossing: &HashSet<ValueId>, base: u64) -> Layout {
    let (whole, escaped) = alloca_uses(f);
    let mut cells: Vec<(ValueId, InstKind)> = Vec::new();
    for b in &f.blocks {
        for i in &b.insts {
            if let (Some(r), InstKind::Alloca(_) | InstKind::AllocaBytes(_)) = (i.result, &i.kind)
                && (crossing.contains(&r) || escaped.contains(&r))
            {
                cells.push((r, i.kind.clone()));
            }
        }
    }
    cells.sort_by_key(|(r, _)| r.0);
    let mut layout = Layout {
        copied: HashMap::default(),
        moved: HashMap::default(),
        size: base,
    };
    for (r, kind) in cells {
        let off = layout.size;
        match kind {
            InstKind::Alloca(ty) if whole.contains(&r) && !escaped.contains(&r) => {
                layout.copied.insert(r, (off, ty));
                layout.size += 8;
            }
            InstKind::Alloca(_) => {
                layout.moved.insert(r, off);
                layout.size += 8;
            }
            InstKind::AllocaBytes(n) => {
                layout.moved.insert(r, off);
                layout.size += n.div_ceil(8) * 8;
            }
            _ => unreachable!(),
        }
    }
    layout
}

/// Per alloca: is every use a whole load or store at offset 0 through
/// the address (`whole`), and is the address ever used as a value
/// (`escaped`)? A part access through the address is neither.
fn alloca_uses(f: &Function) -> (HashSet<ValueId>, HashSet<ValueId>) {
    let mut allocas: HashSet<ValueId> = HashSet::default();
    let mut parts: HashSet<ValueId> = HashSet::default();
    let mut escaped: HashSet<ValueId> = HashSet::default();
    for b in &f.blocks {
        for i in &b.insts {
            match &i.kind {
                InstKind::Alloca(_) | InstKind::AllocaBytes(_) => {
                    allocas.insert(i.result.unwrap());
                }
                InstKind::Load(_, Operand::Value(p), off) => {
                    if *off != 0 {
                        parts.insert(*p);
                    }
                }
                InstKind::Store(v, Operand::Value(p), off) => {
                    if *off != 0 {
                        parts.insert(*p);
                    }
                    if let Operand::Value(v) = v {
                        escaped.insert(*v);
                    }
                }
                k => visit_value_operands(k, |v| {
                    escaped.insert(v);
                }),
            }
        }
        if let Terminator::Ret(Some(Operand::Value(v)))
        | Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        } = &b.term
        {
            escaped.insert(*v);
        }
    }
    escaped.retain(|v| allocas.contains(v));
    let whole = (allocas.iter())
        .filter(|v| !parts.contains(v) && !escaped.contains(v))
        .copied()
        .collect();
    (whole, escaped)
}

/// Point every use of a moved alloca at its frame word, and drop the
/// alloca.
pub(super) fn move_into_frame(f: &mut Function, moved: &HashMap<ValueId, u64>, frame: ValueId) {
    if moved.is_empty() {
        return;
    }
    for bi in 0..f.blocks.len() {
        let insts = std::mem::take(&mut f.blocks[bi].insts);
        let mut out = Vec::with_capacity(insts.len());
        for mut i in insts {
            if i.result.is_some_and(|r| moved.contains_key(&r)) {
                continue;
            }
            match &mut i.kind {
                InstKind::Load(_, Operand::Value(p), k)
                | InstKind::Store(_, Operand::Value(p), k)
                    if moved.contains_key(p) =>
                {
                    *k += moved[p];
                    *p = frame;
                }
                _ => {}
            }
            let mut addr: HashMap<ValueId, ValueId> = HashMap::default();
            let mut need: Vec<ValueId> = Vec::new();
            visit_value_operands(&i.kind, |v| {
                if moved.contains_key(&v) && !need.contains(&v) {
                    need.push(v)
                }
            });
            for v in need {
                let a = frame_addr(f, frame, moved[&v], &mut out);
                addr.insert(v, a);
            }
            rewrite_value_operands(&mut i.kind, |v| addr.get(&v).copied().unwrap_or(v));
            out.push(i);
        }
        f.blocks[bi].insts = out;
    }
}

/// `frame + off` as a pointer value.
fn frame_addr(f: &mut Function, frame: ValueId, off: u64, out: &mut Vec<Inst>) -> ValueId {
    let int = new_value(f, Type::I64);
    let sum = new_value(f, Type::I64);
    let ptr = new_value(f, Type::Ptr);
    let mk = |result, kind| Inst {
        result: Some(result),
        kind,
        origin: None,
    };
    out.push(mk(int, InstKind::PtrToInt(Operand::Value(frame))));
    out.push(mk(
        sum,
        InstKind::BinOp(
            BinOp::Add,
            Operand::Value(int),
            Operand::ConstI64(off as i64),
        ),
    ));
    out.push(mk(ptr, InstKind::IntToPtr(Operand::Value(sum))));
    ptr
}
