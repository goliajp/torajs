//! Which single operations may release a slot another binding still
//! borrows from, or run user code that may.

use crate::ast::accessor_lookup::{getter_fn_names, getter_may_run};
use crate::ast::{Expr, ExprId};
use crate::check::Type as CT;
use crate::ssa::Type;

use super::scan::Scan;

impl Scan<'_, '_> {
    pub(super) fn ty(&self, e: ExprId) -> Option<&CT> {
        self.env.types.get(&e)
    }

    fn field(&self, obj: ExprId, name: &str) -> Option<Type> {
        (self.env.field_slot)(obj, name)
    }

    /// Whether a store to `target` may release a value some borrow
    /// still names. A number or boolean slot holds nothing; a binding
    /// whose declared type cannot reach other cells (a string, a bigint)
    /// can only release itself, which matters when it is the root.
    pub(super) fn store_releases(&self, target: ExprId) -> bool {
        match self.env.ast.get_expr(target) {
            Expr::Ident(n) => {
                self.root.as_deref() == Some(n.as_str()) || !self.env.leaf_names.contains(n)
            }
            Expr::Member { obj, name } => self.field(*obj, name).is_none_or(|t| !t.is_copy()),
            Expr::Index { obj, .. } => {
                !matches!(self.ty(*obj), Some(CT::Array(elem)) if matches!(**elem, CT::Number | CT::Boolean))
            }
            _ => true,
        }
    }

    /// An operand no conversion can reach user code through.
    pub(super) fn primitive_operand(&self, e: ExprId) -> bool {
        match self.env.ast.get_expr(e) {
            Expr::Number(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::BigInt { .. }
            | Expr::Regex { .. } => return true,
            Expr::Member { obj, name } => {
                if let Some(t) = self.field(*obj, name) {
                    return t.is_copy() || matches!(t, Type::Str | Type::Substr);
                }
            }
            Expr::Ident(n) if self.env.leaf_names.contains(n) => return true,
            _ => {}
        }
        matches!(
            self.ty(e),
            Some(CT::Number | CT::String | CT::Boolean | CT::BigInt | CT::Null | CT::Undefined)
        )
    }

    /// An array or a string: read by index, walked without an iterator.
    pub(super) fn indexable(&self, e: ExprId) -> bool {
        if let Expr::Member { obj, name } = self.env.ast.get_expr(e)
            && let Some(t) = self.field(*obj, name)
        {
            return matches!(t, Type::Arr(_) | Type::Str | Type::Substr);
        }
        matches!(self.ty(e), Some(CT::Array(_) | CT::String))
    }

    /// A for-of whose source is an array or a string walks it by index;
    /// anything else steps an iterator, which runs user code.
    pub(super) fn indexable_source(&self, elem_expr: ExprId) -> bool {
        match self.env.ast.get_expr(elem_expr) {
            Expr::Index { obj, .. } => self.indexable(*obj),
            _ => false,
        }
    }

    /// A member read runs user code through a getter. A data field of a
    /// known class layout, a built-in receiver, and a closed literal
    /// shape no getter is declared for cannot; a class instance can only
    /// through a getter its class, an ancestor or a descendant declares
    /// (the instance may be wearing the descendant's); an untyped
    /// receiver may hold anything (a `defineProperty` getter, a proxy).
    pub(super) fn read_may_run_getter(&self, member: ExprId, name: &str) -> bool {
        let ast = self.env.ast;
        let (Expr::Member { obj, .. } | Expr::OptChain { obj, .. }) = ast.get_expr(member) else {
            return true;
        };
        if self.field(*obj, name).is_some() {
            return false;
        }
        let own_recv = match ast.get_expr(*obj) {
            Expr::This => true,
            Expr::Ident(n) => n == "__this",
            _ => false,
        };
        if own_recv {
            return !getter_fn_names(ast, name).is_empty();
        }
        match self.ty(*obj) {
            Some(
                CT::String
                | CT::Number
                | CT::Boolean
                | CT::BigInt
                | CT::Array(_)
                | CT::Map(..)
                | CT::Set
                | CT::Object(_)
                | CT::Function(..),
            ) => false,
            Some(CT::ClassRef(c)) => getter_may_run(ast, c, name),
            Some(CT::Struct(_)) => !getter_fn_names(ast, name).is_empty(),
            _ => true,
        }
    }

    pub(super) fn primitive(&self, e: ExprId) -> bool {
        self.primitive_operand(e)
    }

    /// Calls that run no user code and release nothing: the printing
    /// and `Math` namespaces, methods on primitive receivers fed
    /// primitive arguments, the non-shrinking array reads and appends,
    /// and constructors of built-in classes fed primitive arguments —
    /// each unless the program writes that very method or static.
    pub(super) fn call_is_inert(&self, e: ExprId) -> bool {
        let ast = self.env.ast;
        match ast.get_expr(e) {
            Expr::New {
                class_name, args, ..
            } => {
                !ast.class_parents.contains_key(class_name)
                    && args.iter().all(|a| self.primitive(*a))
            }
            Expr::Call { callee, args } => {
                let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                    return false;
                };
                let all_prim = args.iter().all(|a| self.primitive(*a));
                let shadow = self.env.shadow;
                let (family, inert) = match self.ty(*obj) {
                    Some(CT::Object(ns @ ("console" | "Math"))) => {
                        return (*ns == "console" || all_prim) && !shadow.writes_static(ns, name);
                    }
                    // these defer to a well-known-symbol hook on their
                    // pattern argument
                    Some(CT::String) => {
                        let hook = match name.as_str() {
                            "split" => Some("split"),
                            "replace" | "replaceAll" => Some("replace"),
                            "match" => Some("match"),
                            "matchAll" => Some("matchAll"),
                            "search" => Some("search"),
                            _ => None,
                        };
                        (
                            "String",
                            all_prim && !hook.is_some_and(|h| shadow.hook_written(h)),
                        )
                    }
                    Some(CT::Number) => ("Number", all_prim),
                    Some(CT::Boolean) => ("Boolean", all_prim),
                    Some(CT::Array(elem)) => (
                        "Array",
                        match name.as_str() {
                            "push" => !self.env.appends_move,
                            "at" | "slice" | "indexOf" | "lastIndexOf" | "includes" => true,
                            "join" => matches!(**elem, CT::Number | CT::String | CT::Boolean),
                            _ => false,
                        },
                    ),
                    Some(CT::Map(..)) => ("Map", matches!(name.as_str(), "get" | "has")),
                    Some(CT::Set) => ("Set", matches!(name.as_str(), "has" | "add")),
                    _ => return false,
                };
                // a written prototype method is what the call reaches
                inert && !shadow.shadows(family, name)
            }
            _ => false,
        }
    }
}
