//! Generator class assembly + factory FnDecl replacement.
//!
//! Chunk 338 — extracted from `ast::desugar_generators` to keep the
//! per-generator loop body close to the 200-LOC HARD limit. Given
//! the already-built methods (`next` / `.return` / `.throw`), the
//! ctor's zero-init body, and the lifted-locals list, this sibling
//! finishes the class:
//!
//! 1. assembles `class_fields` = nominal marker + __state + __sent
//!    + lifted locals + generator params;
//! 2. extends the ctor body with `this.<param> = <param>` for every
//!    generator param;
//! 3. appends the `class __Gen_<name> { ... }` ClassDecl to the
//!    module's tail;
//! 4. replaces the original `function* <name>(...)` slot with a
//!    thin factory `function <name>(args) { return new __Gen_<name>(args); }`.
//!
//! Per the existing desugar contract, `gen_params` is moved in
//! (the factory FnDecl ends up owning it); `lifted_locals` is
//! borrowed since it survives outside the per-generator loop for
//! cross-cutting checks.

use super::{Ast, ClassCtor, ClassMethod, Expr, ExprId, Param, Stmt};

#[allow(clippy::too_many_arguments)]
pub(super) fn assemble_generator_class_and_factory(
    ast: &mut Ast,
    idx: usize,
    gen_name: String,
    gen_params: Vec<Param>,
    yield_ty: &str,
    class_name: String,
    lifted_locals: &[(String, String)],
    ctor_body_base: Vec<Stmt>,
    next_method: ClassMethod,
    return_method: ClassMethod,
    throw_method: ClassMethod,
    appended: &mut Vec<Stmt>,
) {
    // P10.6-A3 nominal marker (per-generator unique field name so
    // structurally-identical generator classes get distinct sids),
    // followed by __state + __sent. See desugar_generators.
    let mut class_fields: Vec<(String, String)> = vec![
        (format!("__gen_nominal_{gen_name}"), "number".into()),
        ("__state".into(), "number".into()),
        ("__sent".into(), yield_ty.into()),
    ];
    // Lifted locals as class fields. Their initial values are zero
    // (computed from the type ann) — actual initialization happens
    // when the corresponding let-rewrite assignment fires inside
    // next() body.
    for (lname, lty) in lifted_locals {
        class_fields.push((lname.clone(), lty.clone()));
    }
    let mut ctor_body_with_params = ctor_body_base;
    for p in &gen_params {
        let pname = p.name.clone();
        let pty = p.type_ann.clone().unwrap_or_else(|| "number".into());
        class_fields.push((pname.clone(), pty));
        // this.<param> = <param>
        let this_id = ast.add_expr(Expr::This);
        let f_member = ast.add_expr(Expr::Member {
            obj: this_id,
            name: pname.clone(),
        });
        let arg_ident = ast.add_expr(Expr::Ident(pname));
        let assign = ast.add_expr(Expr::Assign {
            target: f_member,
            value: arg_ident,
        });
        ctor_body_with_params.push(Stmt::Expr(assign));
    }
    let ctor_with_params = ClassCtor {
        params: gen_params.clone(),
        body: ctor_body_with_params,
    };

    appended.push(Stmt::ClassDecl {
        name: class_name.clone(),
        type_params: Vec::new(),
        parent: None,
        is_abstract: false,
        fields: class_fields,
        static_init: Vec::new(),
        ctor: Some(ctor_with_params),
        methods: vec![next_method, return_method, throw_method],
        static_methods: Vec::new(),
    });

    // Replace the original generator FnDecl with a thin factory
    // that returns `new __Gen_<name>(args)`.
    let factory_args: Vec<ExprId> = gen_params
        .iter()
        .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
        .collect();
    let new_expr = ast.add_expr(Expr::New {
        class_name: class_name.clone(),
        args: factory_args,
    });
    let factory_body = vec![Stmt::Return(Some(new_expr))];
    ast.stmts[idx] = Stmt::FnDecl {
        name: gen_name,
        type_params: Vec::new(),
        params: gen_params,
        return_type: Some(class_name),
        body: factory_body,
        is_generator: false,
    };
}
