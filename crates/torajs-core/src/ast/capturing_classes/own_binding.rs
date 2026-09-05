//! The container's half of the pair a class declaration binds.
//!
//! §14.2.3 gives a class DECLARATION two bindings, not one: an
//! immutable one in the class's own scope, which its body reads, and
//! a mutable one in the scope the class was written in, which
//! everything else reads. `class D {}; D = 1` is therefore legal, and
//! a write through the outer binding is invisible to the body.
//!
//! This lane collapsed the pair: the α-rename wrote one minted
//! spelling over every occurrence of the name, inside the class and
//! outside it, so one `let` had to serve both readings. Now it mints
//! two — `__cci<N>_<C>` for the class scope, holding the constructor
//! and carrying every member install, and `__cc<N>_<C>` for the
//! container, declared last as an alias to it.
//!
//! Both, always. rotation 586-05 argued the other way for the hoist
//! lane — a class whose name nobody writes cannot tell the two apart,
//! so one binding is the whole of what the spec describes — and
//! rotation 587-04 carried that argument here. `extends` is the
//! witness against it: a sibling's `super(…)` and prototype link
//! reach the parent through whatever cell the container holds, so
//! `class P {…}; class Q extends P {}; P = null; new Q()` died on a
//! null callee where bun constructs. The class object needs a cell no
//! later write can reach, whether or not its own body ever says its
//! name.

use super::super::{Ast, Expr, Stmt};

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
