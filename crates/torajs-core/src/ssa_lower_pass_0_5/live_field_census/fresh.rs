//! Calls whose answer is live by construction although no declared
//! function of the program returns it: a builtin method that answers a
//! fresh object, and a method call that dispatches over several owners.

use crate::ssa::id_hash::HashMap;

use crate::ast::{BinOp, Expr, ExprId};
use crate::check::Type;

use super::Census;
use super::judge::{State, Visiting};
use super::walk::derives_from;

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

impl Census<'_> {
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

impl Census<'_> {
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
    fn string_builtin(&self, callee: ExprId) -> bool {
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
}

impl<'b> State<'_, 'b> {
    /// A string-typed value is never nullish: a literal, a `+` that
    /// concatenates, a `typeof`, a string builtin's answer, or a read
    /// or call judged as any other value is.
    pub(super) fn string_live(&self, e: ExprId, visiting: &mut Visiting<'b>) -> bool {
        match self.c.ast.get_expr(e) {
            Expr::String(_) | Expr::TypeOf { .. } => true,
            // a string element read the lowering tests where it is read,
            // and trusts once bound, as it did before this census
            Expr::Index { .. } => true,
            Expr::BinOp { op: BinOp::Add, .. } => true,
            Expr::Ident(n) => self.read_live(self.c.scopes.owner(e), n, visiting),
            Expr::As { expr, .. } => {
                self.c.ty(*expr) == Some(&Type::String) && self.value_live(*expr, visiting)
            }
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => self.value_live(*then_branch, visiting) && self.value_live(*else_branch, visiting),
            Expr::Call { callee, .. } => match self.c.ast.get_expr(*callee) {
                Expr::Ident(f) if self.c.fns.contains_key(f.as_str()) => {
                    self.call_live(self.c.retargets.get(&e).unwrap_or(f))
                }
                _ => self.c.string_builtin(*callee),
            },
            _ => false,
        }
    }

    /// A call of the declared function `f` answers a live value: a
    /// factory, a function all of whose returns are live, or a method
    /// dispatch stub every owner's body of which is.
    pub(super) fn call_live(&self, f: &str) -> bool {
        // the runtime's PromiseResolve answers a promise
        if super::walk::factory_class(f).is_some() || f == "__torajs_await_resolve" {
            return true;
        }
        if self.c.shadowed(f) {
            return false;
        }
        // a global called as a function answers a value of its kind
        if !self.c.fns.contains_key(f) && !self.c.assigned.contains(f) {
            return matches!(f, "Symbol" | "BigInt" | "Array" | "Object" | "RegExp");
        }
        let Some(m) = f.strip_prefix("__dispatch_") else {
            return self.live_fns.contains(f);
        };
        // the stub forwards to one owner; the slot reaches them all
        let (bare, suffix) = match m.split_once("$$") {
            Some((b, _)) => (b, &m[b.len()..]),
            None => (m, ""),
        };
        let owners = self.c.ast.method_owners.get(bare);
        owners.is_some_and(|os| {
            !os.is_empty()
                && os.iter().all(|o| {
                    let bodies = [
                        format!("__cm_{o}__{bare}{suffix}"),
                        format!("__cm_{o}__{bare}"),
                    ];
                    let mut found = bodies
                        .iter()
                        .filter(|b| self.c.fns.contains_key(b.as_str()));
                    let first = found.next();
                    first.is_some_and(|b| self.live_fns.contains(b.as_str()))
                        && found.all(|b| self.live_fns.contains(b.as_str()))
                })
        })
    }

    /// A call of method `m` off `obj`, a class instance, answers a
    /// live value: every body of the method the receiver's class, an
    /// ancestor or a subclass declares is live, and nothing else can
    /// stand in the slot (a property store, a field of the name).
    pub(super) fn method_live(&self, obj: ExprId, m: &str) -> bool {
        self.method_class(obj)
            .is_some_and(|class| self.methods_live(class, m))
    }

    /// The class of an instance receiver; none for a receiver that
    /// names a class (a static call).
    pub(super) fn method_class(&self, obj: ExprId) -> Option<&'b str> {
        let c = self.c;
        let Some(Type::ClassRef(class)) = c.ty(obj) else {
            return None;
        };
        let names_class =
            matches!(c.ast.get_expr(obj), Expr::Ident(n) if c.ast.class_parents.contains_key(n));
        (!names_class).then(|| class.split('<').next().unwrap_or(class))
    }

    /// Every body of `m` the class, an ancestor or a subclass declares
    /// is live, and nothing else can stand in the slot.
    pub(super) fn methods_live(&self, class: &str, m: &str) -> bool {
        let c = self.c;
        if !self.fields_open
            || c.foreign_classes.contains(class)
            || c.member_writes.contains_key(m)
            || c.foreign.contains(m)
            || c.deleted.contains(m)
            || c.prefix.iter().any(|w| w.field == m)
        {
            return false;
        }
        let mut found = false;
        for (o, f) in self.methods.get(m).into_iter().flatten() {
            if *o == class || derives_from(c.ast, o, class) || derives_from(c.ast, class, o) {
                if !self.live_fns.contains(*f) {
                    return false;
                }
                found = true;
            }
        }
        found
    }
}

/// The bodies of each instance method, as `(owner, function)`.
pub(super) type Methods<'a> = HashMap<&'a str, Vec<(&'a str, &'a str)>>;

impl<'a> Census<'a> {
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
