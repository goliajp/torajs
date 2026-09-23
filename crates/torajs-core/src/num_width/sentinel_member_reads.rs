//! Which member READS can hand the `undefined` sentinel back, where
//! no name answers for them.
//!
//! The sibling `sentinel_tables` answers the question by NAME — which
//! functions, parameters, fields, elements and bindings can carry
//! one. This file is the residue that has no name to carry: an
//! awaited promise's value and an accessor read. Both are
//! `Expr::Member` reads, and neither is a write to a field of that
//! name, so the field set never saw them; both are what lowering's
//! arming predicate already accepts. One question each; that is the
//! line between the two files.
//!
//! **Both halves of the answer live here, deliberately.** A shape
//! this file admits has to reach two places or it does nothing:
//! `is_sentinel_source`, so the value is tainted at all, and
//! `width.rs`, so the slot it lands in is F64. Arming an I64 slot is
//! a silent no-op — the sentinel is a NaN bit pattern and an I64 slot
//! cannot spell one — and the `NaN` that results looks exactly like
//! the taint table having missed the value (RFC 20260923 §3.3c, which
//! cost a rotation). Keeping the admission rule in one function makes
//! "admitted but not widened" unrepresentable.
//!
//! The candidates are collected ONCE, before the tables close, and
//! they have to be: the getter half reads the whole expression arena
//! (an object-literal getter survives only as a `__getter_<prop>`
//! field), while `is_sentinel_source` runs inside a fixpoint and
//! `width.rs` runs per expression. What cannot be precomputed is
//! whether the getter's body falls through — that is `fns`, which the
//! fixpoint is still growing — so the candidate list is by name and
//! the `fns` test is applied at each query.

use super::{Analysis, sentinel_tables::SentinelTables};
use crate::ast::{Expr, ExprId};
use std::collections::{HashMap, HashSet};

/// Per-expression candidates, independent of how far the sentinel
/// tables have closed.
#[derive(Default)]
pub(super) struct MemberCandidates {
    /// `await p` on a `Promise<number>` — which parses to `p.value`.
    /// Unconditional, matching the lowering-side arming predicate: a
    /// promise's value slot carries whatever settled it, and an async
    /// body that runs off its end settles with the sentinel
    /// (ES §10.2.1.4 step 11).
    awaited: HashSet<ExprId>,
    /// Reads that run a getter: the functions reading this property
    /// can call. Whether any of them hands the sentinel back is the
    /// `fns` question, answered per query.
    getters: HashMap<ExprId, Vec<String>>,
}

pub(super) fn collect_member_candidates(a: &Analysis<'_>) -> MemberCandidates {
    let mut out = MemberCandidates::default();
    // One arena scan per distinct property name rather than per read:
    // `getter_fn_names` is the single spelling of the name -> function
    // hop (shared with lowering) and each call scans the arena for
    // object-literal getters.
    let mut memo: HashMap<String, Vec<String>> = HashMap::new();
    for i in 0..a.ast.exprs.len() {
        let eid = ExprId(i as u32);
        let Expr::Member { name, .. } = a.ast.get_expr(eid) else {
            continue;
        };
        if matches!(
            super::awaited_promise_inner_of(a.ast, a.expr_types, eid),
            Some(crate::check::Type::Number)
        ) {
            out.awaited.insert(eid);
            continue;
        }
        let fns = memo.entry(name.clone()).or_insert_with(|| {
            crate::ast::accessor_lookup::getter_fn_names(a.ast, name)
                .iter()
                .map(|f| (*f).to_string())
                .collect()
        });
        if !fns.is_empty() {
            out.getters.insert(eid, fns.clone());
        }
    }
    out
}

/// The admission rule, against the tables as far as they are known.
pub(super) fn hands_back_sentinel(t: &SentinelTables, eid: ExprId) -> bool {
    t.members.awaited.contains(&eid)
        || t.members
            .getters
            .get(&eid)
            .is_some_and(|fns| fns.iter().any(|f| t.fns.contains(f)))
}

/// The same rule frozen over every member read, for `width.rs` to
/// query per expression once the tables have closed.
pub(super) fn freeze(t: &SentinelTables) -> HashSet<ExprId> {
    let mut out: HashSet<ExprId> = t.members.awaited.iter().copied().collect();
    out.extend(
        t.members
            .getters
            .keys()
            .copied()
            .filter(|eid| hands_back_sentinel(t, *eid)),
    );
    out
}
