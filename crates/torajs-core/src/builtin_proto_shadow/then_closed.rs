//! The `then` closure proof: can the program give some object a
//! callable `then` its type does not show? §27.2.1.3.2 steps 9-13
//! decide whether an awaited object is a thenable by a
//! `Get(resolution, "then")` at run time, so an expando `then`, one
//! reached through a replaced [[Prototype]] and one a proxy answers all
//! count, whatever type the value travels under.
//!
//! The proof is closed by default: it answers "closed" only when every
//! expression and statement of the program is a form it knows cannot
//! put a `then` on an object, and "open" for anything it does not
//! recognise. A program is closed when
//! - every property key it writes, defines or reads under a computed
//!   name is known: spelled as a literal, or typed by the checker as a
//!   number, a boolean, a symbol or a string literal type, and none of
//!   them is `then` or a key that reaches %Object.prototype%'s
//!   `__proto__` accessor or its Annex B definers (B.2.2);
//! - it spells no `then` at all: no string `"then"`, no binding,
//!   function, class member or export of that name, no write to a
//!   member named `then` (reading one, as `p.then(f)` does, is fine);
//! - no [[Prototype]] leaves the ordinary chain: no `__proto__`, no
//!   `Object.create` / `setPrototypeOf`, no read of a builtin's
//!   `.prototype`, no write of any `.prototype`, no class whose heritage
//!   is a value, `Promise` or `Function`;
//! - it names no form that answers any key or runs code from a string:
//!   `Reflect`, `Proxy`, `eval`, `Function`, the global object, `with`,
//!   a dynamic `import()`, `require`, a `constructor` read (a function's
//!   is %Function%);
//! - no `as` cast hands an `any` to an object type: the value under it
//!   may be a promise, or an object of another layout;
//! - it reaches `Object`, `JSON` and `Promise` only through statics that
//!   define nothing (`Object.keys`, `JSON.parse` called without a
//!   reviver, `Promise.resolve`), and the compiler's own helpers only through
//!   the ones known to define nothing a key can name.
//!
//! Copying own properties (spread, `Object.assign` is not even allowed)
//! moves only keys some object already has, so under these rules no
//! object ever holds a `then`. A promise cell under a type that lies
//! some other way (an inferred slot the checker widened past an `any`)
//! is the one thenable left, and the lowering tells it apart by its
//! cell's tag.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Lit, Type};

mod forms;
mod run;
mod stmts;

#[cfg(test)]
mod tests;

use forms::{
    builtin_global, danger_key, danger_member, danger_string, helper_defines_nothing, ident_opens,
};
pub(crate) use run::ThenClosure;

/// Why the proof answered "open": the first form it could not clear.
#[derive(Debug)]
pub(crate) struct Opening {
    pub(crate) why: &'static str,
    pub(crate) at: String,
}

fn open(why: &'static str, at: impl Into<String>) -> Option<Opening> {
    Some(Opening { why, at: at.into() })
}

/// The proof, in one place. `types` are the checker's answers for the
/// keys of computed member accesses; without them (inside a checker
/// run) a computed key that is not a literal is left to the run's end,
/// where [`super::ShadowSet::needs_typed_then_run`] asks again with the
/// run's types.
pub(crate) fn then_opening(ast: &Ast, types: Option<&HashMap<ExprId, Type>>) -> Option<Opening> {
    if !ast.class_computed_keys.is_empty() || !ast.objlit_computed_keys.is_empty() {
        return open("computed member key", "");
    }
    if let Some(p) =
        (ast.builtin_class_parents.values()).find(|p| matches!(p.as_str(), "Promise" | "Function"))
    {
        return open("builtin heritage", p.clone());
    }
    if !ast.es5_value_parents.is_empty() || ast.program_reparents {
        return open("replaced prototype", "");
    }
    if ast.has_with_stmt || ast.dyn_import_present {
        return open("with or dynamic import", "");
    }
    let vetted = forms::vetted_roots(ast);
    for (i, e) in ast.exprs.iter().enumerate() {
        let eid = ExprId(i as u32);
        if let Some(o) = expr_opening(ast, eid, e, &vetted, types) {
            return Some(o);
        }
    }
    ast.stmts.iter().find_map(stmts::stmt_opening)
}

/// One expression, every variant named: a new one must be judged here.
fn expr_opening(
    ast: &Ast,
    eid: ExprId,
    e: &Expr,
    vetted: &crate::ssa::id_hash::HashSet<ExprId>,
    types: Option<&HashMap<ExprId, Type>>,
) -> Option<Opening> {
    match e {
        Expr::Ident(n) => {
            // `new C(…)` reaches here as the class lowering's factory
            let ctor = n.strip_prefix("__new_").is_some_and(ident_opens);
            if ctor || ident_opens(n) || (forms::gated_root(n) && !vetted.contains(&eid)) {
                return open("global", n.clone());
            }
            if let Some(h) = n.strip_prefix("__torajs_")
                && !helper_defines_nothing(h)
            {
                return open("helper", n.clone());
            }
            None
        }
        Expr::String(s) => match s.as_wtf8().as_str() {
            Some(k) if danger_string(k) => open("string", k),
            _ => None,
        },
        Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
            if danger_member(name) {
                return open("member", name.clone());
            }
            match ast.get_expr(*obj) {
                Expr::Ident(n) if name == "prototype" && builtin_global(n) => {
                    open("builtin prototype", n.clone())
                }
                _ => None,
            }
        }
        Expr::Index { index, .. } | Expr::OptIndex { index, .. } => key_opening(ast, *index, types),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            call_opening(ast, *callee, args)
        }
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
            match ast.get_expr(*target) {
                Expr::Member { name, .. } | Expr::OptChain { name, .. }
                    if matches!(name.as_str(), "then" | "prototype") =>
                {
                    open("write", name.clone())
                }
                _ => None,
            }
        }
        Expr::ObjectLit { fields } => fields.iter().find_map(|(k, _)| {
            let k = k.lossy();
            let bare = (k.strip_prefix("__getter_"))
                .or_else(|| k.strip_prefix("__setter_"))
                .unwrap_or(&k);
            (bare == "then" || bare == "__proto__" || k.starts_with("__computed_")).then(|| {
                Opening {
                    why: "literal member",
                    at: k.to_string(),
                }
            })
        }),
        Expr::New { class_name, .. } if ident_opens(class_name) => open("new", class_name.clone()),
        // a promise or a value of another layout may travel under the
        // object type an `as` of an `any` names
        Expr::As { expr, ty_ann } => match types {
            Some(t)
                if t.get(expr) == Some(&Type::Any)
                    && t.get(&eid)
                        .is_some_and(crate::check::awaited::settles_as_object) =>
            {
                open("cast from any", ty_ann.clone())
            }
            _ => None,
        },
        Expr::New { .. }
        | Expr::NewDynamic { .. }
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::BinOp { .. }
        | Expr::Unary { .. }
        | Expr::Array(_)
        | Expr::ArrowFn { .. }
        | Expr::Closure { .. }
        | Expr::This
        | Expr::NewTarget
        | Expr::Super { .. }
        | Expr::Ternary { .. }
        | Expr::Sequence { .. }
        | Expr::Elision
        | Expr::TypeOf { .. }
        | Expr::Delete { .. }
        | Expr::InstanceOf { .. }
        | Expr::Spread { .. }
        | Expr::Nullish { .. } => None,
    }
}

/// A computed member key: a literal, or a key the checker typed as one
/// that cannot name a dangerous property.
fn key_opening(ast: &Ast, index: ExprId, types: Option<&HashMap<ExprId, Type>>) -> Option<Opening> {
    match ast.get_expr(index) {
        Expr::Number(_) | Expr::Bool(_) | Expr::BigInt { .. } => None,
        Expr::String(s) => match s.as_wtf8().as_str() {
            Some(k) if !danger_key(k) => None,
            _ => open("computed key", s.to_string_lossy_owned()),
        },
        _ => {
            let types = types?;
            match types.get(&index) {
                Some(t) if key_type_known(t) => None,
                _ => open("computed key", ""),
            }
        }
    }
}

/// A key of type `t` is a number, a boolean, a symbol, or a string
/// literal that names no dangerous property.
fn key_type_known(t: &Type) -> bool {
    match t {
        Type::Number | Type::BigInt | Type::Symbol | Type::Boolean => true,
        Type::Literal(Lit::Str(s)) => s.as_wtf8().as_str().is_some_and(|k| !danger_key(k)),
        Type::Literal(Lit::Num(_) | Lit::Bool(_)) => true,
        Type::Union(members) => members.iter().all(key_type_known),
        _ => false,
    }
}

/// A call whose callee the walk must read with its arguments: the
/// `super` property kernels take their key as the second argument.
fn call_opening(ast: &Ast, callee: ExprId, args: &[ExprId]) -> Option<Opening> {
    match ast.get_expr(callee) {
        Expr::Ident(n) if n.starts_with("__torajs_super_prop_") => {
            match args.get(1).map(|&k| ast.get_expr(k)) {
                Some(Expr::String(s)) if s.as_wtf8().as_str().is_some_and(|k| !danger_key(k)) => {
                    None
                }
                _ => open("super key", n.clone()),
            }
        }
        _ => None,
    }
}
