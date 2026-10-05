//! The typed mono adapter's hand-off to its all-`any` twin when a
//! slot's argument cannot cross the typed face as it is.
//!
//! A boolean / integer slot has no in-band undefined: `any_to_bool`
//! would make the missing argument `false` and `any_to_number` +
//! `FpToSi` would make it `0`, and `typeof b` / `b === undefined` in
//! the body would then answer for a value nobody passed. A struct
//! slot reads its argument through the layout, which only a cell of
//! that layout (or one extending it) has: an array, a Proxy, a
//! getter-bearing literal, a `3` handed over through `any` means a
//! generic [[Get]] for every `p.x` (§13.3.2.1), not an offset read.
//! The twin's every parameter is `any`, so it hosts such a call
//! exactly; with every argument fitting, the typed body runs as
//! before. The test is one tag compare per lossy scalar slot and one
//! `__torajs_anyv_struct_fits` call per struct slot, ahead of every
//! unbox; a struct slot that passed hands its loaded argument on
//! (`BoxedCoerceIntrinsics::fit`) so the unbox passes the cell
//! through instead of asking the coercion kernel the same question.
//!
//! An array-of-struct slot (`Tok[]`, `Tok[][]`) is the same question
//! over every element: the body indexes the array and reads each
//! element through the element layout, which only a typed array of
//! cells of that layout (or of layouts extending it) has. An `any[]`
//! holds boxes, an `Other[]` holds cells of another layout, a string
//! or a `null` is no array at all — each means the generic [[Get]] on
//! every `toks[i].value`, or a TypeError on `toks.length`, and that
//! is the twin's reading. `__torajs_anyv_arr_fits` walks the array
//! once at the boundary; a typed call never reaches this adapter.

use std::collections::HashMap;

use crate::ast::Ast;
use crate::ssa::{self, FuncId, IPred, InstKind, Operand, Terminator, Type};

use super::BoxedEntryIntrinsics;
use super::fit_tag::{FitTag, lossy_slot};
use super::unbox::BoxedCoerceIds;
use super::unbox::BoxedCoerceIntrinsics;

/// The twin adapter a mono adapter hands a non-fitting call to.
#[derive(Clone, Copy)]
pub(super) struct TwinReroute {
    pub(super) adapter: FuncId,
    pub(super) sig: ssa::SigId,
    /// `__torajs_anyv_invoke_recv_first(env, entry, recv, argv, argc)`.
    pub(super) invoke: FuncId,
    /// The twin adapter wears the mono adapter's own `(env, argv,
    /// argc)` face (the all-`any` relay shell behind a value escape,
    /// `check_monomorph_any_widen::struct_escape`): it is called
    /// directly with the same env. A receiver-polymorphic twin takes
    /// its receiver in argv[0] and goes through `invoke`.
    pub(super) same_face: bool,
}

/// Test every lossy slot of the window at `base`; on the first
/// undefined scalar or non-fitting struct argument, call the twin
/// adapter and return its answer. Returns the block the adapter
/// continues in and, per user slot, the loaded argument of a struct
/// slot that fits (`BoxedCoerceIntrinsics::fit`).
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_twin_reroute(
    f: &mut ssa::Function,
    mut entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    coerce: &BoxedCoerceIntrinsics<'_>,
    rr: TwinReroute,
    env: ssa::ValueId,
    argv: ssa::ValueId,
    argc: ssa::ValueId,
    base: ssa::ValueId,
    user_tys: &[Type],
    dflt_lits: &[Option<super::DfltLit>],
    recv_slot: bool,
    feeds_env: bool,
) -> (ssa::BlockId, Vec<Option<ssa::ValueId>>) {
    let mut fit = vec![None; user_tys.len()];
    let lossy: Vec<usize> = (0..user_tys.len())
        .filter(|&i| {
            lossy_slot(
                user_tys[i],
                dflt_lits.get(i).and_then(|d| d.as_ref()),
                coerce.slots[i],
                coerce.rest && i + 1 == user_tys.len(),
            )
        })
        .collect();
    if lossy.is_empty() {
        return (entry, fit);
    }
    let reroute = f.add_block();
    for i in lossy {
        let rest = coerce.rest && i + 1 == user_tys.len();
        let probe = slot_probe(&coerce.ids, intr, coerce.slots[i], user_tys[i], rest);
        let args = match probe.on {
            On::Arg(extra) => {
                let av = f.append_inst(
                    entry,
                    InstKind::Load(Type::Any, Operand::Value(base), (i as u64) * 8),
                    Type::Any,
                    None,
                );
                if probe.passes_cell {
                    fit[i] = Some(av);
                }
                std::iter::once(Operand::Value(av)).chain(extra).collect()
            }
            // the rest collects `argv[i..argc]` (`build_rest`), and an
            // empty window has no slot `i` to load
            On::Rest(shape) => vec![
                Operand::Value(argv),
                Operand::ConstI64(i as i64),
                Operand::Value(argc),
                Operand::ConstI64(shape as i64),
            ],
        };
        let v = f.append_inst(entry, InstKind::Call(probe.test, args), Type::I64, None);
        let miss = f.append_inst(
            entry,
            InstKind::ICmp(
                IPred::Eq,
                Operand::Value(v),
                Operand::ConstI64(probe.miss_at),
            ),
            Type::Bool,
            None,
        );
        let next = f.add_block();
        f.set_term(
            entry,
            Terminator::CondBr {
                cond: Operand::Value(miss),
                then_blk: reroute,
                else_blk: next,
            },
        );
        entry = next;
    }
    // A recv-slot mono already carries its receiver in argv[0], the
    // twin adapter's own shape, and a same-face twin takes the mono's
    // env as it is; otherwise the receiver is the env pointer of an
    // instance body (boxed as a heap cell, null on the this-free
    // path) or nothing at all for a static body.
    let answer = if recv_slot || rr.same_face {
        f.append_inst(
            reroute,
            InstKind::Call(
                rr.adapter,
                vec![
                    Operand::Value(env),
                    Operand::Value(argv),
                    Operand::Value(argc),
                ],
            ),
            Type::Any,
            None,
        )
    } else {
        let recv = if feeds_env {
            let bits = f.append_inst(
                reroute,
                InstKind::PtrToInt(Operand::Value(env)),
                Type::I64,
                None,
            );
            f.append_inst(
                reroute,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(4), Operand::Value(bits)],
                ),
                Type::Any,
                None,
            )
        } else {
            f.append_inst(
                reroute,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            )
        };
        let addr = f.append_inst(
            reroute,
            InstKind::BoxedEntryAddr(rr.adapter),
            Type::FnSig(rr.sig),
            None,
        );
        let addr_bits = f.append_inst(
            reroute,
            InstKind::PtrToInt(Operand::Value(addr)),
            Type::I64,
            None,
        );
        f.append_inst(
            reroute,
            InstKind::Call(
                rr.invoke,
                vec![
                    Operand::Value(env),
                    Operand::Value(addr_bits),
                    Operand::Value(recv),
                    Operand::Value(argv),
                    Operand::Value(argc),
                ],
            ),
            Type::Any,
            None,
        )
    };
    f.set_term(reroute, Terminator::Ret(Some(Operand::Value(answer))));
    (entry, fit)
}

/// What a lossy slot's test reads besides its loaded argument.
enum On {
    /// The argument, then these operands.
    Arg(Vec<Operand>),
    /// The argv window from the slot on, against this shape word.
    Rest(u64),
}

/// One lossy slot's test: the kernel, what it reads, and the answer
/// that misses.
struct Probe {
    test: FuncId,
    on: On,
    miss_at: i64,
    /// A struct-shaped slot that passed hands its loaded cell on to the
    /// unbox (`BoxedCoerceIntrinsics::fit`).
    passes_cell: bool,
}

/// The test of one lossy slot. A struct-shaped slot asks its layout of
/// the cell (of every element, for an array of structs). A string slot
/// takes only a string, and an array slot only an array block whose
/// element kinds and tags match the slot's at every level (an `any[]`
/// level reads any block kind-aware); a typed rest asks that of every
/// argument it collects. ToString, a raw pointer unbox or the rest's
/// coercion would otherwise hand the body a value the caller never
/// passed, or slots it cannot read. A boolean / integer slot misses on
/// `undefined`, which it has no bits for.
fn slot_probe(
    ids: &BoxedCoerceIds,
    intr: &BoxedEntryIntrinsics,
    slot: super::fit_tag::SlotTest,
    ty: Type,
    rest: bool,
) -> Probe {
    let probe = |test, on, miss_at| Probe {
        test,
        on,
        miss_at,
        passes_cell: false,
    };
    match (slot.fit, slot.shape) {
        (Some(FitTag { tag, depth: 0 }), _) => Probe {
            passes_cell: true,
            ..probe(
                ids.struct_fits,
                On::Arg(vec![Operand::ConstI64(tag as i64)]),
                0,
            )
        },
        (Some(FitTag { tag, depth }), _) => Probe {
            passes_cell: true,
            ..probe(
                ids.arr_fits,
                On::Arg(vec![
                    Operand::ConstI64(tag as i64),
                    Operand::ConstI64(depth as i64),
                ]),
                0,
            )
        },
        (None, Some(shape)) if rest => probe(ids.rest_fits, On::Rest(shape), 0),
        (None, Some(shape)) => probe(
            ids.arr_cell,
            On::Arg(vec![Operand::ConstI64(shape as i64)]),
            0,
        ),
        _ if ty == Type::Str => probe(ids.str_fits, On::Arg(Vec::new()), 0),
        _ => probe(intr.any_unbox_tag, On::Arg(Vec::new()), 5),
    }
}

/// A twin minted beside a class method or constructor.
pub(super) fn is_twin_name(name: &str) -> bool {
    name.starts_with("__cmany_") || name.starts_with("__smany_") || name.starts_with("__ctorany_")
}

/// The boxed adapter of the all-`any` twin minted beside the mono
/// `name`: the receiver-polymorphic twin of a `__cm_` / `__sm_`
/// method (a `$$` specialization looked up by its base), or the
/// all-`any` relay shell or lifted-closure clone behind a value escape
/// (`check_monomorph_any_widen::struct_escape`), when both the twin
/// and its adapter exist.
pub(crate) fn twin_adapter_of(
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
    entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
    name: &str,
) -> Option<(FuncId, ssa::SigId)> {
    use crate::check_monomorph_any_widen::struct_escape;
    let fid = if super::env_faced(name) {
        *fn_table.get(&struct_escape::twin_name(name))?
    } else {
        let table = if name.starts_with("__cm_") {
            &ast.cmany_twins
        } else if name.starts_with("__sm_") {
            &ast.smany_twins
        } else {
            return None;
        };
        let base = name.split_once("$$").map_or(name, |(b, _)| b);
        let twin = table.get(base)?;
        *fn_table
            .get(twin.as_str())
            .or_else(|| fn_table.get(format!("{twin}$$anywv").as_str()))?
    };
    entries.get(&fid).copied()
}
