//! What a target name asks of `instanceof`, from the program alone:
//! whether the §13.10.2 operator decides instead of a fold, and whether
//! a folded builtin walks the operand's prototype chain at run time.
//! The lowering and the throw analysis (`ast_throw_info`) ask the same
//! questions, so the answers live once, here.

use crate::ast::{Ast, Expr, ExprId};

use super::builtin_proto_tag;

/// Whether `x instanceof N`, `N` a name the function binds no local
/// to, asks the §13.10.2 operator rather than a fold; `hook` is whether
/// a value in the module may carry a `Symbol.hasInstance` handler. The
/// operator can throw, a fold cannot, so the throw analysis asks this
/// too (`ast_throw_info`).
pub(crate) fn name_runs_operator(ast: &Ast, class_name: &str, hook: bool) -> bool {
    // A unit class names the evaluation its declaration left in scope
    // (`ast::class_unit`): the tag fold would say yes to an instance of
    // any evaluation, so the operator decides against this one's
    // prototype.
    if ast.eval_unit_classes.contains(class_name) {
        return true;
    }
    if ast.class_parents.contains_key(class_name) {
        // A handler the class body declares is one spelling; a
        // `defineProperty(C, Symbol.hasInstance, …)` is the other, and
        // no class-shaped question sees it. A module that installs one
        // anywhere routes every declared-class target here — the same
        // whole-program posture the iteration lanes take, and the
        // runtime lane falls through to OrdinaryHasInstance when the
        // target carries no handler after all.
        return class_declares_has_instance(ast, class_name) || hook;
    }
    builtin_proto_tag(class_name).is_none()
        && !matches!(class_name, "Object" | "Iterator" | "BigInt" | "Symbol")
}

/// Does this class — or anything it extends — write a member under
/// the computed key `[Symbol.hasInstance]`? The parser folds only
/// `Symbol.iterator` to a synthetic name; every other `Symbol.<x>`
/// member key is stashed as the key expression itself, so the test
/// is on its shape.
///
/// The walk up `class_parents` is what makes `x instanceof Sub`
/// answer through a handler `Sub` inherits: a derived constructor's
/// [[Prototype]] is its base, so GetMethod finds the base's handler
/// even though the subclass declares nothing. The depth cap mirrors
/// [`super::compute_descendant_tags`] — a malformed cycle in the
/// side table must not hang the compiler.
fn class_declares_has_instance(ast: &Ast, class_name: &str) -> bool {
    let mut cur = Some(class_name.to_string());
    let mut depth = 0u32;
    while let Some(name) = cur {
        if depth > 64 {
            return false;
        }
        if (ast.class_computed_keys.iter())
            .any(|((c, _), &key)| *c == name && is_symbol_has_instance(ast, key))
        {
            return true;
        }
        cur = ast.class_parents.get(&name).and_then(|p| p.clone());
        depth += 1;
    }
    false
}

fn is_symbol_has_instance(ast: &Ast, key: ExprId) -> bool {
    let Expr::Member { obj, name } = ast.get_expr(key) else {
        return false;
    };
    name == "hasInstance" && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Symbol")
}

/// Whether `x instanceof N`, the builtin constructor `N` folded by
/// name, walks the prototype chain of an object `x` at run time, where
/// a Proxy's `getPrototypeOf` trap can throw.
pub(crate) fn name_walks_proto_chain(class_name: &str) -> bool {
    crate::ssa_lower_call_typedarray::kind_of_name(class_name).is_none()
        && (builtin_proto_tag(class_name).is_some() || class_name == "Iterator")
}
