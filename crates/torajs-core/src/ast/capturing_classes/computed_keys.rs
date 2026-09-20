//! The computed-member key vocabulary of the ES5 class lane: which
//! `__ccm_<n>__` sentinels a class owns, in the element order
//! §15.7.14 evaluates their keys in, and the binding each key's value
//! is parked under. Both the lane (`super`) and its refusal side
//! (`decline`) speak it, and the nested-class admission census reads
//! it too, so it lives apart from either.

use super::super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::super::{Ast, Expr, ExprId, Stmt};

/// The declaration index a `__ccm_<n>__` sentinel carries. The parser
/// mints `n` off a program-wide counter, so within one class it sorts
/// the computed members back into element order — which is the order
/// §15.7.14 evaluates their keys in.
pub(super) fn sentinel_index(member_name: &str) -> Option<usize> {
    member_name
        .strip_prefix("__ccm_")?
        .trim_end_matches('_')
        .parse()
        .ok()
}

/// Where the evaluated key of computed member `n` of class `cname`
/// parks. The name is the parser's, not this module's: a computed
/// INSTANCE field is already rewritten to `(this as any)[__ccmk_<C>_<n>]
/// = <init>` in the constructor prefix, and that reference has to find
/// something.
pub(super) fn key_binding(cname: &str, n: usize) -> String {
    format!("__ccmk_{cname}_{n}")
}

/// The computed members THIS class declared, by sentinel index, in
/// element order.
///
/// `class_computed_keys` is keyed by class NAME, and a name is not
/// unique — three functions each declaring `class K { [k]() {…} }`
/// share one entry set, so asking the table by name answers with all
/// three. Ownership is read back off the class itself instead: a
/// computed method keeps its `__ccm_<n>__` sentinel as the member
/// name, and a computed instance field is by now a
/// `(this as any)[__ccmk_<C>_<n>]` write in the constructor prefix.
pub(crate) fn own_computed_members(
    ast: &Ast,
    cname: &str,
    ctor_body: &[Stmt],
    members: &[&str],
) -> Vec<usize> {
    let mut ns: Vec<usize> = members.iter().filter_map(|n| sentinel_index(n)).collect();
    let prefix = format!("__ccmk_{cname}_");
    collect_key_refs(ast, ctor_body, &prefix, &mut ns);
    ns.sort_unstable();
    ns.dedup();
    ns
}

/// Every `<prefix><n>` identifier anywhere in `body`.
fn collect_key_refs(ast: &Ast, body: &[Stmt], prefix: &str, out: &mut Vec<usize>) {
    let mut pending: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = pending.pop() {
        for e in stmt_exprs(s) {
            collect_key_refs_in_expr(ast, e, prefix, out);
        }
        pending.extend(stmt_children_ref(s));
    }
}

fn collect_key_refs_in_expr(ast: &Ast, root: ExprId, prefix: &str, out: &mut Vec<usize>) {
    let mut pending = vec![root];
    while let Some(eid) = pending.pop() {
        match ast.get_expr(eid) {
            Expr::Ident(n) => {
                if let Some(n) = n.strip_prefix(prefix).and_then(|r| r.parse::<usize>().ok()) {
                    out.push(n);
                }
            }
            // An arrow's body is a statement list, not a child expr.
            Expr::ArrowFn { body, .. } => collect_key_refs(ast, body, prefix, out),
            _ => {}
        }
        pending.extend(expr_children(ast, eid));
    }
}

/// The key expression of each member in `ns`, dropping any the parser
/// did not record (there is none today; a missing one would mean the
/// sentinel outlived its side-table entry, and emitting nothing for it
/// keeps that loud downstream rather than inventing a key).
pub(crate) fn keys_of(ast: &Ast, cname: &str, ns: &[usize]) -> Vec<(usize, ExprId)> {
    ns.iter()
        .filter_map(|n| {
            let key = ast
                .class_computed_keys
                .get(&(cname.to_string(), format!("__ccm_{n}__")))?;
            Some((*n, *key))
        })
        .collect()
}

/// The computed members this class declares, and the `(n, init)` rows
/// of its computed STATIC fields — the two halves the ES5 lowering
/// needs, lifted out of `lower_to_es5` at the size cap.
///
/// The static half is asked by OWNER (RFC 20260921): the side table is
/// keyed by class name, and until the owner was recorded the lane had
/// to refuse a class whose name another ClassDecl shared, because the
/// rows it read could have been theirs.
pub(crate) fn own_members_and_static_fields(
    ast: &Ast,
    (src_name, decl_id): (&str, u32),
    ctor_body: &[Stmt],
    (methods, static_methods): (&[crate::ast::ClassMethod], &[crate::ast::ClassMethod]),
) -> (Vec<usize>, Vec<(usize, ExprId)>) {
    let member_names: Vec<&str> = methods
        .iter()
        .chain(static_methods.iter())
        .filter_map(|m| m.name.as_str())
        .collect();
    let static_cf: Vec<(usize, ExprId)> = ast
        .class_computed_static_fields
        .iter()
        .filter(|(_, sent, _)| crate::ast::class_computed_owner::owns(ast, sent, decl_id))
        .filter_map(|(_, sent, init)| sentinel_index(sent).map(|n| (n, *init)))
        .collect();
    let mut own = own_computed_members(ast, src_name, ctor_body, &member_names);
    own.extend(static_cf.iter().map(|(n, _)| *n));
    own.sort_unstable();
    own.dedup();
    (own, static_cf)
}
