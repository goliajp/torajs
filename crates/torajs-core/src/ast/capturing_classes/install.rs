//! How a capturing class's members and static initializers INSTALL
//! onto the downgraded binding — split from `capturing_classes.rs`
//! when the 394-05 static-init wrappers pushed it past the 500-line
//! hard limit. The parent answers "how does this class lower"; this
//! sibling answers "how does one member land on the binding".
//! Bodies verbatim.

use super::super::{Ast, ClassMethod, Expr, ExprId, StaticInit, Stmt};
use super::expr_says_this;
use crate::ast::PropKey;

/// Static initialization last, and in source order: §15.7.14 runs
/// field initializers and static blocks at class-definition time,
/// after every member is installed, which is what lets one read
/// the class it belongs to (`static f = K.base + 2`). A plain
/// assignment is the right shape for a field where a method needed
/// `defineProperty` — CreateDataProperty is what the spec performs,
/// so writable / enumerable / configurable all come out true on
/// their own. An initializer that says `this`, and a static block
/// (whose `this` is the class object either way), wrap into
/// `(function () { … }).call(K)` — the same marked-fn-expr mint as
/// every other function this lane emits, so the body's `this`
/// rides the ordinary function-this machinery (394-05).
/// `decline_reason` still refuses computed static names.
pub(super) fn install_static_inits(
    ast: &mut Ast,
    static_init: Vec<StaticInit>,
    name: &str,
    parent: Option<&str>,
    out: &mut Vec<Stmt>,
) {
    for si in static_init {
        // A static initializer's home object is the class, so a
        // `super.m(…)` in it reads through the parent CLASS
        // (405-01 face 3) — rewritten before the `this` probe below,
        // because the rewrite mints `this` as the call receiver.
        if let Some(p) = parent {
            let body_view = match &si {
                StaticInit::Field(sf) => vec![Stmt::Expr(sf.init)],
                StaticInit::Block(stmts) => stmts.clone(),
            };
            super::extends::rewrite_super_sites(ast, &body_view, p, true, name);
        }
        match si {
            StaticInit::Field(sf) => {
                let value = if expr_says_this(ast, sf.init) {
                    let body = vec![Stmt::Return(Some(sf.init))];
                    call_bound_to_class(ast, body, name)
                } else {
                    sf.init
                };
                let recv = ast.add_expr(Expr::Ident(name.to_string()));
                let target = if let Some(ident) = sf.name.as_str() {
                    ast.add_expr(Expr::Member {
                        obj: recv,
                        name: ident.to_string(),
                    })
                } else {
                    let key = ast.add_expr(Expr::String(sf.name.clone().into_wtf8buf()));
                    ast.add_expr(Expr::Index {
                        obj: recv,
                        index: key,
                    })
                };
                out.push(Stmt::Expr(ast.add_expr(Expr::Assign { target, value })));
            }
            StaticInit::Block(stmts) => {
                let call = call_bound_to_class(ast, stmts, name);
                out.push(Stmt::Expr(call));
            }
        }
    }
}

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

/// Computed STATIC fields, last (406-02) — CreateDataProperty's
/// attributes through `defineProperty(K, <key binding>, …)`, whose
/// target and key positions are receiver-safe. The side table holds
/// them apart from `static_init`, so their order relative to the
/// explicit statics is sentinel order, not declaration order
/// (recorded approximation). `decline_reason` admits them only for a
/// program-unique class name, so the rows are provably this class's.
pub(super) fn install_computed_static_fields(
    ast: &mut Ast,
    static_cf: Vec<(usize, ExprId)>,
    name: &str,
    parent: Option<&str>,
    src_name: &str,
    out: &mut Vec<Stmt>,
) {
    for (n, init) in static_cf {
        // Super rewrite BEFORE the `this` probe — the rewrite mints
        // `this` as the call receiver, which is what routes the
        // initializer into the `.call(K)` wrapper below.
        if let Some(p) = parent {
            super::extends::rewrite_super_sites(ast, &[Stmt::Expr(init)], p, true, name);
        }
        let value = if super::expr_says_this(ast, init) {
            let body = vec![Stmt::Return(Some(init))];
            call_bound_to_class(ast, body, name)
        } else {
            init
        };
        let recv = ast.add_expr(Expr::Ident(name.to_string()));
        let key = ast.add_expr(Expr::Ident(super::key_binding(src_name, n)));
        let yes1 = ast.add_expr(Expr::Bool(true));
        let yes2 = ast.add_expr(Expr::Bool(true));
        let yes3 = ast.add_expr(Expr::Bool(true));
        let fields = vec![
            ("value".into(), value),
            ("writable".into(), yes1),
            ("enumerable".into(), yes2),
            ("configurable".into(), yes3),
        ];
        out.push(Stmt::Expr(define_member(ast, recv, key, fields)));
    }
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
) -> Vec<(PropKey, ExprId)> {
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
    .chain([("configurable".into(), yes)])
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
    fields: Vec<(PropKey, ExprId)>,
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

/// Every `this` this lane must stop treating as the class NAME.
///
/// The parser recorded, at the token, that a `this` in a static member
/// body means the class object, and `desugar_classes` pass 2 turns each
/// recorded site into the class NAME. That mint is wrong twice over
/// here: the name has been α-renamed away, and what the renamed binding
/// holds is a function value rather than a class. Drop the registration
/// and those reads become ordinary function `this` — which is what
/// `K.s = function () { … }` invoked as `K.s()` delivers anyway, and it
/// is the same object §10.2.1.2 asked for.
///
/// Static INITIALIZERS are here for the same reason (420-03, which
/// started registering their sites): `install_static_inits` wraps a
/// `this`-saying field initializer or static block in
/// `(function () { … }).call(K)` (394-05), which hands the body the
/// class object as that ordinary receiver.
pub(super) fn drop_static_this_sites(
    ast: &mut Ast,
    src_name: &str,
    static_methods: &[ClassMethod],
    static_init: &[StaticInit],
) {
    // A COMPUTED static field's initializer is not in `static_init` —
    // the parser files it under the side-table sentinel instead, keyed
    // by the name the source used. Missed here it stays registered and
    // pass 2 mints the α-renamed-away name: `unknown identifier C`.
    let computed_inits: Vec<ExprId> = ast
        .class_computed_static_fields
        .iter()
        .filter(|(c, _, _)| c == src_name)
        .map(|(_, _, init)| *init)
        .collect();
    let init_bodies: Vec<Vec<Stmt>> = static_init
        .iter()
        .map(|si| match si {
            StaticInit::Field(f) => vec![Stmt::Expr(f.init)],
            StaticInit::Block(v) => v.clone(),
        })
        .chain(computed_inits.into_iter().map(|e| vec![Stmt::Expr(e)]))
        .collect();
    for body in static_methods
        .iter()
        .map(|m| &m.body)
        .chain(init_bodies.iter())
    {
        for eid in super::this_sites(ast, body) {
            ast.static_this_sites.remove(&eid);
        }
    }
}
