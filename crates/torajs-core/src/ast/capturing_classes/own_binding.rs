//! Does the class read its own name from inside itself?
//!
//! §14.2.3 gives a class DECLARATION two bindings, not one: an
//! immutable one in the class's own scope, which its body reads, and
//! a mutable one in the scope the class was written in, which
//! everything else reads. `class D {}; D = 1` is therefore legal, and
//! a write through the outer binding is invisible to the body.
//!
//! This lane collapses the pair: the α-rename writes `__cc<N>_<C>`
//! over every occurrence of the name, inside the class and outside
//! it, so one `let` has to serve both readings — and it was minted
//! immutable, which made the outer half refuse a legal assignment.
//!
//! The two can only be told apart when the body actually reads the
//! name. When it does not, one MUTABLE binding is exactly what the
//! spec describes and nothing can observe the difference; when it
//! does, the immutable binding stays and the assignment keeps saying
//! so out loud rather than letting a write leak into the class's own
//! reads. That is rotation 586's argument for the hoist lane's outer
//! slot, from the other side: there, a class whose name nobody writes
//! needs no second binding; here, a class that reads nothing of
//! itself needs no second one either.

use super::super::free_vars::free_vars_of_body;
use super::super::{Ast, ClassCtor, ClassMethod, Expr, Param, StaticInit, Stmt};

/// Every part of the class body a self-reference can be written in.
/// The name is the MINTED one — this runs after the α-rename, so the
/// body already spells the binding the `let` below will declare.
///
/// Instance-field initializers are not walked separately: the parser
/// appends them to the constructor body (a synthesized one when the
/// class declares no constructor) at class-decl finalization, so
/// `class C { field = () => C }` is already in `ctor`.
///
/// A computed key cannot reach the binding — §15.7.14 evaluates keys
/// before the class's own binding is initialized, so a key naming the
/// class is a TDZ error rather than a read — and the keys live in
/// side tables under the SOURCE name, which this no longer holds.
/// Over-answering would only keep the binding immutable, so the
/// unwalked direction is the loud one either way.
pub(super) fn is_read_inside(
    ast: &Ast,
    name: &str,
    ctor: Option<&ClassCtor>,
    methods: &[ClassMethod],
    static_methods: &[ClassMethod],
    static_init: &[StaticInit],
) -> bool {
    let reads = |params: &[Param], body: &[Stmt]| -> bool {
        let bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        free_vars_of_body(ast, &bound, body)
            .iter()
            .any(|n| n == name)
    };
    if let Some(c) = ctor
        && reads(&c.params, &c.body)
    {
        return true;
    }
    if methods
        .iter()
        .chain(static_methods.iter())
        .any(|m| reads(&m.params, &m.body))
    {
        return true;
    }
    static_init.iter().any(|si| match si {
        StaticInit::Field(f) => reads(&[], &[Stmt::Expr(f.init)]),
        StaticInit::Block(v) => reads(&[], v),
    })
}

/// [`is_read_inside`] asked of a whole class declaration, under the
/// name it still carries. The lane asks this BEFORE its α-rename, so
/// the name here is the source spelling.
pub(super) fn is_read_inside_class(ast: &Ast, s: &Stmt, name: &str) -> bool {
    let Stmt::ClassDecl {
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    else {
        return false;
    };
    is_read_inside(
        ast,
        name,
        ctor.as_ref(),
        methods,
        static_methods,
        static_init,
    )
}

/// `let <outer>: any = <inner>` — the container's half of the pair.
/// Mutable, because §14.2.3 asks for `CreateMutableBinding`: a class
/// declaration is not a constant declaration, so `C = 1` next to one
/// is legal and the body's own reads do not see it.
pub(super) fn outer_alias_stmt(ast: &mut Ast, outer: String, inner: &str) -> Stmt {
    let init = ast.add_expr(Expr::Ident(inner.to_string()));
    Stmt::LetDecl {
        mutable: true,
        name: outer,
        type_ann: Some("any".to_string()),
        init,
        is_var: false,
    }
}
