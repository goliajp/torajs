//! The witness thunk declarations: one top-level function per face per
//! row, `(__self, args) -> R` with the interface's own signature, so
//! every row of a slot is entered the same way whichever body it
//! forwards to.

use super::plan::{Body, IfacePlan, Member, MemberKind};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

pub(super) fn emit_iface(ast: &mut Ast, p: &IfacePlan, out: &mut Vec<Stmt>) {
    for (face, mi, body) in &p.lit_faces {
        let name = super::lit_thunk(&p.iface, face);
        out.push(thunk(ast, name, &p.iface, &p.members[*mi], body));
    }
    for (class, row) in &p.classes {
        for (face, mi, body) in row {
            let name = super::class_thunk(class, &p.iface, face);
            out.push(thunk(ast, name, class, &p.members[*mi], body));
        }
    }
}

fn param(name: &str, ann: &str) -> Param {
    Param {
        name: name.to_string(),
        type_ann: Some(ann.to_string()),
        default: None,
        is_rest: false,
    }
}

fn thunk(ast: &mut Ast, name: String, self_ann: &str, m: &Member, body: &Body) -> Stmt {
    let mut params = vec![param("__self", self_ann)];
    let ret = match (&m.kind, body) {
        (_, Body::HasTrue | Body::HasField | Body::HasDyn) => "boolean".to_string(),
        (_, Body::FieldGet | Body::DynGet) => m.ann.clone(),
        (MemberKind::Method { params: ps, ret }, _) => {
            for (i, p) in ps.iter().enumerate() {
                params.push(param(&format!("__a{i}"), p));
            }
            ret.clone()
        }
        (MemberKind::Data, Body::FieldSet | Body::DynSet) => {
            params.push(param("__v", &m.ann));
            "void".to_string()
        }
        (MemberKind::Data, _) => unreachable!("a data member's faces are get and set"),
    };
    let stmts = thunk_body(ast, m, body, &params, ret == "void");
    Stmt::FnDecl {
        name,
        type_params: Vec::new(),
        params,
        return_type: Some(ret),
        body: stmts,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    }
}

fn ident(ast: &mut Ast, n: &str) -> ExprId {
    ast.add_expr(Expr::Ident(n.to_string()))
}

/// `__self`, or `(__self as any)` for a member the instance only has
/// dynamically.
fn recv(ast: &mut Ast, dynamic: bool) -> ExprId {
    let s = ident(ast, "__self");
    if dynamic {
        ast.add_expr(Expr::As {
            expr: s,
            ty_ann: "any".to_string(),
        })
    } else {
        s
    }
}

fn member(ast: &mut Ast, dynamic: bool, key: &str) -> ExprId {
    let obj = recv(ast, dynamic);
    ast.add_expr(Expr::Member {
        obj,
        name: key.to_string(),
    })
}

fn args(ast: &mut Ast, params: &[Param], n: usize) -> Vec<ExprId> {
    params[1..=n].iter().map(|p| ident(ast, &p.name)).collect()
}

fn member_call(ast: &mut Ast, dynamic: bool, key: &str, params: &[Param], n: usize) -> ExprId {
    let callee = member(ast, dynamic, key);
    let args = args(ast, params, n);
    ast.add_expr(Expr::Call { callee, args })
}

fn answer(e: ExprId, void: bool) -> Stmt {
    if void {
        Stmt::Expr(e)
    } else {
        Stmt::Return(Some(e))
    }
}

fn thunk_body(ast: &mut Ast, m: &Member, body: &Body, params: &[Param], void: bool) -> Vec<Stmt> {
    let all = params.len() - 1;
    let e = match body {
        Body::Static(cm, n) => {
            let callee = ident(ast, cm);
            let mut a = vec![ident(ast, "__self")];
            a.extend(args(ast, params, *n));
            ast.add_expr(Expr::Call { callee, args: a })
        }
        Body::MemberCall(n) => member_call(ast, false, &m.key, params, *n),
        Body::DynCall => member_call(ast, true, &m.key, params, all),
        Body::GuardedCall => {
            // narrowed by the test, the call is the literal's own method;
            // absent, the dynamic call raises the TypeError a call of
            // undefined does
            let cond = member(ast, false, &m.key);
            let hit = member_call(ast, false, &m.key, params, all);
            let miss = member_call(ast, true, &m.key, params, all);
            let mut then = vec![answer(hit, void)];
            if void {
                then.push(Stmt::Return(None));
            }
            return vec![
                Stmt::If {
                    cond,
                    then_branch: Box::new(Stmt::Block(then)),
                    else_branch: None,
                },
                answer(miss, void),
            ];
        }
        Body::FieldGet | Body::DynGet => member(ast, matches!(body, Body::DynGet), &m.key),
        Body::HasTrue => ast.add_expr(Expr::Bool(true)),
        Body::HasField | Body::HasDyn => {
            let left = member(ast, matches!(body, Body::HasDyn), &m.key);
            let right = ast.add_expr(Expr::Null);
            ast.add_expr(Expr::BinOp {
                op: crate::ast::BinOp::LooseNeq,
                left,
                right,
            })
        }
        Body::FieldSet | Body::DynSet => {
            let target = member(ast, matches!(body, Body::DynSet), &m.key);
            let value = ident(ast, "__v");
            let e = ast.add_expr(Expr::Assign { target, value });
            return vec![Stmt::Expr(e)];
        }
    };
    vec![answer(e, void)]
}
