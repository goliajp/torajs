//! Per-evaluation class units — the AST half (RFC
//! 20260917-class-eval-unit K1).
//!
//! A capture-free class written where it can run more than once — a
//! function body, a loop body, a method, an arrow — is hoisted to the
//! top level so the static class machinery (layout, vtable, typed
//! dispatch) serves it. §15.7.14 says each evaluation is a NEW class
//! object with a new prototype; the hoist made every evaluation share
//! one (`outer() === outer()`, `new (outer())() instanceof outer()`).
//!
//! For a class this module admits, the hoist leaves, where the class
//! was written,
//!
//! ```text
//! let C: any = __cunit_C();
//! ```
//!
//! and `__cunit_C` mints the class object and its prototype and wires
//! them (`__torajs_class_unit_wire`, torajs-meta `classmeta::unit`).
//! Every `new C(…)` passes that binding to the factory, which stores it
//! in a hidden instance slot (`__priv__unit`, a private-element row, so
//! no reflection face ever sees it) and hands it to the constructor as
//! `new.target`. The runtime answers an instance's prototype from that
//! slot. The class still registers once, as the shared template its
//! typed code was always compiled against.
//!
//! Admission is deliberately narrow (the RFC's K1 whitelist); anything
//! else keeps today's shared identity:
//!
//! - W1 no `extends`, and no class extends this one; W5 not generic,
//!   not abstract; W6 no builtin parent;
//! - W2/W3/W4 no private, static, accessor or computed member;
//! - W7 the constructor returns no value; W16 reads no `arguments`;
//! - W8 no member body names the class (the body would read the
//!   registered template);
//! - W12 not a class expression written inside a class body;
//! - W14 the class is referenced only inside the container it was
//!   declared in, and its name is declared exactly once in the program.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Param, PropKey, StaticInit, Stmt};

/// The hidden instance slot holding the evaluation's class object.
/// Spelled as a private-element row (`__priv_<C>__<x>` with an empty
/// class name, which no class declaration produces).
pub const UNIT_SLOT: &str = "__priv__unit";

/// The synthesized runtime wiring call (`classmeta::unit`).
pub const UNIT_WIRE: &str = "__torajs_class_unit_wire";

/// The factory's leading parameter carrying the evaluation's class.
pub const UNIT_PARAM: &str = "__unit";

/// Whether the ClassDecl `cls` (about to be hoisted out of a
/// multi-evaluation container) has a unit-class shape. The name-keyed
/// reference census is [`refs_stay_local`], asked after the hoist's
/// rename has made the name unique.
pub(super) fn admits_shape(ast: &Ast, cls: &Stmt) -> bool {
    let Stmt::ClassDecl {
        name,
        type_params,
        parent,
        is_abstract,
        fields,
        static_init,
        ctor,
        methods,
        static_methods,
    } = cls
    else {
        return false;
    };
    let plain = |k: &PropKey| {
        let k = k.lossy();
        !["__priv_", "__ccm_", "__getter_", "__setter_"]
            .iter()
            .any(|p| k.starts_with(p))
    };
    // A runtime-value parent anywhere (`class D extends f()`) reaches
    // its parent's constructor through the by-cell registry, which no
    // unit class object is in.
    if parent.is_some()
        || !ast.es5_value_parents.is_empty()
        || !type_params.is_empty()
        || *is_abstract
        || !static_init.is_empty()
        || !static_methods.is_empty()
        || !fields.iter().all(|(k, _)| plain(k))
        || !methods
            .iter()
            .all(|m| plain(&m.name) && m.type_params.is_empty())
        || ast.builtin_class_parents.contains_key(name)
        || ast.class_expr_deferred.contains(name)
        || ast
            .class_parents
            .values()
            .any(|p| p.as_deref() == Some(name.as_str()))
        || ast.class_computed_keys.keys().any(|(c, _)| c == name)
        || ast
            .class_computed_instance_fields
            .iter()
            .any(|(c, _, _)| c == name)
        || ast.generator_factory_classes.contains_key(name)
        || ast.static_this_sites.values().any(|c| c == name)
    {
        return false;
    }
    let mut bodies: Vec<&[Stmt]> = methods.iter().map(|m| m.body.as_slice()).collect();
    if let Some(c) = ctor {
        if c.body.iter().any(returns_value) {
            return false;
        }
        bodies.push(&c.body);
    }
    let mut in_bodies = Occurrences::default();
    for b in bodies {
        in_bodies.list(ast, b, name);
    }
    in_bodies.name == 0 && !in_bodies.arguments
}

/// Whether every reference to `name` in the program sits in the
/// container that declares it (`container[idx]`, excluded).
pub(super) fn refs_stay_local(ast: &Ast, container: &[Stmt], idx: usize, name: &str) -> bool {
    let mut in_container = Occurrences::default();
    for (j, s) in container.iter().enumerate() {
        if j != idx {
            in_container.stmt(ast, s, name);
        }
    }
    let program = ast.exprs.iter().filter(|e| names_class(e, name)).count();
    in_container.name == program
}

/// Turn the admitted class at `container[idx]` into a unit class: the
/// hidden slot joins its fields, and the answer is the statement left
/// where the class was written plus the `__cunit_` function to hoist
/// beside it. `binding` is the name the container refers to the class
/// by (the hoist's collision rename, when one happened).
pub(super) fn make_unit(ast: &mut Ast, cls: &mut Stmt, binding: &str) -> (Stmt, Stmt) {
    let Stmt::ClassDecl { fields, ctor, .. } = cls else {
        unreachable!("admitted a ClassDecl");
    };
    fields.push((PropKey::from(UNIT_SLOT), "any".to_string()));
    let length = ctor.as_ref().map_or(0, |c| {
        c.params
            .iter()
            .take_while(|p| p.default.is_none() && !p.is_rest)
            .count()
    });
    ast.eval_unit_classes.insert(binding.to_string());
    let display = ast
        .class_expr_display_names
        .get(binding)
        .cloned()
        .unwrap_or_else(|| binding.to_string());

    let proto_init = ast.add_expr(Expr::ObjectLit { fields: Vec::new() });
    let proto_ref = ast.add_expr(Expr::Ident("__cup".into()));
    let name_lit = ast.add_expr(Expr::String(display.into()));
    let length_lit = ast.add_expr(Expr::Number(length as f64));
    let class_init = ast.add_expr(Expr::ObjectLit {
        fields: vec![
            (PropKey::from("name"), name_lit),
            (PropKey::from("prototype"), proto_ref),
            (PropKey::from("length"), length_lit),
        ],
    });
    let wire = ast.add_expr(Expr::Ident(UNIT_WIRE.into()));
    let class_arg = ast.add_expr(Expr::Ident("__cuc".into()));
    let proto_arg = ast.add_expr(Expr::Ident("__cup".into()));
    let tag_arg = ast.add_expr(Expr::String(binding.to_string().into()));
    let wire_call = ast.add_expr(Expr::Call {
        callee: wire,
        args: vec![class_arg, proto_arg, tag_arg],
    });
    let ret = ast.add_expr(Expr::Ident("__cuc".into()));
    let any_let = |name: &str, init: ExprId| Stmt::LetDecl {
        mutable: false,
        name: name.into(),
        type_ann: Some("any".into()),
        init,
        is_var: false,
    };
    let fn_name = format!("__cunit_{binding}");
    let unit_fn = Stmt::FnDecl {
        name: fn_name.clone(),
        type_params: Vec::new(),
        params: Vec::<Param>::new(),
        return_type: Some("any".into()),
        body: vec![
            any_let("__cup", proto_init),
            any_let("__cuc", class_init),
            Stmt::Expr(wire_call),
            Stmt::Return(Some(ret)),
        ],
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    };
    let callee = ast.add_expr(Expr::Ident(fn_name));
    let init = ast.add_expr(Expr::Call {
        callee,
        args: Vec::new(),
    });
    let residual = Stmt::LetDecl {
        mutable: true,
        name: binding.to_string(),
        type_ann: Some("any".into()),
        init,
        is_var: false,
    };
    (residual, unit_fn)
}

fn names_class(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::Ident(n) if n == name)
        || matches!(e, Expr::New { class_name, .. } if class_name == name)
}

fn returns_value(s: &Stmt) -> bool {
    match s {
        Stmt::Return(Some(_)) => true,
        Stmt::FnDecl { .. } => false,
        other => stmt_children_ref(other).into_iter().any(returns_value),
    }
}

/// Occurrences of the class name (and of `arguments`) in a subtree,
/// every nested function and class body included.
#[derive(Default)]
struct Occurrences {
    name: usize,
    arguments: bool,
}

impl Occurrences {
    fn list(&mut self, ast: &Ast, stmts: &[Stmt], name: &str) {
        for s in stmts {
            self.stmt(ast, s, name);
        }
    }

    fn stmt(&mut self, ast: &Ast, s: &Stmt, name: &str) {
        match s {
            Stmt::FnDecl { body, .. } => self.list(ast, body, name),
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                if let Some(c) = ctor {
                    self.list(ast, &c.body, name);
                }
                for m in methods.iter().chain(static_methods) {
                    self.list(ast, &m.body, name);
                }
                for i in static_init {
                    match i {
                        StaticInit::Block(b) => self.list(ast, b, name),
                        StaticInit::Field(f) => self.expr(ast, f.init, name),
                    }
                }
            }
            _ => {}
        }
        // An arrow's body is reached through its own node.
        for e in stmt_exprs(s) {
            self.expr(ast, e, name);
        }
        for c in stmt_children_ref(s) {
            self.stmt(ast, c, name);
        }
    }

    fn expr(&mut self, ast: &Ast, eid: ExprId, name: &str) {
        let e = ast.get_expr(eid);
        if names_class(e, name) {
            self.name += 1;
        }
        if matches!(e, Expr::Ident(n) if n == "arguments") {
            self.arguments = true;
        }
        if let Expr::ArrowFn { body, .. } = e {
            self.list(ast, body, name);
            return;
        }
        for c in expr_children(ast, eid) {
            self.expr(ast, c, name);
        }
    }
}
