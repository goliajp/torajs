//! The statement form of a call whose callee is a parenthesized
//! optional chain (`(o?.a[k])(x)`) and whose keys or arguments await.
//!
//! The parentheses keep the chain's reference (§13.3.9.1
//! ChainEvaluation): when the base is present the callee is read off
//! its receiver past the link and called on it; when it is nullish the
//! keys past the link are not evaluated, the arguments still are, and
//! the call throws its TypeError after them (§13.3.6.1 EvaluateCall).
//!
//! ```text
//! let b = <base>; let t: any = undefined; let f: any = undefined;
//! if (b != null) { t = <receiver past the link>; f = t.<step>; }
//! __torajs_call_with_this(f, t, [args…])
//! ```

use super::guarded::{CALL_WITH_THIS, Step, link_base, past_link, present, read};
use super::{LiftLog, replace};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Is `eid` a call on a parenthesized optional chain?
pub(super) fn is_paren_callee_call(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Call { callee, .. } if ast.paren_optchains.contains(callee))
}

/// Push the callee's statement form onto `prefix` (its base already
/// lifted), leave the call on the bound callee and receiver at `eid`,
/// and hand back the arguments, still to be lifted in order.
pub(super) fn to_statements(
    ast: &mut Ast,
    eid: ExprId,
    prefix: &mut Vec<Stmt>,
    log: &mut LiftLog,
) -> Vec<ExprId> {
    let Expr::Call { callee, args } = ast.get_expr(eid).clone() else {
        panic!("paren callee lift on a node that is not a call");
    };
    let base = link_base(ast, callee);
    let b = format!("__awb_{}", base.0);
    let t = format!("__awt_{}", eid.0);
    let f = format!("__awf_{}", eid.0);
    prefix.push(Stmt::LetDecl {
        mutable: false,
        name: b.clone(),
        type_ann: None,
        init: base,
        is_var: false,
    });
    for name in [&t, &f] {
        let init = ast.add_expr(Expr::Ident("undefined".into()));
        prefix.push(Stmt::LetDecl {
            mutable: true,
            name: name.clone(),
            type_ann: Some("any".into()),
            init,
            is_var: false,
        });
    }
    let (receiver, step) = match ast.get_expr(callee).clone() {
        Expr::Member { obj, name } => (past_link(ast, obj, &b), Step::Member(name)),
        Expr::Index { obj, index } => (past_link(ast, obj, &b), Step::Index(index)),
        Expr::OptChain { name, .. } => (read(ast, &b, None), Step::Member(name)),
        Expr::OptIndex { index, .. } => (read(ast, &b, None), Step::Index(index)),
        other => panic!("paren callee lift on a callee that is not a chain: {other:?}"),
    };
    let set_t = assign(ast, &t, receiver);
    let fv = read(ast, &t, Some(step));
    let set_f = assign(ast, &f, fv);
    let r = read(ast, &b, None);
    let cond = present(ast, r);
    prefix.push(Stmt::If {
        cond,
        then_branch: Box::new(Stmt::Block(vec![set_t, set_f])),
        else_branch: None,
    });
    let fr = read(ast, &f, None);
    let tr = read(ast, &t, None);
    let list = ast.add_expr(Expr::Array(args.clone()));
    let kernel = ast.add_expr(Expr::Ident(CALL_WITH_THIS.into()));
    let call = Expr::Call {
        callee: kernel,
        args: vec![fr, tr, list],
    };
    replace(ast, eid, call, log);
    args
}

fn assign(ast: &mut Ast, name: &str, value: ExprId) -> Stmt {
    let target = ast.add_expr(Expr::Ident(name.to_string()));
    Stmt::Expr(ast.add_expr(Expr::Assign { target, value }))
}
