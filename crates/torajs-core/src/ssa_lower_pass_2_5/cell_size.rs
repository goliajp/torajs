//! Sized free preconditions, settled over the finished module
//! ([`settle`], the last step of `lower_inner`).
//!
//! An object cell is a bare allocator block: nothing in front of it
//! records its size, so whoever frees it has to name the size it was
//! allocated with — to the size class. Two parties free without
//! having made the allocation:
//!
//! - The runtime reaches a class instance without a static type and
//!   sizes it from its tag's layout row,
//!   `OBJ_HEADER_SIZE + 8 · n_fields`. The allocation constant comes
//!   from the layout as the factory was lowered, the row from the
//!   final layout; a generator or async frame class gains fields
//!   after its factory is lowered (the state machine's spilled
//!   bindings), so [`settle_alloc_sizes_to_rows`] rewrites the
//!   constant to the row and the typed drops of such a sid free by
//!   the cell, since their constants were read at the same early
//!   point.
//! - A typed drop sizes the cell from the slot's static layout, and
//!   the slot may hold a wider cell: struct assignability is prefix
//!   subtyping (a subclass instance in a base-typed slot, `{a, b}` in
//!   an array anchored on `{a}`). Whether any wider layout exists is
//!   only known once every literal is interned, so
//!   [`settle_typed_frees`] decides each such free here. The slot may
//!   also hold a cell of any layout at all where `any` crossed into
//!   it (`cell_slot_flow`). Either way the cell says what it is: a
//!   class instance by its tag's row, anything else by its own tag's
//!   drop (`__torajs_obj_free_cell`).

use std::collections::{HashMap, HashSet};

use torajs_mmalloc::size_class::bucket_for;

use crate::ast::PropKey;
use crate::ssa::{FuncId, InstKind, Module, Operand, Type, ValueId};
use crate::ssa_lower::{OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE};
use crate::ssa_lower_emit_drop_value::leaf_arr::{
    LEAF_INSTANCES, LEAF_INSTANCES_BY_CELL, LEAF_OBJS, LEAF_OBJS_BY_CELL,
};
use crate::ssa_lower_intrinsics_obj_capture::{OBJ_DROP_SIZED_SYM, OBJ_FREE_CELL_SYM};

use super::cell_slot_flow::{EnvLayouts, env_layouts, is_prefix, open_sids};

const OBJ_ALLOC: &str = "__torajs_obj_alloc";

fn cell_bytes(n_fields: usize) -> usize {
    OBJ_HEADER_SIZE as usize + 8 * n_fields
}

/// Runs after the class layout rows are final and every synthesized
/// body (env drops, class releases) is in place. `captures` and the
/// AST's `widen_clone_origin` are the closure side channels the env
/// layouts are read from (`cell_slot_flow::env_layouts`); `facts` names
/// the value classes the SSA is held to (`value_class_check`); the
/// ones it contradicts are the answer.
pub(crate) fn settle(
    module: &mut Module,
    captures: &HashMap<String, Vec<(String, Type, bool)>>,
    ast: &crate::ast::Ast,
    facts: &crate::ssa_lower::proofs::ProgramFacts,
) -> Vec<crate::ssa::StructId> {
    let envs = env_layouts(module, captures, &ast.widen_clone_origin);
    let unsettled = settle_alloc_sizes_to_rows(module);
    settle_typed_frees(module, &envs, &unsettled, ast, facts)
}

/// A typed object drop leaves lowering as `obj_drop_sized(p, size,
/// sid)`, an array of such cells as a leaf kernel with the same
/// operands. Keep the sized free where every cell the slot can hold
/// lands in the size class of `sid`'s own; where a wider layout
/// would not, or where the slot can hold a cell of any layout, free
/// by what the cell says of itself instead.
fn settle_typed_frees(
    module: &mut Module,
    envs: &EnvLayouts,
    unsettled: &HashSet<usize>,
    ast: &crate::ast::Ast,
    facts: &crate::ssa_lower::proofs::ProgramFacts,
) -> Vec<crate::ssa::StructId> {
    let find = |name: &str| {
        let at = module.funcs.iter().position(|f| f.name == name)?;
        Some(FuncId(at as u32))
    };
    // each sized free with its by-cell twin: the single cell's, and
    // the array kernels' that free every element the same way
    let twins: Vec<(FuncId, FuncId)> = [
        (OBJ_DROP_SIZED_SYM, OBJ_FREE_CELL_SYM),
        (LEAF_OBJS, LEAF_OBJS_BY_CELL),
        (LEAF_INSTANCES, LEAF_INSTANCES_BY_CELL),
    ]
    .into_iter()
    .filter_map(|(s, b)| Some((find(s)?, find(b)?)))
    .collect();
    let open = open_sids(module, envs);
    let strict = super::value_class_check::strict();
    let disagree = super::value_class_check::report(module, ast, facts, &open, strict);
    if !disagree.is_empty() {
        // the module lowers again without them
        return disagree;
    }
    let layouts = &module.struct_layouts;
    let mut admits_wider: HashMap<i64, bool> = HashMap::new();
    for f in &mut module.funcs {
        for inst in f.blocks.iter_mut().flat_map(|b| &mut b.insts) {
            let InstKind::Call(fid, args) = &mut inst.kind else {
                continue;
            };
            let Some(&(_, twin)) = twins.iter().find(|(s, _)| s == fid) else {
                continue;
            };
            let [_, _, Operand::ConstI64(sid)] = args.as_slice() else {
                continue;
            };
            let sid = *sid;
            let wider = *admits_wider
                .entry(sid)
                .or_insert_with(|| admits_wider_cell(layouts, sid as usize));
            if wider || open[sid as usize] || unsettled.contains(&(sid as usize)) {
                args.truncate(1);
                *fid = twin;
            } else {
                args.truncate(2);
            }
        }
    }
    disagree
}

/// True when some layout extends `sid`'s by field-name prefix into
/// another size class (or either is past the largest class, where the
/// free needs the exact byte count). Field types are not compared:
/// that only widens the answer.
fn admits_wider_cell(layouts: &[Vec<(PropKey, Type)>], sid: usize) -> bool {
    let base = &layouts[sid];
    let class = bucket_for(cell_bytes(base.len()));
    layouts.iter().any(|wide| {
        wide.len() > base.len()
            && (class.is_none() || bucket_for(cell_bytes(wide.len())) != class)
            && is_prefix(base, wide)
    })
}

/// Every `obj_alloc(n)` whose result is stamped with a class tag ends
/// up allocating what the tag's row sizes the instance at. The two
/// disagree when the class gained fields after its factory was
/// lowered — a generator or async frame class, whose state machine
/// spills bindings into the frame while the body is lowered — and the
/// instance's field stores already address the row's layout, so the
/// smaller constant was a cell written past its end. Answers the sids
/// whose constant moved: their typed drops carry the same early
/// constant and free by the cell instead. Every stamp names a row: the
/// runtime sizes a `Tag::Obj` cell it reaches without a static type
/// from its row, so a cell stamped 0 could never be freed, and a
/// stamp past the table is a row that was never emitted.
fn settle_alloc_sizes_to_rows(module: &mut Module) -> HashSet<usize> {
    let mut unsettled = HashSet::new();
    let Some(alloc) = module.funcs.iter().position(|f| f.name == OBJ_ALLOC) else {
        return unsettled;
    };
    let rows: Vec<i64> = module
        .class_layouts
        .iter()
        .map(|row| cell_bytes(row.field_metadata.len()) as i64)
        .collect();
    for f in &mut module.funcs {
        let mut allocs: HashMap<ValueId, (usize, usize)> = HashMap::new();
        for (bi, b) in f.blocks.iter().enumerate() {
            for (ii, inst) in b.insts.iter().enumerate() {
                if let (Some(v), InstKind::Call(fid, args)) = (inst.result, &inst.kind)
                    && fid.0 as usize == alloc
                    && let [Operand::ConstI64(_)] = args.as_slice()
                {
                    allocs.insert(v, (bi, ii));
                }
            }
        }
        if allocs.is_empty() {
            continue;
        }
        let mut moves: Vec<(usize, usize, i64, usize)> = Vec::new();
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            let InstKind::Store(Operand::ConstI64(tag), Operand::Value(p), OBJ_CLASS_TAG_OFF) =
                &inst.kind
            else {
                continue;
            };
            let Some(&(bi, ii)) = allocs.get(p) else {
                continue;
            };
            let Some(&by_row) = (*tag as usize).checked_sub(1).and_then(|i| rows.get(i)) else {
                panic!(
                    "{}: object cell stamped with class tag {tag}, which has no layout row",
                    f.name
                );
            };
            let Type::Obj(sid) = f.value_type(*p) else {
                continue;
            };
            moves.push((bi, ii, by_row, sid.0 as usize));
        }
        for (bi, ii, by_row, sid) in moves {
            let InstKind::Call(_, args) = &mut f.blocks[bi].insts[ii].kind else {
                continue;
            };
            if args[0] != Operand::ConstI64(by_row) {
                args[0] = Operand::ConstI64(by_row);
                unsettled.insert(sid);
            }
        }
    }
    unsettled
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(names: &[&str]) -> Vec<(PropKey, Type)> {
        names
            .iter()
            .map(|n| (PropKey::from(*n), Type::I64))
            .collect()
    }

    #[test]
    fn a_wider_layout_in_another_class_opens_its_prefix() {
        // 40 B and 80 B: classes 64 and 128
        let layouts = [layout(&["a"]), layout(&["a", "s", "b", "c", "d", "e"])];
        assert!(admits_wider_cell(&layouts, 0));
        assert!(!admits_wider_cell(&layouts, 1));
    }

    #[test]
    fn a_wider_layout_in_the_same_class_does_not() {
        // 48 B and 64 B share the 64 class
        let layouts = [
            layout(&["kind", "value"]),
            layout(&["kind", "value", "l", "r"]),
        ];
        assert!(!admits_wider_cell(&layouts, 0));
    }

    #[test]
    fn an_unrelated_layout_does_not() {
        let layouts = [layout(&["a"]), layout(&["b", "c", "d", "e", "f", "g"])];
        assert!(!admits_wider_cell(&layouts, 0));
    }

    #[test]
    fn the_empty_layout_is_every_layouts_prefix() {
        let layouts = [layout(&[]), layout(&["b"])];
        assert!(admits_wider_cell(&layouts, 0));
    }
}
