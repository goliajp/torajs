//! The `__cm_` method row of the this-first argc tier: which class
//! methods may carry the hidden argc right after `__this` (module doc
//! in the parent).

use super::super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
};
use super::super::{Ast, Expr, Param, Stmt};
use super::{has_spread_call_site, name_only_called_directly};

/// The `__cm_` methods the argc tier admits (head width 1), outside
/// the excluded set and the ones the static face already answers.
pub(super) fn collect_method_argc(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
    static_argv: &std::collections::HashMap<String, usize>,
) -> Vec<String> {
    let bars = MethodBars::of(ast);
    ast.stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name, params, body, ..
            } if name.starts_with("__cm_")
                && !name.ends_with("__ctor")
                && !ast.smany_twins.values().any(|t| t == name)
                && !ast.this_param_fns.contains(name)
                && !excluded.contains(name)
                && !static_argv.contains_key(name)
                && method_admits(ast, &bars, name, params, body) =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .collect()
}

/// What keeps a `__cm_` method off the argc tier regardless of its
/// own declaration: the names an old-signature lane can call.
struct MethodBars {
    /// Every `__cm_<C>__<M>` the vtable / `__dispatch_` / sibling
    /// lanes can spell.
    old_signature: std::collections::HashSet<String>,
}

impl MethodBars {
    fn of(ast: &Ast) -> Self {
        MethodBars {
            old_signature: super::super::arguments_object_method_argv::old_signature_lane(ast),
        }
    }
}

/// A class getter or setter (the member-read and member-write lanes
/// call it by fid with the receiver alone in front), or a method a
/// protocol lane calls by its spelled name ([`PROTOCOL_METHODS`]).
pub(super) fn accessor_or_protocol(ast: &Ast, name: &str) -> bool {
    ast.accessor_getters
        .values()
        .chain(ast.accessor_setters.values())
        .any(|a| a == name)
        || PROTOCOL_METHODS.iter().any(|m| name.ends_with(m))
}

/// Method names lowering calls by their spelled `__cm_<C>__<M>` fid
/// with the declared signature: the for-of protocol (`@@iterator`,
/// `next`), and the hooks a typed lane looks up by name (`toJSON`,
/// `then`). `return` / `throw` / `@@asyncIterator` ride along as the
/// rest of the iterator protocol.
const PROTOCOL_METHODS: [&str; 7] = [
    "__next",
    "__return",
    "__throw",
    "__toJSON",
    "__then",
    "__sym_Symbol_iterator__",
    "__sym_Symbol_asyncIterator__",
];

/// A single-owner `__cm_` method takes the argc tier when its body
/// reads only `arguments.length`, it has at least one direct call
/// (`__cm_C__m(recv, …)`) and every other use of the name is one too,
/// and no lane that calls by the declared signature can reach it.
/// Requiring a direct call keeps it disjoint from the method argv
/// face, which admits only methods with no `Ident` at all.
fn method_admits(
    ast: &Ast,
    bars: &MethodBars,
    name: &str,
    params: &[Param],
    body: &[Stmt],
) -> bool {
    if params.first().is_none_or(|p| p.name != "__this")
        || params.get(1).is_some_and(|p| p.name == "__new_target")
    {
        return false;
    }
    if !body_has_arguments_length(ast, body) || body_has_non_length_arguments_touch(ast, body) {
        return false;
    }
    if params
        .iter()
        .any(|p| p.is_rest || p.name.starts_with("__param_destr_"))
    {
        return false;
    }
    if bars.old_signature.contains(name) || accessor_or_protocol(ast, name) {
        return false;
    }
    let called = ast.exprs.iter().any(|e| {
        matches!(e, Expr::Call { callee, .. }
            if matches!(ast.get_expr(*callee), Expr::Ident(n) if n == name))
    });
    called && !has_spread_call_site(ast, name) && name_only_called_directly(ast, name)
}
