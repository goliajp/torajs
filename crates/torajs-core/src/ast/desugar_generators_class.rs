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

use super::{Ast, ClassCtor, ClassMethod, Expr, ExprId, Param, Stmt, Visibility};

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
    ctor_destr_lets: Vec<Stmt>,
    destr_leaf_fields: &[String],
    next_method: ClassMethod,
    return_method: ClassMethod,
    throw_method: ClassMethod,
    captures_arguments: bool,
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
        // The `let`'s annotation is about to be the only record that
        // this field holds a generator object — see
        // `Ast::generator_local_classes`.
        if lty.starts_with("__Gen_") {
            ast.generator_local_classes
                .insert(lname.clone(), lty.clone());
        }
    }
    let mut ctor_body_with_params = ctor_body_base;
    for p in &gen_params {
        // Synthetic destr-pattern params (`__param_destr_N`) stay
        // plain ctor params: the moved destr lets below consume them,
        // the state machine never reads them, and a struct-annotated
        // field would need a struct zero-init the factory's default
        // literal can't provide.
        if p.name.starts_with("__param_destr_") {
            continue;
        }
        let pname = p.name.clone();
        let pty = p.type_ann.clone().unwrap_or_else(|| "any".into());
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
    // Param destructuring runs inside the ctor (eager, at the factory
    // call — ES §9.2 timing): the parser-synthesized lets execute
    // against the ctor params verbatim, then each leaf binding is
    // stored into its class field for the state machine to read.
    for leaf in destr_leaf_fields {
        class_fields.push((leaf.clone(), "any".into()));
    }
    ctor_body_with_params.extend(ctor_destr_lets);
    for leaf in destr_leaf_fields {
        let this_id = ast.add_expr(Expr::This);
        let f_member = ast.add_expr(Expr::Member {
            obj: this_id,
            name: leaf.clone(),
        });
        let leaf_ident = ast.add_expr(Expr::Ident(leaf.clone()));
        let assign = ast.add_expr(Expr::Assign {
            target: f_member,
            value: leaf_ident,
        });
        ctor_body_with_params.push(Stmt::Expr(assign));
    }
    // RFC 20260801-arguments-method-face knife 2 — the captured
    // argv rides a trailing ctor param into a class field literally
    // named `arguments` (the body rewrite minted `this.arguments`
    // reads; the ctor's own param named `arguments` is exactly the
    // sloppy shadow the arguments pass already exempts).
    let mut ctor_params = gen_params.clone();
    if captures_arguments {
        class_fields.push(("arguments".into(), "any".into()));
        ctor_params.push(Param {
            name: "arguments".into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        });
        let this_id = ast.add_expr(Expr::This);
        let target = ast.add_expr(Expr::Member {
            obj: this_id,
            name: "arguments".into(),
        });
        let value = ast.add_expr(Expr::Ident("arguments".into()));
        let assign = ast.add_expr(Expr::Assign { target, value });
        ctor_body_with_params.push(Stmt::Expr(assign));
    }
    let ctor_with_params = ClassCtor {
        params: ctor_params,
        body: ctor_body_with_params,
    };

    // ES §27.5.1.5 — `%GeneratorPrototype%[@@iterator]` answers the
    // generator itself, so a generator object IS its own iterable.
    // Without this method `for (const v of gen())` had no iterator to
    // resolve (the P5.3 Phase B protocol path looks up
    // `__cm_<C>____sym_Symbol_iterator__` and panics when absent), and
    // the any-lane runtime kernel has no name to probe either.
    let self_this = ast.add_expr(Expr::This);
    let self_iter_method = ClassMethod {
        name: "__sym_Symbol_iterator__".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Some(class_name.clone()),
        body: vec![Stmt::Return(Some(self_this))],
        is_abstract: false,
        visibility: Visibility::Public,
        accessor_kind: None,
        span: crate::lexer::Span { start: 0, end: 0 },
    };

    appended.push(Stmt::ClassDecl {
        name: class_name.clone(),
        type_params: Vec::new(),
        parent: None,
        is_abstract: false,
        fields: class_fields,
        static_init: Vec::new(),
        ctor: Some(ctor_with_params),
        methods: vec![next_method, return_method, throw_method, self_iter_method],
        static_methods: Vec::new(),
    });

    // Replace the original generator FnDecl with a thin factory
    // that returns `new __Gen_<name>(args)`.
    let mut factory_args: Vec<ExprId> = gen_params
        .iter()
        .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
        .collect();
    if captures_arguments {
        // `[...arguments]` — the inline-spread rewrite compiles this
        // under EVERY argc mode: a static-argv-qualified factory
        // expands the exact call-site argv (injected extras
        // included), a non-qualified one degrades to the declared
        // params (parity with the historical next()-arity fold) —
        // never a compile break, never a loud regression on
        // declaration-only tests.
        let src = ast.add_expr(Expr::Ident("arguments".into()));
        let spread = ast.add_expr(Expr::Spread { expr: src });
        factory_args.push(ast.add_expr(Expr::Array(vec![spread])));
    }
    let new_expr = ast.add_expr(Expr::New {
        class_name: class_name.clone(),
        args: factory_args,
        type_args: vec![],
    });
    let factory_body = vec![Stmt::Return(Some(new_expr))];
    // Reflection link (RFC 20260713 blade 5): `g.prototype` /
    // `x instanceof g` resolve through the __Gen class's prototype
    // substrate via this map.
    ast.generator_factory_classes
        .insert(gen_name.clone(), class_name.clone());
    // the factory replaces the user's `function*` declaration in
    // place -- keep its source range so toString answers the
    // generator text the user wrote (B1b)
    let orig_span = match &ast.stmts[idx] {
        Stmt::FnDecl { span, .. } => *span,
        _ => crate::lexer::Span { start: 0, end: 0 },
    };
    ast.stmts[idx] = Stmt::FnDecl {
        name: gen_name,
        type_params: Vec::new(),
        params: gen_params,
        return_type: Some(class_name),
        body: factory_body,
        is_generator: false,
        span: orig_span,
    };
}
