//! A plain function that reads `new.target` takes it as a parameter
//! (RFC 20260917-plain-fn-new-target knife 1).
//!
//! §9.4.5 GetNewTarget reads the [[NewTarget]] of the function's own
//! environment: the constructor for a [[Construct]], undefined for a
//! [[Call]]. An arrow function has none of its own and sees the
//! enclosing function's. A class constructor already carries it as its
//! second parameter (`__cm_<C>__ctor(__this, __new_target, …)`); a plain
//! function had no binding at all, so `Expr::NewTarget` lowered to
//! undefined even under `new F()`.
//!
//! This pass gives a top-level function declaration the class
//! constructor's head, `(__this, __new_target, …user)`, when its body —
//! arrows included, nested `function`s not — reads `new.target`, and
//! turns each such read into the parameter. The same head keeps every
//! positional rule the constructor head already has (arguments object,
//! this-first argv) applying unchanged. Direct calls pass `undefined`;
//! the constructor factory `__fnctor_<F>` passes `F`
//! (`fn_constructor.rs`); a value face forwards `undefined`
//! (`forwarder_synth.rs`).
//!
//! Only functions that read `new.target` change, so a program without
//! one is untouched.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Param, Stmt};

/// The hidden parameter's name, shared with the class constructor head.
pub(crate) const NEW_TARGET: &str = "__new_target";

fn hidden(name: &str) -> Param {
    Param {
        name: name.into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    }
}

pub fn bind_new_target_param(ast: &mut Ast) {
    bind_fn_expr_new_target(ast);
    // (name, `new.target` reads, whether the receiver head is new)
    let mut found: Vec<(String, Vec<ExprId>, bool)> = Vec::new();
    for st in &ast.stmts {
        let Stmt::FnDecl {
            name,
            params,
            body,
            is_generator: false,
            ..
        } = st
        else {
            continue;
        };
        if params.get(1).is_some_and(|p| p.name == NEW_TARGET) {
            continue;
        }
        // Only a function the source declared. Class members are
        // FnDecls by now too (`__cm_<C>__m`, `__sm_<C>__m`, …), but a
        // method, getter or setter is never a constructor, so its
        // new.target is undefined — which is what an unbound read
        // already lowers to. A declared function that reads `this` got
        // its receiver from `bind_this_param`, which records it.
        let declared = match params.first() {
            Some(p) if p.name == "__this" => ast.this_param_fns.contains(name),
            _ => !name.starts_with("__"),
        };
        if !declared {
            continue;
        }
        // A parameter default is evaluated at the call site
        // (`Param::default`), outside the body's bindings, so a read
        // there is not rewritten.
        let mut reads = Vec::new();
        collect_stmts(ast, body, &mut reads);
        if !reads.is_empty() {
            let adds_this = params.first().is_none_or(|p| p.name != "__this");
            found.push((name.clone(), reads, adds_this));
        }
    }
    if found.is_empty() {
        return;
    }
    for (name, reads, adds_this) in &found {
        for r in reads {
            ast.exprs[r.0 as usize] = Expr::Ident(NEW_TARGET.into());
        }
        let Some(Stmt::FnDecl { params, .. }) = ast
            .stmts
            .iter_mut()
            .find(|s| matches!(s, Stmt::FnDecl { name: n, .. } if n == name))
        else {
            continue;
        };
        if *adds_this {
            params.insert(0, hidden("__this"));
            ast.this_param_fns.insert(name.clone());
        }
        params.insert(1, hidden(NEW_TARGET));
        ast.new_target_fns.insert(name.clone());
    }
    // Direct calls — a [[Call]]: no receiver the source gave a slot to
    // yet when the head is new, and no new.target.
    let sites: Vec<(usize, bool)> = (0..ast.exprs.len())
        .filter_map(|i| match &ast.exprs[i] {
            Expr::Call { callee, .. } => match &ast.exprs[callee.0 as usize] {
                Expr::Ident(n) => found.iter().find(|f| &f.0 == n).map(|f| (i, f.2)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for (i, adds_this) in sites {
        let undef_this = adds_this.then(|| ast.add_expr(Expr::Ident("undefined".into())));
        let undef_nt = ast.add_expr(Expr::Ident("undefined".into()));
        if let Expr::Call { args, .. } = &mut ast.exprs[i] {
            if let Some(u) = undef_this {
                args.insert(0, u);
            }
            args.insert(1, undef_nt);
        }
    }
}

/// Knife 2 — a function EXPRESSION reading `new.target`. Its closure
/// value is the function itself, so no head can change: every call
/// through the value would shift. The reads become a body-level
/// `let __new_target = undefined` (an arrow inside captures it like any
/// local), which is the [[Call]] answer; the checker's monomorph step
/// later mints a construct face from the lifted body with the slot as a
/// parameter (`check_monomorph_ctor_face.rs`), and the construct kernel
/// enters that face with the new.target. Recorded by ExprId, which
/// `lift_arrow_fns` keeps (it rewrites the arena slot in place).
fn bind_fn_expr_new_target(ast: &mut Ast) {
    let mut exprs: Vec<ExprId> = ast.fn_expr_exprs.iter().copied().collect();
    exprs.sort_by_key(|e| e.0);
    for eid in exprs {
        let Expr::ArrowFn { body, .. } = ast.get_expr(eid) else {
            continue;
        };
        let mut reads = Vec::new();
        collect_stmts(ast, body, &mut reads);
        if reads.is_empty() {
            continue;
        }
        for r in reads {
            ast.exprs[r.0 as usize] = Expr::Ident(NEW_TARGET.into());
        }
        let undef = ast.add_expr(Expr::Ident("undefined".into()));
        if let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] {
            body.insert(
                0,
                Stmt::LetDecl {
                    mutable: false,
                    name: NEW_TARGET.into(),
                    type_ann: Some("any".into()),
                    init: undef,
                    is_var: false,
                },
            );
        }
        ast.new_target_fn_exprs.insert(eid);
    }
}

/// The register call knife 3 prepends, named for the checker and the
/// lowering arm.
pub(crate) const REGISTER: &str = "__torajs_new_target_register";

/// Knife 3 — [[Construct]] through a value. A function above that has a
/// value face (`__fwdrecv_<F>`, which forwards `undefined` for the
/// slot) also gets a construct face `__ntctor_<F>(__env, __this,
/// __new_target, …user)` passing both through, registered at program
/// start against the value face (`torajs-anyvalue`
/// `construct_new_target.rs`). Runs after `promote_this_fn_values`,
/// which mints the value faces.
///
/// A function with a parameter default or a rest parameter keeps the
/// call channel: the face forwards each parameter by name, which a
/// call-site default or a spread would misrepresent.
pub fn synthesize_new_target_ctor_faces(ast: &mut Ast) {
    let mut faces: Vec<Stmt> = Vec::new();
    let mut registers: Vec<Stmt> = Vec::new();
    // name order, not hash order: the faces land in this order
    let mut names: Vec<String> = ast.new_target_fns.iter().cloned().collect();
    names.sort();
    for name in names {
        let value_face = format!("__fwdrecv_{name}");
        if !ast
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name: n, .. } if *n == value_face))
        {
            continue;
        }
        let Some(Stmt::FnDecl {
            params,
            return_type,
            span,
            ..
        }) = ast
            .stmts
            .iter()
            .find(|s| matches!(s, Stmt::FnDecl { name: n, .. } if *n == name))
        else {
            continue;
        };
        if params.iter().any(|p| p.default.is_some() || p.is_rest)
            || params.first().is_some_and(|p| p.name == "__env")
        {
            continue;
        }
        let (params, return_type, span) = (params.clone(), return_type.clone(), *span);
        let ctor_face = format!("__ntctor_{name}");
        let callee = ast.add_expr(Expr::Ident(name.clone()));
        let args = params
            .iter()
            .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
            .collect();
        let call = ast.add_expr(Expr::Call { callee, args });
        let mut face_params = vec![Param {
            name: "__env".into(),
            type_ann: Some("__env()".into()),
            default: None,
            is_rest: false,
        }];
        // An untyped JS parameter has no annotation to copy; the face
        // takes it as `any`, as the value faces do.
        face_params.extend(params.into_iter().map(|p| Param {
            type_ann: p.type_ann.or_else(|| Some("any".into())),
            ..p
        }));
        faces.push(Stmt::FnDecl {
            name: ctor_face.clone(),
            type_params: Vec::new(),
            params: face_params,
            return_type,
            body: vec![Stmt::Return(Some(call))],
            is_generator: false,
            span,
        });
        let reg = ast.add_expr(Expr::Ident(REGISTER.into()));
        let vf = ast.add_expr(Expr::Closure {
            fn_name: value_face,
            captures: Vec::new(),
        });
        let cf = ast.add_expr(Expr::Closure {
            fn_name: ctor_face,
            captures: Vec::new(),
        });
        let call = ast.add_expr(Expr::Call {
            callee: reg,
            args: vec![vf, cf],
        });
        registers.push(Stmt::Expr(call));
    }
    ast.stmts.extend(faces);
    ast.stmts.splice(0..0, registers);
}

fn collect_stmts(ast: &Ast, stmts: &[Stmt], out: &mut Vec<ExprId>) {
    for s in stmts {
        // A nested declaration is its own function with its own
        // [[NewTarget]].
        if matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. }) {
            continue;
        }
        for e in stmt_exprs(s) {
            collect(ast, e, out);
        }
        for c in stmt_children_ref(s) {
            collect_stmts(ast, std::slice::from_ref(c), out);
        }
    }
}

fn collect(ast: &Ast, eid: ExprId, out: &mut Vec<ExprId>) {
    match ast.get_expr(eid) {
        Expr::NewTarget => out.push(eid),
        // A function expression binds its own new.target; an arrow
        // reads the enclosing one.
        Expr::ArrowFn { .. } if ast.fn_expr_exprs.contains(&eid) => {}
        Expr::ArrowFn { params, body, .. } => {
            for p in params {
                if let Some(d) = p.default {
                    collect(ast, d, out);
                }
            }
            collect_stmts(ast, body, out);
        }
        _ => {
            for c in expr_children(ast, eid) {
                collect(ast, c, out);
            }
        }
    }
}
