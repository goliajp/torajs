//! Generator function-expression markers — carved out of `ast.rs`
//! (file-size hard limit) when RFC 20260717-fnexpr-this-channel knife
//! 1 grew the marker-registry face past 500 prod lines. Payload types
//! for `Ast::gen_fn_exprs`; the set itself stays on the `Ast` struct.

use super::{Ast, Expr, ExprId};

/// Which function-value expression form a `gen_fn_exprs` entry came
/// from. Only the two generator shapes hoist: plain `async
/// function(){}` expressions stay `Expr::ArrowFn` and ride the
/// closure lift (marked in `async_fn_value_exprs` instead), so no
/// `Async` variant exists here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenFnExprKind {
    Generator,
    AsyncGenerator,
}

/// Per-expression payload for `Ast::gen_fn_exprs`: the form kind plus
/// how many parser-synthesized destructuring lets prefix the body
/// (mirrors `gen_param_destr_prefix` for decl-form generators —
/// `hoist_gen_fn_exprs` re-registers the count under the hoisted name
/// so the __Gen ctor gets the eager destructure).
#[derive(Debug, Clone, Copy)]
pub struct GenFnExprInfo {
    pub kind: GenFnExprKind,
    pub destr_prefix: usize,
}

/// RFC 20260912 — a generator expression whose body captures an
/// enclosing local BY REFERENCE. The wrapper closure the hoist mints
/// (`(p) => __genexpr_N(<cell>, p)`) captures those names like any
/// closure — the env holds their capture boxes — and hands its own
/// cell to the factory, which parks it in an iterator field. Each
/// iterator method reads the names back out of that env (the closure
/// preamble, sourced from the field) instead of from a snapshot.
#[derive(Debug, Clone)]
pub struct GenEnvInfo {
    /// The wrapper's arena slot: an `Expr::Closure` once lifted, whose
    /// `fn_name` keys the construction-site capture layout.
    pub wrapper: ExprId,
    /// The factory's leading param, and so the iterator field, that
    /// carries the wrapper's own cell.
    pub cell_field: String,
    /// The captured names the body reads and writes through the env.
    pub names: Vec<String>,
    /// The iterator class (`__Gen_<factory>`), filled when the methods
    /// register: the `__cmany_` twin's receiver is `any`, and the
    /// lowering finds the field layout by this name.
    pub class: String,
}

impl GenEnvInfo {
    /// The wrapper's lifted fn name — the key of every capture table.
    pub fn wrapper_fn_name(&self, ast: &Ast) -> String {
        match ast.get_expr(self.wrapper) {
            Expr::Closure { fn_name, .. } => fn_name.clone(),
            other => panic!(
                "gen-env wrapper slot {:?} is not a lifted closure: {other:?} (lift_arrow_fns \
                 must run before any reader of gen_env_methods)",
                self.wrapper
            ),
        }
    }
}

/// `desugar_generators` hook: the three iterator methods of a
/// generator with a by-reference capture record join
/// `gen_env_methods`, keyed the way `desugar_classes` will name them.
pub(super) fn register_gen_env_methods(
    ast: &mut Ast,
    gen_name: &str,
    class_name: &str,
    is_async_gen: bool,
) {
    if let Some(mut info) = ast.gen_env_captures.get(gen_name).cloned() {
        info.class = class_name.to_string();
        // D4 — an async generator's machine methods carry the
        // `__step*` names; the public trio only forwards.
        let names: [&str; 3] = if is_async_gen {
            [
                super::desugar_generators_async::MACHINE_NEXT,
                super::desugar_generators_async::MACHINE_RETURN,
                super::desugar_generators_async::MACHINE_THROW,
            ]
        } else {
            ["next", "return", "throw"]
        };
        for m in names {
            // The any-receiver twin `desugar_classes` mints for every
            // method (`__cmany_` + the rest of the name) carries the
            // same body and runs when the iterator is reached through
            // an `any` value — which is how a generator expression's
            // result is always typed.
            ast.gen_env_methods
                .insert(format!("__cm_{class_name}__{m}"), info.clone());
            ast.gen_env_methods
                .insert(format!("__cmany_{class_name}__{m}"), info.clone());
        }
    }
}

/// Wrapper lifted-fn name → the iterator methods that read its env.
/// A wrapper not yet lifted has no fn name to be asked about, so it
/// is simply absent: the assigned-name collector this feeds runs from
/// an AST pass before `lift_arrow_fns` too (`fndecl_rebound`), where
/// no lifted name exists for it to match.
pub fn gen_env_methods_by_wrapper(ast: &Ast) -> std::collections::HashMap<String, Vec<String>> {
    let mut out: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (m, info) in &ast.gen_env_methods {
        if let Expr::Closure { fn_name, .. } = ast.get_expr(info.wrapper) {
            out.entry(fn_name.clone()).or_default().push(m.clone());
        }
    }
    out
}
