//! Whether the super base has to be read at run time (§13.3.7).
//!
//! `super.x` resolves against `[[HomeObject]].[[Prototype]]`, and that
//! is a RUNTIME value. The class desugar spells it statically as the
//! parent — `<Parent>.prototype` for an instance member, the
//! `<Parent>` class object for a static one — and that spelling is
//! what lets `super.m()` become a direct `__cm_<owner>__<m>` call
//! instead of a prototype-chain walk. The two agree for the whole run
//! unless the program re-links a prototype after the class was
//! defined, so the static spelling is kept for every program that
//! cannot do that.
//!
//! The judgment is whole-program and syntactic: a program that never
//! spells `setPrototypeOf` or `__proto__` has no channel for changing
//! an object's [[Prototype]] after creation, so the parent IS the home
//! object's prototype for the whole run. One occurrence anywhere
//! degrades every super site in the program to the runtime base —
//! including one in dead code, since the scan reads the expression
//! arena rather than the reachable graph. Naming the setter is what
//! trips it, so an alias (`const f = Object.setPrototypeOf`) is caught
//! by the `Object.setPrototypeOf` member read that mints it.
//!
//! Recorded boundary: a program that reaches the setter reflectively
//! without ever spelling its name (`Object[k]` for a computed `k`)
//! keeps the static base. Closing that would mean making the base
//! dynamic unconditionally, which charges every `super.m()` in every
//! program a chain walk for a channel almost no program uses.

use super::{Ast, Expr, ExprId};

/// True when the program contains a channel that can change some
/// object's [[Prototype]] after that object exists.
pub(super) fn program_mutates_prototypes(ast: &Ast) -> bool {
    ast.exprs.iter().any(|e| match e {
        Expr::Member { name, .. } => is_proto_mutator(name),
        Expr::Index { index, .. } => {
            matches!(ast.get_expr(*index), Expr::String(s) if s.as_str().is_some_and(is_proto_mutator))
        }
        Expr::ObjectLit { fields } => fields.iter().any(|(n, _)| is_proto_mutator(n)),
        _ => false,
    })
}

fn is_proto_mutator(name: &str) -> bool {
    name == "setPrototypeOf" || name == "__proto__"
}

/// `Object.getPrototypeOf(<home object>)` — the §13.3.7 super base
/// spelled so it is read when the site runs. The home object is the
/// class's prototype for an instance member and the class object
/// itself for a static one.
pub(super) fn mint_home_proto(ast: &mut Ast, cname: &str, is_static: bool) -> ExprId {
    let class = ast.add_expr(Expr::Ident(cname.to_string()));
    let home = if is_static {
        class
    } else {
        ast.add_expr(Expr::Member {
            obj: class,
            name: "prototype".to_string(),
        })
    };
    let obj = ast.add_expr(Expr::Ident("Object".to_string()));
    let callee = ast.add_expr(Expr::Member {
        obj,
        name: "getPrototypeOf".to_string(),
    });
    ast.add_expr(Expr::Call {
        callee,
        args: vec![home],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn judge(src: &str) -> bool {
        let toks = tokenize(src).expect("lex");
        let ast = parse(src, &toks).expect("parse");
        program_mutates_prototypes(&ast)
    }

    #[test]
    fn plain_class_hierarchy_keeps_the_static_base() {
        // The shape that must stay on the direct `__cm_` call: an
        // ordinary override chain naming no prototype-mutating channel.
        assert!(!judge(
            "class A { m() { return 1 } }\n\
             class B extends A { m() { return super.m() + 1 } }\n\
             console.log(new B().m())\n"
        ));
    }

    #[test]
    fn reading_a_prototype_is_not_mutating_one() {
        // `getPrototypeOf` / `.prototype` are reads — the base a class
        // was defined with is still the base for the whole run.
        assert!(!judge(
            "class A { m() { return 1 } }\n\
             console.log(Object.getPrototypeOf(A.prototype) === Object.prototype)\n"
        ));
    }

    #[test]
    fn set_prototype_of_degrades() {
        assert!(judge("Object.setPrototypeOf({}, null)\n"));
    }

    #[test]
    fn reflect_set_prototype_of_degrades() {
        assert!(judge("Reflect.setPrototypeOf({}, null)\n"));
    }

    #[test]
    fn an_alias_of_the_setter_degrades() {
        // Naming it is what trips the scan, so binding it to a local
        // and calling through that is caught by the member read.
        assert!(judge("const f = Object.setPrototypeOf\nf({}, null)\n"));
    }

    #[test]
    fn proto_write_degrades() {
        assert!(judge("const o = {}\no.__proto__ = null\n"));
    }

    #[test]
    fn computed_proto_key_degrades() {
        assert!(judge("const o = {}\no[\"__proto__\"] = null\n"));
    }

    #[test]
    fn proto_in_an_object_literal_degrades() {
        // Creation-time, so it cannot invalidate a base on its own —
        // the scan is deliberately conservative here rather than
        // teaching it to tell the two `__proto__` positions apart.
        assert!(judge("const o = { __proto__: null }\n"));
    }
}
