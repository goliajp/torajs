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

/// The run-time test one parameter slot asks of its argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotTest {
    /// The layout test of a struct-shaped slot.
    pub(crate) fit: Option<FitTag>,
    /// The shape word of an array slot over non-struct elements
    /// (`torajs_rc::arr_kind::SLOT_LEVEL_BITS`).
    pub(crate) shape: Option<u64>,
}

pub(crate) fn slot_test(
    ty: Type,
    arr_layouts: &[Type],
    sid_tag: &dyn Fn(ssa::StructId) -> u32,
) -> SlotTest {
    let fit = fit_tag(ty, arr_layouts, sid_tag);
    let shape = match ty {
        Type::Arr(a) if fit.is_none() => Some(level(arr_layouts[a.0 as usize], arr_layouts, 0)),
        _ => None,
    };
    SlotTest { fit, shape }
}

/// The shape bits of one array level whose elements are `elem`, and of
/// every level under it. Past four levels nothing passes, and a heap
/// element other than a string or an array names no tag, so the test
/// sends such arguments to the twin.
fn level(elem: Type, arr_layouts: &[Type], depth: u32) -> u64 {
    use torajs_rc::arr_kind::{SLOT_KIND_NONE, SLOT_LEVEL_BITS, SLOT_TAG_NONE};
    let heap = |tag: u64| u64::from(torajs_rc::ARR_KIND_HEAP) | tag << 3;
    if depth >= 4 {
        return SLOT_KIND_NONE;
    }
    match elem {
        Type::Any => u64::from(torajs_rc::ARR_KIND_UNSET),
        Type::I64 | Type::I32 => u64::from(torajs_rc::ARR_KIND_I64),
        Type::F64 => u64::from(torajs_rc::ARR_KIND_F64),
        Type::Bool => u64::from(torajs_rc::ARR_KIND_BOOL),
        Type::Str => heap(torajs_rc::Tag::Str as u64),
        Type::Arr(b) => {
            heap(torajs_rc::Tag::Arr as u64)
                | level(arr_layouts[b.0 as usize], arr_layouts, depth + 1) << SLOT_LEVEL_BITS
        }
        t if t.is_refcounted() => heap(SLOT_TAG_NONE),
        _ => SLOT_KIND_NONE,
    }
}

/// A slot an argument cannot cross without loss: no literal default
/// to substitute, and a type with no undefined of its own, a string
/// or array slot that cannot hold `null` / `undefined` / another kind
/// of value, or a struct-shaped layout (`fit`) the argument may not
/// have. A rest slot asks the same of every argument it collects,
/// which an `any[]` rest takes as they are.
pub(super) fn lossy_slot(
    ty: Type,
    dflt: Option<&super::DfltLit>,
    test: SlotTest,
    rest: bool,
) -> bool {
    dflt.is_none()
        && (test.fit.is_some()
            || match ty {
                Type::Bool | Type::I64 | Type::I32 | Type::Str => true,
                Type::Arr(_) => !rest || test.shape != Some(0),
                _ => false,
            })
}
