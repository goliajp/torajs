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
//!   name is spelled: a literal, or literals the walk folds into one
//!   (`"a" + "b"`, `-1`), and none of them is `then` or a key that
//!   reaches %Object.prototype%'s `__proto__` accessor or its Annex B
//!   definers (B.2.2). A key's type does not count: an `as` or an `any`
//!   puts a string built at run time under any type;
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
//!   is %Function%), and reads no such global's name, nor `Object`,
//!   `JSON` or `Promise`, off another object (the global object a sloppy
//!   function's `this` is);
//! - no `as` cast hands an `any` to an object type: the value under it
//!   may be a promise, or an object of another layout. A generic body's
//!   cast counts as each of its specializations' copies, so the proof
//!   under types is taken over the mono program as well;
//! - it reaches `Object`, `JSON` and `Promise` only through statics that
//!   define nothing and hand out no builtin prototype (`Object.keys`,
//!   `JSON.parse` called without a reviver or a spread, `Promise.resolve`),
//!   and the compiler's own helpers only through
//!   the ones known to define nothing a key can name.
//!
//! Copying own properties (spread, `Object.assign` is not even allowed)
//! moves only keys some object already has, so under these rules no
//! object ever holds a `then`. A promise cell under a type that lies
//! some other way (an inferred slot the checker widened past an `any`)
//! is the one thenable left, and the lowering tells it apart by its
//! cell's tag.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId};
use crate::check::Type;

mod exprs;
mod forms;
mod keys;
mod run;
mod stmts;

#[cfg(test)]
mod tests;

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
/// operands of `as` casts; without them (inside a checker run) a cast is
/// left to the run's end, where [`super::ShadowSet::needs_typed_then_run`]
/// asks again with the run's types, and again over the mono program
/// with its specializations' types (`check_entry::check_round`).
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
        if let Some(o) = exprs::expr_opening(ast, eid, e, &vetted, types) {
            return Some(o);
        }
    }
    ast.stmts.iter().find_map(stmts::stmt_opening)
}
