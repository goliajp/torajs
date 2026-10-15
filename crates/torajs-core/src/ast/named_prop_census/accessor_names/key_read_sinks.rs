//! Computed-key reads whose value cannot be handed out as a definer.
//!
//! A computed read on a dictionary or an `any` receiver can answer an
//! inherited `Object.prototype` member — `__defineGetter__` is one — and
//! a definer held as a value may later be called on any receiver with
//! any key. That needs the value to reach a call. A read whose every use
//! stores into it, or feeds an operator that only converts or compares
//! its operand (`(d[k] ?? 0) + 1`, `d[k] === v`, `typeof d[k]`), hands
//! the value to nothing but §7.1.1 ToPrimitive, whose only program code
//! is a `valueOf` / `toString` / `@@toPrimitive` the program installs on
//! a prototype. A program that may install one keeps every such read
//! open.

use std::collections::HashMap;

use crate::ast::{Ast, BinOp, Expr, ExprId};

/// The names ToPrimitive reads off a function's prototype chain, and
/// the one that replaces the chain.
const HOOKS: &[&str] = &["valueOf", "toString", "__proto__"];

/// The answers [`super::AccessorFieldNames::key_reads_open`] asks of
/// the program, built once.
pub(super) struct KeyReadSinks<'a> {
    ast: &'a Ast,
    /// The expressions each expression is a direct child of.
    parents: Vec<Vec<ExprId>>,
    /// The program may install a ToPrimitive hook.
    hooks: bool,
}

impl<'a> KeyReadSinks<'a> {
    pub(super) fn of(
        ast: &'a Ast,
        expr_types: &HashMap<ExprId, crate::check::Type>,
        hook_named: &dyn Fn(&str) -> bool,
    ) -> Self {
        let mut parents = vec![Vec::new(); ast.exprs.len()];
        for i in 0..ast.exprs.len() {
            let p = ExprId(i as u32);
            crate::ast::desugar_async_sm_walk::visit_expr_children(ast, p, &mut |c| {
                parents[c.0 as usize].push(p)
            });
        }
        let hooks = HOOKS.iter().any(|h| hook_named(h)) || may_install_hook(ast, expr_types);
        KeyReadSinks {
            ast,
            parents,
            hooks,
        }
    }

    /// Whether the read `r` can never hand its value to a call.
    pub(super) fn closes(&self, r: ExprId) -> bool {
        !self.hooks && self.consumed(r)
    }

    /// Every use of `e` stores into it or converts its value; a use
    /// that forwards the value (`??`, `&&`, a branch of `?:`, `as`) is
    /// judged by its own uses. An expression with no parent expression
    /// is a statement's, which may bind it.
    fn consumed(&self, e: ExprId) -> bool {
        let ps = &self.parents[e.0 as usize];
        !ps.is_empty() && ps.iter().all(|p| self.use_consumes(*p, e))
    }

    fn use_consumes(&self, p: ExprId, e: ExprId) -> bool {
        match self.ast.get_expr(p) {
            Expr::Assign { target, value } => *target == e && *value != e,
            Expr::BinOp {
                op: BinOp::LAnd | BinOp::LOr,
                ..
            }
            | Expr::Nullish { .. }
            | Expr::As { .. } => self.consumed(p),
            Expr::BinOp { .. } | Expr::Unary { .. } | Expr::TypeOf { .. } => true,
            Expr::PostIncr { .. } => true,
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => (*then_branch != e && *else_branch != e) || (*cond != e && self.consumed(p)),
            Expr::Sequence { left, right } => *right != e || (*left != e && self.consumed(p)),
            _ => false,
        }
    }
}

/// Whether a write of the program may put a `valueOf`, a `toString` or
/// an `@@toPrimitive` on a prototype, or replace one's prototype: a
/// store of a value that may be an object under one of those names, or
/// under a key computed at run time into a receiver that may be a
/// prototype, or the built-ins that write or re-parent through their
/// arguments (`Object.assign`, `Reflect.set`, `setPrototypeOf`), or a
/// read that can hand out the `__proto__` setter. A primitive is no
/// hook and no prototype. A definer call is the
/// caller's census.
fn may_install_hook(ast: &Ast, expr_types: &HashMap<ExprId, crate::check::Type>) -> bool {
    let origins = std::cell::OnceCell::new();
    let origins =
        || origins.get_or_init(|| super::super::receiver_origin::Origins::of(ast, expr_types));
    ast.exprs.iter().any(|e| match e {
        Expr::Assign { target, value } if !primitive(ast, expr_types, *value) => {
            match ast.get_expr(*target) {
                Expr::Member { name, .. } => HOOKS.contains(&name.as_str()),
                Expr::Index { obj, index } => match ast.get_expr(*index) {
                    Expr::String(s) => s.as_str().is_none_or(|s| HOOKS.contains(&s)),
                    Expr::Number(_) => false,
                    _ => !origins().holds_no_slot(*obj),
                },
                _ => false,
            }
        }
        Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
            writer(ast, *obj, name.as_str())
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => match ast.get_expr(*index) {
            Expr::String(s) => s.as_str().is_some_and(|s| writer(ast, *obj, s)),
            _ => false,
        },
        _ => false,
    })
}

/// Whether `v` is a primitive whatever its operands hold: its type says
/// so, or its operator answers one (every operator but `&&` / `||`).
fn primitive(ast: &Ast, expr_types: &HashMap<ExprId, crate::check::Type>, v: ExprId) -> bool {
    use crate::check::Type;
    matches!(
        expr_types.get(&v),
        Some(Type::Number | Type::String | Type::Boolean | Type::Literal(_))
    ) || match ast.get_expr(v) {
        Expr::BinOp { op, .. } => !matches!(op, BinOp::LAnd | BinOp::LOr),
        Expr::Unary { .. }
        | Expr::TypeOf { .. }
        | Expr::PostIncr { .. }
        | Expr::Number(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null => true,
        _ => false,
    }
}

/// `Object.assign`, `Object.setPrototypeOf`, `Reflect.set` and
/// `Reflect.setPrototypeOf`, read in any position, and on any base a
/// read of `getOwnPropertyDescriptor(s)` or `__lookupSetter__`, which
/// can hand out the `__proto__` setter. `Object` or `Reflect` held any
/// other way already reaches every name.
fn writer(ast: &Ast, obj: ExprId, name: &str) -> bool {
    // the `__proto__` accessor's setter, once taken out, re-parents any
    // object it is called on with no assignment the walk could see
    if matches!(
        name,
        "getOwnPropertyDescriptor" | "getOwnPropertyDescriptors" | "__lookupSetter__"
    ) {
        return true;
    }
    match ast.get_expr(obj) {
        Expr::Ident(b) => matches!(
            (b.as_str(), name),
            ("Object", "assign" | "setPrototypeOf") | ("Reflect", "set" | "setPrototypeOf")
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::ast::{Ast, Expr, ExprId};
    use crate::check::Type;

    /// Whether the computed reads of `src` stay open, with `d` a
    /// `Record<string, number>`, `k` / `w` strings and `z` an `any`.
    fn open(src: &str) -> bool {
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        let mut ast = crate::parser::parse(src, &tokens).expect("parse");
        super::super::census(&mut ast);
        let dict = Type::Dict(Box::new(Type::String), Box::new(Type::Number));
        let types: HashMap<ExprId, Type> = (ast.exprs.iter().enumerate())
            .filter_map(|(i, e)| {
                let t = match e {
                    Expr::Ident(n) if n == "d" => dict.clone(),
                    Expr::Ident(n) if n == "k" || n == "w" => Type::String,
                    Expr::Ident(n) if n == "z" => Type::Any,
                    Expr::Number(_) => Type::Number,
                    _ => return None,
                };
                Some((ExprId(i as u32), t))
            })
            .collect();
        let ast: &Ast = &ast;
        ast.accessor_field_names.key_reads_open(ast, &types)
    }

    #[test]
    fn a_read_that_only_meets_a_conversion_or_a_store_stays_closed() {
        for src in [
            "d[k] = (d[k] ?? 0) + 1;",
            "s = s + d[k]; t = typeof d[k]; u = d[k] === 1; v = -d[k];",
            "s = (c ? d[k] : 0) * 2; t = (d[k] || 1) - 1; u = (x, d[k]) < 3;",
            "d[k]++; s = s + (d[k] as number);",
            "const z: any = {}; z[w] = h; s = d[k] + 1;",
            "s = d[k] + 1; F.prototype.toString = 5; F.prototype[w] = d[k] + 1;",
        ] {
            assert!(!open(src), "{src}");
        }
    }

    #[test]
    fn a_read_whose_value_can_reach_a_call_stays_open() {
        for src in [
            "f(d[k]);",
            "const g = d[k];",
            "s = d[k] ?? 0;",
            "x.y = (c ? d[k] : 0);",
            "s = (d[k] ?? f)(1);",
        ] {
            assert!(open(src), "{src}");
        }
    }

    #[test]
    fn a_program_that_may_install_a_to_primitive_hook_keeps_them_open() {
        for src in [
            "s = d[k] + 1; F.prototype.toString = h;",
            "s = d[k] + 1; F.prototype[\"valueOf\"] = h;",
            "s = d[k] + 1; F.prototype.__proto__ = h;",
            "s = d[k] + 1; F.prototype[Symbol.toPrimitive] = h;",
            "s = d[k] + 1; Object.assign(F.prototype, h);",
            "s = d[k] + 1; Reflect.setPrototypeOf(F.prototype, h);",
            "s = d[k] + 1; const a = Object.setPrototypeOf;",
            "s = d[k] + 1; z[w] = h;",
        ] {
            assert!(open(src), "{src}");
        }
    }
}
