//! Per-ExprId side tables that must ride a specialization clone.
//!
//! `clone_spec_body` deep-clones a generic body into fresh ExprIds;
//! anything the pipeline recorded against the ORIGINAL ids is
//! invisible to the clone unless copied across the id map. Two
//! registries qualify today — both keyed by a call/destructuring
//! site the clone reproduces verbatim:
//!
//! * `ary_destr_groups` — the array-destructuring group registry, so
//!   the specialization's own check picks each group's lane (it can
//!   differ per instantiation: the same `const [a, b] = xs`
//!   destructures an array in one and a generator in the next).
//! * `default_padded_argc` — the source-written argument count of a
//!   call `apply_default_args` padded; without the carry the fresh
//!   ExprId falls back to the padded length and the head-less tier's
//!   `arguments.length` over-counts.
//!
//! Split from `check_monomorph.rs` at the 500-line file cap.

use crate::ast::{Ast, ExprId};

/// Copy every per-ExprId registry entry from the cloned-from ids to
/// their clones.
pub(crate) fn carry(owned_ast: &mut Ast, id_map: &[(ExprId, ExprId)]) {
    let groups: Vec<(ExprId, i64)> = id_map
        .iter()
        .filter_map(|&(old, new)| {
            owned_ast
                .ary_destr_groups
                .get(&old)
                .map(|&limit| (new, limit))
        })
        .collect();
    owned_ast.ary_destr_groups.extend(groups);
    let argc: Vec<(ExprId, usize)> = id_map
        .iter()
        .filter_map(|&(old, new)| {
            owned_ast
                .default_padded_argc
                .get(&old)
                .map(|&count| (new, count))
        })
        .collect();
    owned_ast.default_padded_argc.extend(argc);
}
