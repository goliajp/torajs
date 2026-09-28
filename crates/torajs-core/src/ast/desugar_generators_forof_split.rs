//! `for (v of e.split("sep"))` in a machine body.
//!
//! The parser mints `Stmt::ForOfSplitIter` for a literal separator so
//! the loop walks the string through a stack-held split cursor instead
//! of materializing the array. Neither premise of that cursor holds in
//! a machine body: a stack slot does not survive a `yield` (a
//! yield-bearing loop fell through to the state machine's catch-all
//! and was rejected with "yield is only valid inside a `function*`
//! generator body"), and a lifted binding the loop splits is `any`,
//! where the cursor needs a string ("for-of split parent must be
//! string, got Any" — the same loop without a yield). Here it becomes
//! the array the plain loop would have iterated, bound to a lifted
//! local, and takes the manual iterator protocol:
//!
//! ```text
//! for (const v of e.split(sep)) body   →
//! let __gfsplit_N: any = e.split(sep);
//! <manual protocol over __gfsplit_N>
//! ```
//!
use super::desugar_generators_forof::{Cx, build_manual_protocol, rewrite_in_stmt};
use super::{Ast, Expr, Stmt};

pub(super) fn rewrite(ast: &mut Ast, s: &mut Stmt, cx: &mut Cx) {
    let Stmt::ForOfSplitIter { body, .. } = s else {
        return;
    };
    rewrite_in_stmt(ast, body, cx);
    let Stmt::ForOfSplitIter {
        var_name,
        parent,
        sep,
        body,
    } = std::mem::replace(s, Stmt::Multi(Vec::new()))
    else {
        unreachable!()
    };
    let src = format!("__gfsplit_{}", cx.counter);
    let callee = ast.add_expr(Expr::Member {
        obj: parent,
        name: "split".into(),
    });
    let init = ast.add_expr(Expr::Call {
        callee,
        args: vec![sep],
    });
    let decl = Stmt::LetDecl {
        mutable: false,
        name: src.clone(),
        type_ann: Some("any".into()),
        init,
        is_var: false,
    };
    // One flat Multi: a wrapping label moves onto the loop only when the
    // Multi's LAST statement is the loop tail (`forof_label`).
    *s = match build_manual_protocol(ast, var_name, src, *body, cx, false) {
        Stmt::Multi(mut ss) => {
            ss.insert(0, decl);
            Stmt::Multi(ss)
        }
        other => Stmt::Multi(vec![decl, other]),
    };
}
