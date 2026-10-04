//! Which parameter slots an argument cannot cross as it is, and the
//! layout test a struct-shaped one asks of it.

use crate::ssa::{self, Type};

/// The layout test a struct-shaped slot asks of its argument: the
/// class tag of the cell layout, behind `depth` levels of array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FitTag {
    pub(crate) tag: u32,
    pub(crate) depth: u32,
}

/// The [`FitTag`] of a struct-shaped slot type: a cell, or an array
/// of cells at any depth. `None` for every other type.
pub(crate) fn fit_tag(
    ty: Type,
    arr_layouts: &[Type],
    sid_tag: &dyn Fn(ssa::StructId) -> u32,
) -> Option<FitTag> {
    let mut depth = 0;
    let mut t = ty;
    loop {
        match t {
            Type::Obj(sid) => {
                return Some(FitTag {
                    tag: sid_tag(sid),
                    depth,
                });
            }
            Type::Arr(a) => {
                depth += 1;
                t = arr_layouts[a.0 as usize];
            }
            _ => return None,
        }
    }
}

/// The run-time test of an array slot whose elements are not
/// struct-shaped: the slot's element-kind chain (3 bits per level, the
/// encoding `__torajs_arr_mark_kind` stamps) and whether the innermost
/// elements are strings. `None` for every other type.
pub(crate) fn arr_cell_test(ty: Type, arr_layouts: &[Type]) -> Option<(u64, bool)> {
    let Type::Arr(a) = ty else { return None };
    let elem = arr_layouts[a.0 as usize];
    let mut leaf = elem;
    while let Type::Arr(b) = leaf {
        leaf = arr_layouts[b.0 as usize];
    }
    let chain = crate::ssa_lower_arr_kind_mark::arr_kind_chain_of(arr_layouts, &elem, 0);
    Some((chain, leaf == Type::Str))
}

/// A slot an argument cannot cross without loss: no literal default
/// to substitute, and a type with no undefined of its own, a string
/// or array slot that cannot hold `null` / `undefined` / another kind
/// of value, or a struct-shaped layout (`fit`) the argument may not
/// have. A rest slot is fed from the trailing argv window, one element
/// per slot, so only its struct layout is asked about.
pub(super) fn lossy_slot(
    ty: Type,
    dflt: Option<&super::DfltLit>,
    fit: Option<FitTag>,
    rest: bool,
) -> bool {
    dflt.is_none()
        && (fit.is_some()
            || !rest
                && matches!(
                    ty,
                    Type::Bool | Type::I64 | Type::I32 | Type::Str | Type::Arr(_)
                ))
}
