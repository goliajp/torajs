//! W4 (ann-width RFC §5.4 D2) — container element width injection.
//!
//! `parse_type` resolves `number[]` to the I64-elem default; the
//! module-wide num_width analysis decides per *alias class* whether
//! every value reaching the array's elements is provably integral.
//! Each annotation-consuming site calls `widen_arr_elem` with its
//! slot key right after parse_type: when the elem face is `number`
//! and the class is f64-possible, the array type re-interns with an
//! F64 element (recursively for `number[][]` — the inner face keys
//! through the Elem spelling, which the table canonicalizes onto the
//! congruence-closed class).
//!
//! The runtime is width-agnostic here: array slots are always 8 bytes
//! (`torajs-arr` layout), so this changes only the compiler-side
//! interpretation — load/store/print sites all read
//! `arr_layouts[arr_id]` and follow automatically.

use crate::num_width::{SlotKey, WidthTable};
use crate::ssa::Type;
use crate::ssa_lower::intern_arr_layout;

/// Widen a parsed / inferred array type's element faces per the
/// width table. `key` is the slot (or literal Anon origin) holding
/// the container. With an annotation, the `number` elem gate keeps
/// explicit `: i64[]` narrow (mirroring the scalar W1 consumer
/// gate); without one (un-annotated `let a = [1, 2]`, literal
/// origins) an I64 elem can only have come from the number-domain
/// inference, so it is widenable by construction.
pub(crate) fn widen_arr_elem(
    parsed: Type,
    ann: Option<&str>,
    key: &SlotKey,
    table: &WidthTable,
    arr_layouts: &mut Vec<Type>,
) -> Type {
    let Type::Arr(id) = parsed else {
        return parsed;
    };
    let elem_ann = match ann {
        Some(a) => match a.strip_suffix("[]") {
            Some(inner) => Some(inner),
            // Annotated but not `T[]`-spelled (alias) — leave the
            // user's spelling alone; alias faces are D3+ scope.
            None => return parsed,
        },
        None => None,
    };
    let number_elem = matches!(elem_ann, Some("number") | None);
    let elem = arr_layouts[id.0 as usize];
    let widened_elem = match elem {
        Type::I64 if number_elem && table.elem_is_f64(key) => Type::F64,
        Type::Arr(_) => {
            let elem_key = SlotKey::Elem(Box::new(key.clone()));
            widen_arr_elem(elem, elem_ann, &elem_key, table, arr_layouts)
        }
        _ => elem,
    };
    if widened_elem == elem {
        return parsed;
    }
    Type::Arr(intern_arr_layout(arr_layouts, widened_elem))
}
