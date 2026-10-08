//! The vtable-slot row of the this-first argc tier: an overridden
//! method whose rows read `arguments.length`.
//!
//! Every row of a vtable slot is entered through one signature (the
//! slot ABI check refuses a slot whose rows disagree), so the count
//! cannot ride one row alone: either the whole slot carries the hidden
//! argc right after `__this`, or none of it does. A slot takes it when
//! some row reads `arguments.length` and no row reads argument values
//! (that would need the argv channel too); the rows that never read
//! `arguments` then carry one I64 they ignore. The `__dispatch_` stub
//! shares the slot's signature, so it carries the slot too, and
//! forwards its own count to the row it calls.
//!
//! The callers that reach a row: the vtable and sibling lanes (which
//! insert the count the source wrote), direct calls by name (`super.m`,
//! a devirtualized site; the direct-call terminal), and the boxed
//! adapter (its own argc). A row a value use, a spread site, an
//! accessor or protocol lane can reach keeps the whole slot off.

use super::super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
};
use super::super::{Ast, Param, Stmt};
use super::method::accessor_or_protocol;
use super::{has_spread_call_site, name_only_called_directly};

/// The rows and `__dispatch_` stubs of every vtable slot the argc tier
/// admits (head width 1).
pub(super) fn collect_slot_argc(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
) -> Vec<String> {
    let decls: std::collections::HashMap<&str, (&[Param], &[Stmt])> =
        super::super::toplevel_stmts_flat(ast)
            .into_iter()
            .filter_map(|s| match s {
                Stmt::FnDecl {
                    name, params, body, ..
                } => Some((name.as_str(), (params.as_slice(), body.as_slice()))),
                _ => None,
            })
            .collect();
    let decl_names: Vec<String> = decls.keys().map(|k| k.to_string()).collect();
    let mut out = Vec::new();
    for (m, by_root) in super::super::vtable_slot::slot_groups_by_name(ast) {
        let rows: Vec<&String> = by_root.values().flatten().collect();
        let stubs = super::super::vtable_slot::dispatch_stub_names(decl_names.iter(), &m);
        let Some(rows) = admitted_rows(ast, excluded, &decls, &rows, &stubs) else {
            continue;
        };
        out.extend(rows);
        out.extend(stubs);
    }
    out
}

/// The slot's rows when the slot takes the argc tier, `None` when it
/// stays on its declared signature.
fn admitted_rows(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
    decls: &std::collections::HashMap<&str, (&[Param], &[Stmt])>,
    rows: &[&String],
    stubs: &[String],
) -> Option<Vec<String>> {
    let mut reads_length = false;
    for row in rows {
        let (params, body) = decls.get(row.as_str())?;
        if !row_shape_admits(params) || accessor_or_protocol(ast, row) {
            return None;
        }
        if body_has_non_length_arguments_touch(ast, body) {
            return None;
        }
        reads_length |= body_has_arguments_length(ast, body);
    }
    if !reads_length {
        return None;
    }
    let names = rows
        .iter()
        .map(|r| r.as_str())
        .chain(stubs.iter().map(|s| s.as_str()));
    for n in names {
        if excluded.contains(n)
            || has_spread_call_site(ast, n)
            || !name_only_called_directly(ast, n)
        {
            return None;
        }
    }
    Some(rows.iter().map(|r| r.to_string()).collect())
}

/// A row's declaration has the plain receiver head and no parameter a
/// call-site reshape packs (rest, destructured).
fn row_shape_admits(params: &[Param]) -> bool {
    params.first().is_some_and(|p| p.name == "__this")
        && params.get(1).is_none_or(|p| p.name != "__new_target")
        && !params
            .iter()
            .any(|p| p.is_rest || p.name.starts_with("__param_destr_"))
}
