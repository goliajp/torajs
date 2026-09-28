//! Pass 0's `for…in` arm — a yield-bearing `for (k in o)` in a machine
//! body.
//!
//! The parser has already split the loop the way pass 0 needs it: the
//! object is evaluated once into `__forin_obj_N` and the §14.7.5.9 key
//! snapshot is hoisted as `let __forof_src_N = Object.__forinKeys(..)`,
//! both declared before the loop, so the let-lift turns them into
//! fields that survive a suspend. What the SSA lowering keeps in a
//! stack slot — the snapshot cursor — is what cannot, so the loop
//! becomes a counted walk over the snapshot with the cursor as one
//! more lifted local:
//!
//! ```text
//! let __gfki_N: number = 0;
//! while (__gfki_N < __forof_src_N.length) {
//!   let k: string = __forof_src_N[__gfki_N];
//!   __gfki_N = __gfki_N + 1;
//!   if (!Object.__forinKeyLive(__forin_obj_N, k)) continue;
//!   body
//! }
//! ```
//!
//! The cursor advances before the body, so `continue` needs no step of
//! its own. `Object.__forinKeyLive` is the lowering's mid-loop-delete
//! guard (§14.7.5.9: a property deleted before it is visited is not
//! visited) spelled as a call, since a machine body cannot reach the
//! loop's blocks; a head that writes a `var`, a member or a pattern is
//! already inside `body` (the parser's write-back).

use super::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};

/// The counted walk for one for-in whose body suspends. `src` is the
/// hoisted key snapshot, `obj` the loop's read of the once-evaluated
/// object.
pub(super) fn build_forin_walk(
    ast: &mut Ast,
    var_name: String,
    src: String,
    obj: ExprId,
    body: Stmt,
    n: usize,
) -> Stmt {
    let cursor = format!("__gfki_{n}");
    let ident = |ast: &mut Ast, s: &str| ast.add_expr(Expr::Ident(s.to_string()));
    let zero = ast.add_expr(Expr::Number(0.0));
    let decl = Stmt::LetDecl {
        mutable: true,
        name: cursor.clone(),
        type_ann: Some("number".into()),
        init: zero,
        is_var: false,
    };
    let c = ident(ast, &cursor);
    let s = ident(ast, &src);
    let len = ast.add_expr(Expr::Member {
        obj: s,
        name: "length".into(),
    });
    let cond = ast.add_expr(Expr::BinOp {
        op: BinOp::Lt,
        left: c,
        right: len,
    });
    let s = ident(ast, &src);
    let c = ident(ast, &cursor);
    let key = ast.add_expr(Expr::Index { obj: s, index: c });
    let bind = Stmt::LetDecl {
        mutable: true,
        name: var_name.clone(),
        type_ann: Some("string".into()),
        init: key,
        is_var: false,
    };
    let c = ident(ast, &cursor);
    let one = ast.add_expr(Expr::Number(1.0));
    let next = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: c,
        right: one,
    });
    let target = ident(ast, &cursor);
    let step = Stmt::Expr(ast.add_expr(Expr::Assign {
        target,
        value: next,
    }));
    let ns = ident(ast, "Object");
    let callee = ast.add_expr(Expr::Member {
        obj: ns,
        name: "__forinKeyLive".into(),
    });
    let args = vec![obj, ident(ast, &var_name)];
    let live = ast.add_expr(Expr::Call { callee, args });
    let gone = ast.add_expr(Expr::Unary {
        op: UnaryOp::Not,
        expr: live,
    });
    let skip = Stmt::If {
        cond: gone,
        then_branch: Box::new(Stmt::Continue(None)),
        else_branch: None,
    };
    Stmt::Multi(vec![
        decl,
        Stmt::While {
            cond,
            body: Box::new(Stmt::Block(vec![bind, step, skip, body])),
        },
    ])
}
