//! RFC 20260805-async-fn-state-machine blade 3 (D3) — async class
//! METHODS: `class C { async m(a) { … this … } }` and `static async`.
//!
//! The receiver is the one thing a method body has that a declaration
//! does not, and the class generator-method path already settled how a
//! generator body gets one: as its leading PARAMETER, `__genrecv`
//! (`parser/parse_class_decl_generator.rs` — a generator's own `this`
//! is the `__Gen_*` instance, and `__this` is load-bearing in the
//! class-method world). This pass does the same at desugar time: the
//! body's `this` becomes `Ident("__genrecv")`, awaits become yields,
//! the body moves into a top-level `function* __async_gen__<C>__<m>
//! (__genrecv, params)`, and the method keeps its signature with the
//! body `return __async_drive(() => __async_gen__<C>__<m>(this, params))`
//! — an ordinary method whose `this` `desugar_classes` rewrites as
//! always (a static one records its `this` in `static_this_sites`,
//! as the parser does for its forwarders).
//!
//! Not moved, keeping the synchronous method lowering: accessors and
//! abstract bodies; a body with `super` (the generator has no
//! HomeObject) or a private name (`#x` on a `__genrecv` receiver is a
//! brand check this pass has not wired); a generic class or method
//! (the mono machinery would need the minted generator twinned); and
//! every shape the declaration gate refuses.

use super::desugar_async_sm::{
    recv_generator_params, recv_wrapper_body, rewrite_body, wrapper_params,
};
use super::desugar_async_sm_gate::blocks_move;
use super::desugar_with::walk;
use super::{Ast, ClassMethod, Expr, ExprId, GEN_RECV_PARAM, Stmt, mangle_key};

/// Rewrite every movable async method. Answers whether any moved.
pub(super) fn rewrite_async_methods(ast: &mut Ast) -> bool {
    let mut moved_any = false;
    let mut minted: Vec<Stmt> = Vec::new();
    for idx in 0..ast.stmts.len() {
        let Stmt::ClassDecl {
            name: cname,
            type_params,
            ..
        } = &ast.stmts[idx]
        else {
            continue;
        };
        if !type_params.is_empty() {
            continue;
        }
        let cname = cname.clone();
        for is_static in [false, true] {
            let prefix = if is_static { "__sm_" } else { "__cm_" };
            let count = match &ast.stmts[idx] {
                Stmt::ClassDecl {
                    methods,
                    static_methods,
                    ..
                } => {
                    if is_static {
                        static_methods.len()
                    } else {
                        methods.len()
                    }
                }
                _ => 0,
            };
            for mi in 0..count {
                let m = match &ast.stmts[idx] {
                    Stmt::ClassDecl {
                        methods,
                        static_methods,
                        ..
                    } => {
                        if is_static {
                            static_methods[mi].clone()
                        } else {
                            methods[mi].clone()
                        }
                    }
                    _ => unreachable!(),
                };
                let mangled = format!("{prefix}{cname}__{}", mangle_key(&m.name));
                if !ast.async_fns.contains(&mangled)
                    || ast.async_ssa.fns.contains(&mangled)
                    || !movable(ast, &m)
                {
                    continue;
                }
                let Some((forwarder, generator)) =
                    move_method(ast, &cname, is_static, &mangled, &m)
                else {
                    continue;
                };
                if let Stmt::ClassDecl {
                    methods,
                    static_methods,
                    ..
                } = &mut ast.stmts[idx]
                {
                    if is_static {
                        static_methods[mi] = forwarder;
                    } else {
                        methods[mi] = forwarder;
                    }
                }
                minted.push(generator);
                ast.async_sm_fns.insert(mangled);
                moved_any = true;
            }
        }
    }
    ast.stmts.extend(minted);
    moved_any
}

fn movable(ast: &Ast, m: &ClassMethod) -> bool {
    m.accessor_kind.is_none()
        && !m.is_abstract
        && m.type_params.is_empty()
        && !m.params.iter().any(|p| p.name.starts_with("__"))
        // the parser spells `#m` as `__priv_<C>__m` (parse_class_decl_member)
        && !m.name.lossy().starts_with("__priv_")
        && !blocks_move(ast, &m.params, &m.body)
        && !m.body.iter().any(|s| stmt_has_super_or_private(ast, s))
}

fn stmt_has_super_or_private(ast: &Ast, s: &Stmt) -> bool {
    walk::stmt_exprs(s)
        .into_iter()
        .any(|e| expr_has_super_or_private(ast, e))
        || walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt_has_super_or_private(ast, c))
}

fn expr_has_super_or_private(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Super { .. } => true,
        Expr::Member { name, .. } | Expr::OptChain { name, .. }
            if name.starts_with('#') || name.starts_with("__priv_") =>
        {
            true
        }
        Expr::Ident(n) if n.starts_with("__super") => true,
        Expr::ArrowFn { body, .. } => body.iter().any(|s| stmt_has_super_or_private(ast, s)),
        _ => walk::expr_children(ast, eid)
            .into_iter()
            .any(|c| expr_has_super_or_private(ast, c)),
    }
}

/// The forwarder method and the generator FnDecl for one async method.
fn move_method(
    ast: &mut Ast,
    cname: &str,
    is_static: bool,
    mangled: &str,
    m: &ClassMethod,
) -> Option<(ClassMethod, Stmt)> {
    let mut moved = m.body.clone();
    if !rewrite_body(ast, &mut moved) {
        return None;
    }
    super::desugar_async_sm_this::rewrite_this(ast, &moved, GEN_RECV_PARAM);
    let gen_name = format!("__async_gen__{}", &mangled["__cm_".len()..]);
    let generator = Stmt::FnDecl {
        name: gen_name.clone(),
        type_params: Vec::new(),
        params: recv_generator_params(&m.params, GEN_RECV_PARAM),
        return_type: Some("any".into()),
        body: moved,
        is_generator: true,
        span: m.span,
    };
    // `__async_drive(() => __async_gen__C__m(this, a, b))` — the
    // receiver rides in front, read by the forwarder's own `this`.
    let gen_ident = ast.add_expr(Expr::Ident(gen_name));
    let this_eid = ast.add_expr(Expr::This);
    if is_static {
        ast.static_this_sites.insert(this_eid, cname.to_string());
    }
    let body = recv_wrapper_body(ast, gen_ident, &m.params, this_eid);
    let forwarder = ClassMethod {
        name: m.name.clone(),
        type_params: Vec::new(),
        params: wrapper_params(ast, &m.params),
        return_type: Some("Promise<any>".into()),
        body,
        is_abstract: false,
        visibility: m.visibility,
        accessor_kind: None,
        span: m.span,
    };
    Some((forwarder, generator))
}
