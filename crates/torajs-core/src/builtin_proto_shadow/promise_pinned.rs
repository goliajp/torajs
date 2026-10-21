//! Can a write reach the own `resolve` / `reject` of the Promise
//! constructor? A typed `Promise.resolve(v)` asks at run time
//! (`promise_ctor_patched`, §13.3.6.1: the callee is read when the call
//! runs) because the any lane can install an override in the
//! constructor's own properties. The question has a whole-program
//! answer when the program cannot hold the constructor as a value.
//!
//! The `then` closure proof ([`super::then_opening`]) already rules out
//! every road to it but one: `Promise` is spelled only as the object of
//! a member read of one of its statics or as the right of an
//! `instanceof`, and nothing hands the constructor out under another
//! name — no `constructor` read, no global object (named or a sloppy
//! `this`), no `Reflect` / `Proxy` / `eval` / `Function`, no computed
//! key, no `Object.getPrototypeOf`, no class whose heritage is
//! `Promise`. The road left is a write through that spelling itself
//! (`Promise.resolve = f`, `delete Promise.reject`), which the proof
//! does not count because it puts no `then` anywhere.

use super::ShadowSet;
use crate::ast::{Ast, Expr, ExprId};

impl ShadowSet {
    /// Do `Promise.resolve` and `Promise.reject` stay %Promise%'s own
    /// for the whole run? Proven once, on the first call site that asks.
    pub(crate) fn promise_statics_pinned(&self, ast: &Ast) -> bool {
        *self
            .promise_pinned
            .get_or_init(|| self.then_closed(ast) && !writes_promise_member(ast))
    }
}

/// A write, an update or a delete whose target is a member of `Promise`.
fn writes_promise_member(ast: &Ast) -> bool {
    let strip_as = |mut e: ExprId| {
        while let Expr::As { expr, .. } = ast.get_expr(e) {
            e = *expr;
        }
        e
    };
    let on_promise = |target: ExprId| match ast.get_expr(strip_as(target)) {
        Expr::Member { obj, .. }
        | Expr::OptChain { obj, .. }
        | Expr::Index { obj, .. }
        | Expr::OptIndex { obj, .. } => {
            matches!(ast.get_expr(strip_as(*obj)), Expr::Ident(n) if n == "Promise")
        }
        _ => false,
    };
    ast.exprs.iter().any(|e| match e {
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => on_promise(*target),
        Expr::Delete { expr } => on_promise(*expr),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use crate::ast::Ast;
    use crate::builtin_proto_shadow::ShadowSet;

    fn pinned(src: &str) -> bool {
        let tokens = crate::lexer::tokenize(src).unwrap();
        let mut ast: Ast = crate::parser::parse(src, &tokens).unwrap();
        crate::ast::desugar_classes(&mut ast);
        crate::ast::desugar_objlit_super(&mut ast);
        ShadowSet::default().promise_statics_pinned(&ast)
    }

    #[test]
    fn statics_only_called_stay_pinned() {
        assert!(pinned(
            "function f(v: number): number { return v + 1; }\n\
             Promise.resolve(1).then(f); Promise.reject(2).catch(f);"
        ));
    }

    #[test]
    fn a_write_through_the_spelling_unpins() {
        assert!(!pinned(
            "Promise.resolve = function (x: any): any { return x; };"
        ));
        assert!(!pinned(
            "function p(): void { Promise.reject = function (x: any): any { return x; }; }"
        ));
        assert!(!pinned("delete (Promise as any).resolve;"));
    }

    #[test]
    fn another_road_to_the_constructor_unpins() {
        assert!(!pinned("const P: any = Promise; P.resolve = 1;"));
        assert!(!pinned("Reflect.set(Promise, \"resolve\", 1);"));
        assert!(!pinned(
            "const c: any = Promise.resolve(1); c.constructor.resolve = 1;"
        ));
        assert!(!pinned("(globalThis as any).Promise.resolve = 1;"));
    }
}
