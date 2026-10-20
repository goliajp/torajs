//! Parameters a function reads a member of before it does anything a
//! caller could observe.
//!
//! A member read of a nullish value raises a TypeError (§13.3.2.1). When
//! it is the first thing the body evaluates that can be seen at all,
//! the caller can make the same test right before the call, once its
//! arguments are evaluated, and nobody can tell the difference: the
//! callee is never entered with a nullish value there, so its reads of
//! the parameter need no test of their own. The caller usually knows
//! more about the argument than the callee knows about the parameter.
//!
//! What may come first: declarations of locals from literals and plain
//! reads of bindings, and nested function declarations. The read itself
//! has to be the first evaluation of a condition, an initialiser, an
//! expression statement or a return, reached only through operands
//! that are plain reads or literals. Nothing runs between the call and
//! the body only when no parameter has an initialiser or a pattern
//! (§10.2.11 evaluates both before the body).
//!
//! A call tests each such argument the census does not prove live
//! there, whatever its shape, so every argument the callee reads is
//! tested or live.

use crate::ssa::id_hash::{HashMap, HashSet};

use crate::ast::{Ast, BinOp, Expr, ExprId, Param, Stmt};

use super::Census;
use super::bindings::holds_nullish_in_band;

enum First {
    /// A member or element read of the binding comes first.
    Deref(String),
    /// Nothing anyone can observe happens.
    Pure,
    /// Something else may happen first.
    Effect,
}

impl<'a> Census<'a> {
    /// The judged parameters a body reads a member of first, by function.
    pub(super) fn entry_checked(
        &self,
        judged: &HashSet<(&'a str, usize)>,
    ) -> HashMap<&'a str, Vec<usize>> {
        let mut out: HashMap<&str, Vec<usize>> = HashMap::default();
        let judged_fns: HashSet<&str> = judged.iter().map(|(f, _)| *f).collect();
        for (f, (params, body)) in &self.fns {
            // an async body's TypeError rejects the returned promise
            // (§27.7.5.1), so its caller must not throw it first
            if !judged_fns.contains(f)
                || params.iter().any(runs_code)
                || self.ast.async_fns.contains(*f)
                || self.ast.async_generator_fns.contains(*f)
            {
                continue;
            }
            let Some(i) = first_deref(self.ast, params, body) else {
                continue;
            };
            let p = &params[i];
            // the lowering tests the argument of a fixed-argc call only,
            // and only one whose slot spells a nullish value in band
            let sites = self.calls.args.get(f).map_or(&[][..], Vec::as_slice);
            let fixed = !sites.is_empty()
                && sites.iter().all(|args| {
                    args.len() > i
                        && (args[..=i].iter())
                            .all(|a| !matches!(self.ast.get_expr(*a), Expr::Spread { .. }))
                        && self.ty(args[i]).is_some_and(holds_nullish_in_band)
                });
            if fixed
                && judged.contains(&(*f, i))
                && !p.is_rest
                && !self.assigned.contains(p.name.as_str())
            {
                out.entry(*f).or_default().push(i);
            }
        }
        out
    }
}

/// A parameter whose binding evaluates code before the body: an
/// initialiser, or a pattern (bound by `let`s off a synthesized
/// parameter).
fn runs_code(p: &Param) -> bool {
    p.default.is_some() || p.name.starts_with("__param_destr_")
}

fn first_deref(ast: &Ast, params: &[Param], body: &[Stmt]) -> Option<usize> {
    let mut known: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
    match block(ast, body, &mut known) {
        First::Deref(n) => params.iter().position(|p| p.name == n),
        _ => None,
    }
}

/// `known` holds the bindings declared so far, whose plain read cannot
/// raise or run code.
fn block<'a>(ast: &Ast, body: &'a [Stmt], known: &mut HashSet<&'a str>) -> First {
    for s in body {
        match stmt(ast, s, known) {
            First::Pure => {}
            other => return other,
        }
    }
    First::Pure
}

fn stmt<'a>(ast: &Ast, s: &'a Stmt, known: &mut HashSet<&'a str>) -> First {
    match s {
        Stmt::FnDecl { .. } => First::Pure,
        Stmt::LetDecl { name, init, .. } => {
            let f = expr(ast, *init, known);
            known.insert(name);
            f
        }
        Stmt::Expr(e) => expr(ast, *e, known),
        Stmt::Block(v) | Stmt::Multi(v) => block(ast, v, known),
        // what follows a test runs only on one side of it
        Stmt::If { cond, .. } | Stmt::While { cond, .. } => only_deref(expr(ast, *cond, known)),
        Stmt::Switch { scrutinee, .. } => only_deref(expr(ast, *scrutinee, known)),
        Stmt::Return(Some(e)) => only_deref(expr(ast, *e, known)),
        Stmt::For { init, cond, .. } => {
            match init.as_deref().map_or(First::Pure, |i| stmt(ast, i, known)) {
                First::Pure => only_deref(cond.map_or(First::Effect, |c| expr(ast, c, known))),
                other => only_deref(other),
            }
        }
        _ => First::Effect,
    }
}

fn only_deref(f: First) -> First {
    match f {
        First::Deref(n) => First::Deref(n),
        _ => First::Effect,
    }
}

/// What evaluating `e` does first. An operator over plain operands is
/// an effect: converting an object operand may run its `valueOf`.
fn expr(ast: &Ast, e: ExprId, known: &HashSet<&str>) -> First {
    match ast.get_expr(e) {
        Expr::Ident(n) if known.contains(n.as_str()) => First::Pure,
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null => First::Pure,
        Expr::Array(v) if v.is_empty() => First::Pure,
        Expr::ObjectLit { fields } if fields.is_empty() => First::Pure,
        Expr::Member { obj, .. } => receiver(ast, *obj, known),
        // the key is evaluated before the base is tested
        Expr::Index { obj, index } => {
            match (receiver(ast, *obj, known), expr(ast, *index, known)) {
                (First::Deref(n), First::Pure) => First::Deref(n),
                _ => First::Effect,
            }
        }
        Expr::Call { callee, .. } if matches!(ast.get_expr(*callee), Expr::Member { .. }) => {
            only_deref(expr(ast, *callee, known))
        }
        Expr::BinOp { op, left, right } => match expr(ast, *left, known) {
            First::Pure if !matches!(op, BinOp::LAnd | BinOp::LOr) => {
                only_deref(expr(ast, *right, known))
            }
            other => only_deref(other),
        },
        Expr::Unary { expr: inner, .. } | Expr::Ternary { cond: inner, .. } => {
            only_deref(expr(ast, *inner, known))
        }
        Expr::TypeOf { expr: inner } | Expr::As { expr: inner, .. } => expr(ast, *inner, known),
        _ => First::Effect,
    }
}

/// The base of a member read: a binding is tested right there; any
/// other base is evaluated first, and reading a member of it may run a
/// getter.
fn receiver(ast: &Ast, obj: ExprId, known: &HashSet<&str>) -> First {
    match ast.get_expr(obj) {
        Expr::Ident(n) if known.contains(n.as_str()) => First::Deref(n.clone()),
        _ => only_deref(expr(ast, obj, known)),
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Stmt;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// The parameter `f`'s body reads a member of first, by index.
    fn first(src: &str) -> Option<usize> {
        let tokens = tokenize(src).expect("lex");
        let ast = parse(src, &tokens).expect("parse");
        ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl {
                name, params, body, ..
            } if name == "f" => Some(super::first_deref(&ast, params, body)),
            _ => None,
        })?
    }

    #[test]
    fn a_read_reached_through_plain_operands_comes_first() {
        let n = "type N = { n: number }\n";
        let at = |body: &str| first(&format!("{n}function f(x: N, k: number) {{ {body} }}"));
        assert_eq!(at("return x.n"), Some(0));
        assert_eq!(at("return k + x.n"), Some(0));
        assert_eq!(at("return -x.n"), Some(0));
        assert_eq!(
            at("const b = 10; let a = 0; if (x.n > k) a = b; return a"),
            Some(0)
        );
        assert_eq!(
            at("let t = 0; for (let i = 0; i < x.n; i++) t += i; return t"),
            Some(0)
        );
        assert_eq!(at("while (k < x.n) k++; return k"), Some(0));
        assert_eq!(at("return x.n > 0 && k > 0"), Some(0));
    }

    #[test]
    fn anything_a_caller_could_see_first_keeps_the_test_in_the_callee() {
        let n = "type N = { n: number }\nfunction g(): number { return 1 }\n";
        let at = |body: &str| first(&format!("{n}function f(x: N, k: number) {{ {body} }}"));
        assert_eq!(at("console.log(k); return x.n"), None);
        assert_eq!(at("const a = g(); return x.n + a"), None);
        // reading an undeclared name raises first
        assert_eq!(at("return u + x.n"), None);
        // `+` on two plain operands may run a `valueOf`
        assert_eq!(at("const s = k + k; return x.n + s"), None);
        assert_eq!(at("return k > 0 && x.n > 0"), None);
        assert_eq!(at("if (k > 0) return x.n; return 0"), None);
        assert_eq!(at("return x"), None);
    }
}
