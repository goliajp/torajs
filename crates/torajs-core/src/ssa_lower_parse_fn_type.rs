//! `__fn(P1|P2|...)->(R)` annotation decoder — extracted from
//! `ssa_lower_parse_type.rs` so that file fits under the 500-prod-LOC
//! file-size hard limit (`rules/common/file-size.md`). Pure mechanical
//! pull, no semantic change.
//!
//! Same encoding produced by `parser::parse_type_ann` and decoded
//! the same depth-aware way as `check.rs::resolve_type_ann` so SSA +
//! check agree on the signature structure.

use crate::ast::PropKey;
use std::collections::HashMap;

use crate::ssa::{self, Type};
use crate::ssa_lower::intern_fn_sig;
use crate::ssa_lower_parse_type::parse_type;

/// Decode `__fn(P1|P2|...)->(R)` into `Type::FnSig(id)` if `s` starts
/// with `__fn(`; returns `None` otherwise so `parse_type` can fall
/// through to the next annotation form.
///
/// Recursively invokes `parse_type` on each param + the return type
/// so the same alias / arr-layout / fn-sig / generic-struct /
/// struct-layout / inst-memo state used by the parent walk is threaded
/// through.
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_parse_fn_type(
    s: &str,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(PropKey, String)>)>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
) -> Option<Type> {
    let rest = s.strip_prefix("__fn(")?;
    let bytes = rest.as_bytes();
    let mut depth: i32 = 1;
    let mut close_idx = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close_idx.unwrap_or_else(|| panic!("ssa-lower: malformed fn-type `{s}`"));
    let params_str = &rest[..close];
    let after = &rest[close + 1..];
    let ret_str = crate::type_ann_fnsig::ret_of_tail(after)
        .unwrap_or_else(|| panic!("ssa-lower: malformed fn-type ret `{s}`"));

    // Split params at depth-0 `|` on the checker's own splitter, so
    // the two sides of this encoding cannot disagree about what nests.
    let mut params: Vec<Type> = Vec::new();
    for seg in crate::check_type_ann::split_top_pipe(params_str) {
        params.push(parse_type(
            Some(seg),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        ));
    }
    let ret = parse_type(
        Some(ret_str),
        aliases,
        arr_layouts,
        fn_sigs,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
    );
    let id = intern_fn_sig(fn_sigs, params, ret);
    Some(Type::FnSig(id))
}
