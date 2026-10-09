//! The run-time test one lossy slot of a mono adapter asks of its
//! argument before the typed body may take it (`build_reroute`).

use crate::ssa::{self, BinOp, FuncId, IPred, InstKind, Operand, Type};

use super::BoxedEntryIntrinsics;
use super::fit_tag::{FitTag, scalar_tags};
use super::unbox::BoxedCoerceIntrinsics;

/// What a lossy slot's test reads besides its loaded argument.
pub(super) enum On {
    /// The argument, then these operands.
    Arg(Vec<Operand>),
    /// The argv window from the slot on, against this shape word.
    Rest(u64),
}

/// Which answers of a lossy slot's test miss.
pub(super) enum Miss {
    /// This one.
    At(i64),
    /// Every `AnySlotTag` outside this bit set (`fit_tag::scalar_tags`).
    Outside(i64),
}

/// One lossy slot's test: the kernel, what it reads, and the answers
/// that miss.
pub(super) struct Probe {
    pub(super) test: FuncId,
    pub(super) on: On,
    pub(super) miss: Miss,
    /// A struct-shaped slot that passed hands its loaded cell on to the
    /// unbox (`BoxedCoerceIntrinsics::fit`).
    pub(super) passes_cell: bool,
}

/// The test of one lossy slot. A struct-shaped slot asks its layout of
/// the cell (of every element, for an array of structs). A string slot
/// takes only a string, and an array slot only an array block whose
/// element kinds and tags match the slot's at every level (an `any[]`
/// level reads any block kind-aware); a typed rest asks that of every
/// argument it collects. ToString, a raw pointer unbox or the rest's
/// coercion would otherwise hand the body a value the caller never
/// passed, or slots it cannot read. A `number` / `boolean` / integer
/// slot misses on every tag outside the set it binds as it is
/// (`fit_tag::scalar_tags`), which ToNumber / ToBoolean would convert.
/// A struct-shaped slot's class list is addressed in `entry` of `f`.
pub(super) fn slot_probe(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    coerce: &BoxedCoerceIntrinsics<'_>,
    i: usize,
    ty: Type,
    dflt: Option<&super::DfltLit>,
) -> Probe {
    let (ids, slot) = (&coerce.ids, coerce.slots[i]);
    let rest = coerce.rest && i + 1 == coerce.slots.len();
    let probe = |test, on, miss_at| Probe {
        test,
        on,
        miss: Miss::At(miss_at),
        passes_cell: false,
    };
    if let Some(tags) = scalar_tags(ty, dflt, slot.undef_misses) {
        return Probe {
            miss: Miss::Outside(tags),
            ..probe(intr.any_unbox_tag, On::Arg(Vec::new()), 0)
        };
    }
    match (slot.fit, slot.shape) {
        (Some(FitTag { tag, depth, kin }), _) => {
            let mut on = vec![Operand::ConstI64(tag as i64)];
            on.extend((depth > 0).then_some(Operand::ConstI64(depth as i64)));
            on.push(kin_operand(f, entry, kin));
            let test = if depth == 0 {
                ids.struct_fits
            } else {
                ids.arr_fits
            };
            Probe {
                passes_cell: true,
                ..probe(test, On::Arg(on), 0)
            }
        }
        (None, Some(shape)) if rest => probe(ids.rest_fits, On::Rest(shape), 0),
        (None, Some(shape)) => probe(
            ids.arr_cell,
            On::Arg(vec![Operand::ConstI64(shape as i64)]),
            0,
        ),
        _ => probe(ids.str_fits, On::Arg(Vec::new()), 0),
    }
}

/// The `kin` operand of a fit test: the class list's address, or null
/// for a slot with none.
fn kin_operand(f: &mut ssa::Function, entry: ssa::BlockId, kin: Option<ssa::StringId>) -> Operand {
    let Some(list) = kin else {
        return Operand::ConstI64(0);
    };
    let ptr = f.append_inst(entry, InstKind::StringRef(list), Type::Ptr, None);
    let bits = f.append_inst(
        entry,
        InstKind::PtrToInt(Operand::Value(ptr)),
        Type::I64,
        None,
    );
    Operand::Value(bits)
}

/// The `Bool` that is true when the test's answer `v` misses.
pub(super) fn emit_miss(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    v: ssa::ValueId,
    miss: Miss,
) -> ssa::ValueId {
    let cmp = |f: &mut ssa::Function, x: ssa::ValueId, k: i64| {
        f.append_inst(
            entry,
            InstKind::ICmp(IPred::Eq, Operand::Value(x), Operand::ConstI64(k)),
            Type::Bool,
            None,
        )
    };
    match miss {
        Miss::At(k) => cmp(f, v, k),
        Miss::Outside(tags) => {
            let bit = f.append_inst(
                entry,
                InstKind::BinOp(BinOp::Shl, Operand::ConstI64(1), Operand::Value(v)),
                Type::I64,
                None,
            );
            let kept = f.append_inst(
                entry,
                InstKind::BinOp(BinOp::And, Operand::Value(bit), Operand::ConstI64(tags)),
                Type::I64,
                None,
            );
            cmp(f, kept, 0)
        }
    }
}
