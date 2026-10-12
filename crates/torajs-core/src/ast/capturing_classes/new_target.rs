//! The constructor half of §10.2.1 on the capturing lane: a class
//! constructor is entered by [[Construct]] only, with the new.target
//! the `new` (or the subclass's `super(…)`) hands it.
//!
//! The lane's constructor is a function expression, so a call through
//! the value (`K()`, `[2, 1].sort(K)`, `"a".replace(/a/, K)`) ran its
//! body where §10.2.1 step 2 throws. [`call_guard`] puts the throw at
//! the top of the body, keyed on `new.target`: a function expression
//! that reads it gets a construct face that `new`, `Reflect.construct`
//! and the `super(…)` kernel enter with the real value, while every
//! call through the value runs the call channel, where it is undefined
//! (`new_target_param.rs`, `check_monomorph_ctor_face.rs`). So an
//! undefined `new.target` is exactly the [[Call]].
//!
//! The guard makes `P.call(this, …)` a [[Call]] that throws, so a
//! sibling's `super(…)` (§13.3.7.1 step 5: [[Construct]] with the
//! subclass's new.target) needs another way in. A class some sibling
//! extends gets a construct twin: its constructor with new.target as a
//! leading parameter and no guard, bound next to the class, which the
//! sibling `.call`s with typed arguments — the direct receiver-first
//! call the lane always made, with the new.target added. A landing
//! without a twin (its constructor reads `arguments`, which the extra
//! slot would shift, or the extender sits where the scan does not look)
//! goes through the runtime super kernel, which enters the face.

use super::super::clone_body::BodyCloner;
use super::super::desugar_with::walk::{arrow_nodes, stmt_children_ref};
use super::super::{Ast, Expr, ExprId, Param, Stmt, UnaryOp};
use super::extends::REST_ARGS;

/// What a `super(…)` passes on: the site's own argument list, or the
/// implicit constructor's rest binding.
pub(super) enum SuperArgs {
    List(Vec<ExprId>),
    Rest,
}

/// The twin's leading parameter, the new.target its caller passes.
const TWIN_NT: &str = "__ccnt";

/// `__cci<N>_<C>` (the class-scope binding) → its twin `__ccn<N>_<C>`.
pub(super) fn twin_name(inner: &str) -> String {
    format!("__ccn{}", &inner["__cci".len()..])
}

fn is_twin(name: &str) -> bool {
    name.strip_prefix("__ccn")
        .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
}

/// Whether the class `s`, bound to `binding` in `list`, gets a twin: it
/// constructs itself rather than forwarding, nothing in its constructor
/// reads `arguments`, and some class in `list` extends it.
pub(super) fn wants_twin(ast: &Ast, list: &[Stmt], s: &Stmt, binding: &str) -> bool {
    let Stmt::ClassDecl { ctor, parent, .. } = s else {
        return false;
    };
    let reads_args = ctor.as_ref().is_some_and(|c| {
        let dflts: Vec<Stmt> = c
            .params
            .iter()
            .filter_map(|p| p.default)
            .map(Stmt::Expr)
            .collect();
        super::super::arguments_object_walkers::body_has_any_arguments_touch(ast, &c.body)
            || super::super::arguments_object_walkers::body_has_any_arguments_touch(ast, &dflts)
    });
    (ctor.is_some() || parent.is_none()) && !reads_args && extended_in(ast, list, binding)
}

fn extended_in(ast: &Ast, list: &[Stmt], binding: &str) -> bool {
    list.iter().any(|s| stmt_extends(ast, s, binding))
}

fn stmt_extends(ast: &Ast, s: &Stmt, binding: &str) -> bool {
    match s {
        Stmt::ClassDecl { parent, .. } if ast.parent_ident_name(*parent) == Some(binding) => {
            return true;
        }
        Stmt::FnDecl { body, .. } => return extended_in(ast, body, binding),
        _ => {}
    }
    stmt_children_ref(s)
        .into_iter()
        .any(|c| stmt_extends(ast, c, binding))
        || arrow_nodes(ast, s)
            .into_iter()
            .any(|a| match ast.get_expr(a) {
                Expr::ArrowFn { body, .. } => extended_in(ast, body, binding),
                _ => false,
            })
}

/// `const __ccn<N>_<C>: any = function (__ccnt, …params) { body }` —
/// the constructor's lowered params and body, cloned, with every
/// new.target read of its own taken from the parameter.
pub(super) fn twin_stmt(ast: &mut Ast, params: &[Param], body: &[Stmt], twin: String) -> Stmt {
    let mut cloner = BodyCloner::new(ast);
    let body = cloner.clone_stmts(body);
    let mut head = vec![Param {
        name: TWIN_NT.into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    }];
    head.extend(params.iter().map(|p| Param {
        default: p.default.map(|d| cloner.clone_expr(d)),
        ..p.clone()
    }));
    super::super::clone_body_tables::migrate(&mut cloner);
    let mut reads = Vec::new();
    super::super::new_target_param::collect_stmts(ast, &body, &mut reads);
    for d in head.iter().filter_map(|p| p.default) {
        super::super::new_target_param::collect(ast, d, &mut reads);
    }
    for r in reads {
        ast.exprs[r.0 as usize] = Expr::Ident(TWIN_NT.into());
    }
    let init = ast.add_expr(Expr::ArrowFn {
        params: head,
        return_type: None,
        body,
    });
    ast.fn_expr_exprs.insert(init);
    Stmt::LetDecl {
        mutable: false,
        name: twin,
        type_ann: Some("any".into()),
        init,
        is_var: false,
    }
}

/// `super(…)` into the lane sibling whose construction lands on
/// `target`: `target.call(this, new.target, args…)` when it is a twin,
/// `__torajs_super_value(target, this, [args…], new.target)` otherwise.
pub(super) fn sibling_super(ast: &mut Ast, target: &str, args: SuperArgs) -> Expr {
    let p = ast.add_expr(Expr::Ident(target.to_string()));
    let this = ast.add_expr(Expr::This);
    let nt = ast.add_expr(Expr::NewTarget);
    let rest = || Expr::Ident(REST_ARGS.to_string());
    if is_twin(target) {
        let callee = ast.add_expr(Expr::Member {
            obj: p,
            name: "call".to_string(),
        });
        let mut full = vec![this, nt];
        match args {
            SuperArgs::List(a) => full.extend(a),
            SuperArgs::Rest => {
                let expr = ast.add_expr(rest());
                full.push(ast.add_expr(Expr::Spread { expr }));
            }
        }
        return Expr::Call { callee, args: full };
    }
    let callee = ast.add_expr(Expr::Ident("__torajs_super_value".to_string()));
    let pack = match args {
        SuperArgs::List(a) => ast.add_expr(Expr::Array(a)),
        SuperArgs::Rest => ast.add_expr(rest()),
    };
    Expr::Call {
        callee,
        args: vec![p, this, pack, nt],
    }
}

/// The synthetic raise the guard calls: a TypeError carrying bun's
/// (JSC's) message, raised the way a runtime kernel raises one, so a
/// program that never names `TypeError` needs no injected class for
/// it (`ssa_lower_call_super_value`).
pub(crate) const CALL_THROW: &str = "__torajs_class_call_throw";

/// `if (!new.target) __torajs_class_call_throw("<name>")` — `shown`
/// is the class's own name, which the message carries. new.target is
/// undefined or a constructor, so its falsiness is the [[Call]] test
/// (the guard names no `undefined` a program's own binding could take).
pub(super) fn call_guard(ast: &mut Ast, shown: &str) -> Stmt {
    let nt = ast.add_expr(Expr::NewTarget);
    let cond = ast.add_expr(Expr::Unary {
        op: UnaryOp::Not,
        expr: nt,
    });
    let callee = ast.add_expr(Expr::Ident(CALL_THROW.to_string()));
    let name = ast.add_expr(Expr::String(shown.to_string().into()));
    let raise = ast.add_expr(Expr::Call {
        callee,
        args: vec![name],
    });
    Stmt::If {
        cond,
        then_branch: Box::new(Stmt::Expr(raise)),
        else_branch: None,
    }
}

/// Is `s` a [`call_guard`]? A default the body takes over goes after it.
pub(crate) fn is_call_guard(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::If {
        then_branch,
        else_branch: None,
        ..
    } = s
    else {
        return false;
    };
    matches!(then_branch.as_ref(), Stmt::Expr(e)
        if matches!(ast.get_expr(*e), Expr::Call { callee, .. }
            if matches!(ast.get_expr(*callee), Expr::Ident(n) if n == CALL_THROW)))
}
