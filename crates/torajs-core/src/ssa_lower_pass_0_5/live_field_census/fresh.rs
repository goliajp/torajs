//! Calls whose answer is live by construction although no declared
//! function of the program returns it: a builtin method that answers a
//! fresh object, and a method call that dispatches over several owners.

use super::names::HashMap;

use crate::ast::{BinOp, Expr, ExprId};
use crate::check::Type;

use super::Census;
use super::walk::Few;

/// Array methods that answer a new array or iterator, never an element.
const ARRAY_FRESH: &[&str] = &[
    "map",
    "filter",
    "slice",
    "concat",
    "flat",
    "flatMap",
    "toSorted",
    "toReversed",
    "toSpliced",
    "with",
    "splice",
    "keys",
    "values",
    "entries",
];

impl<'a> Census<'a> {
    /// `callee` is a builtin method of a typed receiver, or a static
    /// builtin, that answers a fresh object: a new array, a split, a
    /// derived promise. The receiver's own liveness does not matter: a
    /// nullish one raises before the call answers.
    pub(super) fn fresh_builtin(&self, callee: ExprId) -> bool {
        let Expr::Member { obj, name } = self.ast.get_expr(callee) else {
            return false;
        };
        let name = name.as_str();
        // a store to a property of that name may replace the builtin
        if self.member_writes.contains_key(name)
            || self.foreign.contains(name)
            || self.deleted.contains(name)
        {
            return false;
        }
        match self.ty(*obj) {
            Some(Type::Array(_)) => return ARRAY_FRESH.contains(&name),
            Some(Type::String) => return name == "split",
            Some(Type::Promise(_)) => return matches!(name, "then" | "catch" | "finally"),
            Some(Type::Map(..) | Type::Set) => {
                return matches!(name, "keys" | "values" | "entries");
            }
            _ => {}
        }
        let Expr::Ident(ns) = self.ast.get_expr(*obj) else {
            return false;
        };
        if self.shadowed(ns) || self.assigned.contains(ns.as_str()) {
            return false;
        }
        match ns.as_str() {
            "Promise" => matches!(
                name,
                "resolve" | "reject" | "all" | "allSettled" | "race" | "any"
            ),
            "Array" => matches!(name, "from" | "of"),
            "Object" => matches!(name, "keys" | "values" | "entries"),
            _ => false,
        }
    }

    /// A `+` one operand of which is a string, which concatenates
    /// whatever the other is.
    pub(super) fn concat(&self, e: ExprId) -> bool {
        let Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } = self.ast.get_expr(e)
        else {
            return false;
        };
        [left, right].iter().any(|o| {
            self.ty(**o) == Some(&Type::String) || (self.ty(**o).is_none() && self.concat(**o))
        })
    }

    /// `callee` is a builtin that answers a string.
    pub(super) fn string_builtin(&self, callee: ExprId) -> bool {
        let (obj, name) = match self.ast.get_expr(callee) {
            Expr::Member { obj, name } => (*obj, name.as_str()),
            Expr::Ident(n) => {
                return n == "String" && !self.shadowed(n) && !self.assigned.contains(n.as_str());
            }
            _ => return false,
        };
        if self.member_writes.contains_key(name) || self.foreign.contains(name) {
            return false;
        }
        match self.ty(obj) {
            Some(Type::String) => STRING_FRESH.contains(&name),
            Some(Type::Number) => matches!(
                name,
                "toString" | "toFixed" | "toPrecision" | "toExponential"
            ),
            Some(Type::Array(_)) => name == "join",
            _ => match self.ast.get_expr(obj) {
                Expr::Ident(ns) if ns == "String" && !self.shadowed(ns) => {
                    matches!(name, "fromCharCode" | "fromCodePoint")
                }
                _ => false,
            },
        }
    }

    pub(super) fn methods(&self) -> Methods<'a> {
        let mut out: Methods<'a> = HashMap::default();
        for f in self.fns.keys() {
            let Some((owner, m)) = f.strip_prefix("__cm_").and_then(|r| r.split_once("__")) else {
                continue;
            };
            let m = m.split("$$").next().unwrap_or(m);
            out.entry(m).or_default().push((owner, *f));
        }
        out
    }
}

/// String methods that answer a string, never `undefined`.
const STRING_FRESH: &[&str] = &[
    "slice",
    "substring",
    "substr",
    "toUpperCase",
    "toLowerCase",
    "toLocaleUpperCase",
    "toLocaleLowerCase",
    "trim",
    "trimStart",
    "trimEnd",
    "padStart",
    "padEnd",
    "repeat",
    "replace",
    "replaceAll",
    "concat",
    "normalize",
    "charAt",
    "toString",
    "valueOf",
];

/// The bodies of each instance method, as `(owner, function)`.
pub(super) type Methods<'a> = HashMap<&'a str, Few<(&'a str, &'a str)>>;
