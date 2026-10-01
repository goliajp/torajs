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
//!   from the literal being lowered, the row from the module metadata
//!   builders; [`assert_cell_sizes_match_rows`] pins the two together.
//! - A typed drop sizes the cell from the slot's static layout, and
//!   the slot may hold a wider cell: struct assignability is prefix
//!   subtyping (a subclass instance in a base-typed slot, `{a, b}` in
//!   an array anchored on `{a}`). Whether any wider layout exists is
//!   only known once every literal is interned, so
//!   [`settle_typed_frees`] decides each such free here. The slot may
//!   also hold a cell of any layout at all where `any` crossed into
//!   it (`cell_slot_flow`). Either way the cell says what it is: a
//!   class instance by its tag's row, anything else by the span
//!   registry (`__torajs_obj_free_cell`).

use std::collections::HashMap;

use torajs_mmalloc::Allocator;

use crate::ast::PropKey;
use crate::ssa::{FuncId, InstKind, Module, Operand, Type, ValueId};
use crate::ssa_lower::{OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE};
use crate::ssa_lower_intrinsics_obj_capture::{OBJ_DROP_SIZED_SYM, OBJ_FREE_CELL_SYM};

use super::cell_slot_flow::{EnvLayouts, env_layouts, is_prefix, open_sids};

const OBJ_ALLOC: &str = "__torajs_obj_alloc";

fn cell_bytes(n_fields: usize) -> usize {
    OBJ_HEADER_SIZE as usize + 8 * n_fields
}

/// Runs after the class layout rows are final and every synthesized
/// body (env drops, class releases) is in place. `captures` and
/// `clone_origin` are the closure side channels the env layouts are
/// read from (`cell_slot_flow::env_layouts`).
pub(crate) fn settle(
    module: &mut Module,
    captures: &HashMap<String, Vec<(String, Type, bool)>>,
    clone_origin: &HashMap<String, String>,
) {
    let envs = env_layouts(module, captures, clone_origin);
    settle_typed_frees(module, &envs);
    assert_cell_sizes_match_rows(module);
}

/// A typed object drop leaves lowering as `obj_drop_sized(p, size,
/// sid)`. Keep the sized free where every cell the slot can hold
/// lands in the size class of `sid`'s own; where a wider layout
/// would not, or where the slot can hold a cell of any layout, free
/// by what the cell says of itself instead.
fn settle_typed_frees(module: &mut Module, envs: &EnvLayouts) {
    let find = |name: &str| {
        let at = module.funcs.iter().position(|f| f.name == name)?;
        Some(FuncId(at as u32))
    };
    let (Some(sized), Some(by_cell)) = (find(OBJ_DROP_SIZED_SYM), find(OBJ_FREE_CELL_SYM)) else {
        return;
    };
    let open = open_sids(module, envs);
    let layouts = &module.struct_layouts;
    let mut admits_wider: HashMap<i64, bool> = HashMap::new();
    for f in &mut module.funcs {
        for inst in f.blocks.iter_mut().flat_map(|b| &mut b.insts) {
            let InstKind::Call(fid, args) = &mut inst.kind else {
                continue;
            };
            if *fid != sized {
                continue;
            }
            let [_, _, Operand::ConstI64(sid)] = args.as_slice() else {
                continue;
            };
            let sid = *sid;
            let wider = *admits_wider
                .entry(sid)
                .or_insert_with(|| admits_wider_cell(layouts, sid as usize));
            if wider || open[sid as usize] {
                args.truncate(1);
                *fid = by_cell;
            } else {
                args.truncate(2);
            }
        }
    }
}

/// True when some layout extends `sid`'s by field-name prefix into
/// another size class (or either is past the largest class, where the
/// free needs the exact byte count). Field types are not compared:
/// that only widens the answer.
fn admits_wider_cell(layouts: &[Vec<(PropKey, Type)>], sid: usize) -> bool {
    let base = &layouts[sid];
    let class = Allocator::bucket_for(cell_bytes(base.len()));
    layouts.iter().any(|wide| {
        wide.len() > base.len()
            && (class.is_none() || Allocator::bucket_for(cell_bytes(wide.len())) != class)
            && is_prefix(base, wide)
    })
}

/// Panics on the first `obj_alloc(n)` whose result is stamped with a
/// class tag whose row sizes the instance differently. A tag with no
/// row is left alone: the runtime frees such a cell through the span
/// registry, not by a computed size.
fn assert_cell_sizes_match_rows(module: &Module) {
    let Some(alloc) = module.funcs.iter().position(|f| f.name == OBJ_ALLOC) else {
        return;
    };
    for f in &module.funcs {
        let mut sized: HashMap<ValueId, i64> = HashMap::new();
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            if let (Some(v), InstKind::Call(fid, args)) = (inst.result, &inst.kind)
                && fid.0 as usize == alloc
                && let [Operand::ConstI64(n)] = args.as_slice()
            {
                sized.insert(v, *n);
            }
        }
        if sized.is_empty() {
            continue;
        }
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            let InstKind::Store(Operand::ConstI64(tag), Operand::Value(p), OBJ_CLASS_TAG_OFF) =
                &inst.kind
            else {
                continue;
            };
            let Some(&n) = sized.get(p) else { continue };
            let Some(row) = (*tag as usize)
                .checked_sub(1)
                .and_then(|i| module.class_layouts.get(i))
            else {
                continue;
            };
            let by_row = cell_bytes(row.field_metadata.len()) as i64;
            assert!(
                n == by_row,
                "ssa-lower: `{}` allocates {n} bytes for a cell of class tag {tag} (`{}`), \
                 whose layout row sizes it at {by_row}",
                f.name,
                row.class_name,
            );
        }
    }
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
