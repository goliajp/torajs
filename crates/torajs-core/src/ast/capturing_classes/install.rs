//! How a capturing class's members and static initializers INSTALL
//! onto the downgraded binding — split from `capturing_classes.rs`
//! when the 394-05 static-init wrappers pushed it past the 500-line
//! hard limit. The parent answers "how does this class lower"; this
//! sibling answers "how does one member land on the binding".
//! Bodies verbatim.

use super::super::{Ast, Expr, ExprId, Stmt};

/// `(function () { <body> }).call(<class binding>)` — the wrapper that
/// hands a static initializer's `this` the class object (§15.7.14).
/// Registered in `fn_expr_exprs` so it is function-this, not lexical.
pub(super) fn call_bound_to_class(ast: &mut Ast, body: Vec<Stmt>, class_binding: &str) -> ExprId {
    let f = ast.add_expr(Expr::ArrowFn {
        params: Vec::new(),
        return_type: None,
        body,
    });
    ast.fn_expr_exprs.insert(f);
    let callee = ast.add_expr(Expr::Member {
        obj: f,
        name: "call".to_string(),
    });
    let recv = ast.add_expr(Expr::Ident(class_binding.to_string()));
    ast.add_expr(Expr::Call {
        callee,
        args: vec![recv],
    })
}

/// The descriptor a class member gets in §15.7.14: an accessor half is
/// `{ get|set, configurable }`, an ordinary method is
/// `{ value, writable, configurable }`. Neither says `enumerable`,
/// which is the point — a property `defineProperty` creates without it
/// is non-enumerable, exactly as a class declares it.
///
/// A getter and a setter of the same name arrive as two members and so
/// emit two calls. That is the spec's own shape (each MethodDefinition
/// is its own DefinePropertyOrThrow), and the second call keeps the
/// first half: a descriptor naming only `[[Set]]` leaves an existing
/// `[[Get]]` alone (§10.1.6.3 step 4).
pub(super) fn descriptor_fields(
    ast: &mut Ast,
    kind: Option<super::super::AccessorKind>,
    func: ExprId,
) -> Vec<(String, ExprId)> {
    let yes = ast.add_expr(Expr::Bool(true));
    match kind {
        Some(super::super::AccessorKind::Getter) => vec![("get".into(), func)],
        Some(super::super::AccessorKind::Setter) => vec![("set".into(), func)],
        None => {
            let writable = ast.add_expr(Expr::Bool(true));
            vec![("value".into(), func), ("writable".into(), writable)]
        }
    }
    .into_iter()
    .chain([("configurable".to_string(), yes)])
    .collect()
}

/// `Object.defineProperty(<recv>, <key>, { … })`.
///
/// The descriptor stays a BARE object literal. Wrapping it in `as any`
/// reads fine and even runs when hand-written, but the fnexpr-this face
/// walk requires an inline `ObjectLit` at exactly this argument — an
/// `As` in between hands it zero faces, and the function's `this` stays
/// a capture nobody binds.
pub(super) fn define_member(
    ast: &mut Ast,
    recv: ExprId,
    key: ExprId,
    fields: Vec<(String, ExprId)>,
) -> ExprId {
    let desc = ast.add_expr(Expr::ObjectLit { fields });
    let object = ast.add_expr(Expr::Ident("Object".to_string()));
    let callee = ast.add_expr(Expr::Member {
        obj: object,
        name: "defineProperty".to_string(),
    });
    ast.add_expr(Expr::Call {
        callee,
        args: vec![recv, key, desc],
    })
}
