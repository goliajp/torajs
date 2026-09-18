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
//! - W3/W4 no public instance accessor, no computed member. K2 admits
//!   private methods and accessors, K3 private fields: their brand check
//!   also compares the receiver's evaluation with the one `__this` runs
//!   in, so no member body may hold a `function` expression, whose
//!   `this` is not the instance. K4 admits static methods, accessors,
//!   fields and blocks, private ones included: they run inside
//!   `__cunit_C` on the evaluation's own class object;
//! - W7 the constructor returns no value; W16 reads no `arguments`;
//! - W8 no member body names the class (the body would read the
//!   registered template);
//! - W12 not a class expression written inside a class body;
//! - W14 the class is referenced only inside the container it was
//!   declared in, and its name is declared exactly once in the program.

use super::{Ast, Expr, ExprId, Param, PropKey, StaticInit, Stmt};

/// The hidden instance slot holding the evaluation's class object.
/// Spelled as a private-element row (`__priv_<C>__<x>` with an empty
/// class name, which no class declaration produces).
pub const UNIT_SLOT: &str = "__priv__unit";

/// The synthesized runtime wiring call (`classmeta::unit`).
pub const UNIT_WIRE: &str = "__torajs_class_unit_wire";

/// The factory's leading parameter carrying the evaluation's class.
pub const UNIT_PARAM: &str = "__unit";

/// K4-1 — the synthesized per-evaluation static-method define
/// (`classmeta::define::statics`).
pub const UNIT_STATIC_METHOD_REIFY: &str = "__torajs_unit_static_method_reify";

/// K4-3 — its accessor twin: one pair per evaluation.
pub const UNIT_STATIC_ACCESSOR_REIFY: &str = "__torajs_unit_static_accessor_reify";

/// K4-2 — one static FIELD of one evaluation: §7.3.34 DefineField's
/// CreateDataPropertyOrThrow on the class object. A define, not a
/// write: `static name = "x"` replaces the class object's non-writable
/// `name`, and a private field has no brand to write through yet.
pub const UNIT_STATIC_FIELD_DEFINE: &str = "__torajs_unit_static_field_define";

/// Turn the admitted class at `container[idx]` into a unit class: the
/// hidden slot joins its fields, and the answer is the statement left
/// where the class was written plus the `__cunit_` function to hoist
/// beside it. `binding` is the name the container refers to the class
/// by (the hoist's collision rename, when one happened).
pub(super) fn make_unit(ast: &mut Ast, cls: &mut Stmt, binding: &str) -> (Stmt, Stmt) {
    let Stmt::ClassDecl {
        fields,
        ctor,
        static_methods,
        static_init,
        ..
    } = cls
    else {
        unreachable!("admitted a ClassDecl");
    };
    // K4-1 / K4-3 — one reify per static method, one per accessor
    // NAME (a get / set pair defines once, at its first face).
    let mut static_names: Vec<(PropKey, bool)> = Vec::new();
    for m in static_methods.iter() {
        let is_accessor = m.accessor_kind.is_some();
        if !(is_accessor && static_names.iter().any(|(n, a)| *a && *n == m.name)) {
            static_names.push((m.name.clone(), is_accessor));
        }
    }
    // K4-2 — the static initializers stay on the declaration so
    // `emit_static_inits` still mints each block's strict `__sb_<C>__<i>`
    // function (with a `__unit` param for a unit class); the fields'
    // `__sf_` slots and the template reify it skips. `__cunit_C` runs
    // them here: a field as `__unit.x = init`, a block as a call.
    enum UnitStatic {
        Field(PropKey, ExprId),
        Block(usize),
    }
    let static_inits: Vec<UnitStatic> = static_init
        .iter()
        .enumerate()
        .map(|(i, si)| match si {
            StaticInit::Field(sf) => UnitStatic::Field(sf.name.clone(), sf.init),
            StaticInit::Block(_) => UnitStatic::Block(i),
        })
        .collect();
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
            (PropKey::from("length"), length_lit),
            (PropKey::from("name"), name_lit),
            (PropKey::from("prototype"), proto_ref),
        ],
    });
    let wire = ast.add_expr(Expr::Ident(UNIT_WIRE.into()));
    let class_arg = ast.add_expr(Expr::Ident(UNIT_PARAM.into()));
    let proto_arg = ast.add_expr(Expr::Ident("__cup".into()));
    let tag_arg = ast.add_expr(Expr::String(binding.to_string().into()));
    let wire_call = ast.add_expr(Expr::Call {
        callee: wire,
        args: vec![class_arg, proto_arg, tag_arg],
    });
    // K4-1 — one own entry per static method on THIS evaluation's
    // class object: `__torajs_unit_static_method_reify("C", "m",
    // __cuc)` resolves `__sm_C__m`'s boxed adapter at lowering and
    // mints a mortal face holding the class object (the instance
    // rows' `define_unit_rows` shape, on the class object instead of
    // the prototype).
    let mut static_reifies: Vec<Stmt> = Vec::with_capacity(static_names.len());
    for (name, is_accessor) in static_names {
        let magic = if is_accessor {
            UNIT_STATIC_ACCESSOR_REIFY
        } else {
            UNIT_STATIC_METHOD_REIFY
        };
        let callee = ast.add_expr(Expr::Ident(magic.into()));
        let cname_lit = ast.add_expr(Expr::String(binding.to_string().into()));
        let mname_lit = ast.add_expr(Expr::String(name.into()));
        let unit_arg = ast.add_expr(Expr::Ident(UNIT_PARAM.into()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![cname_lit, mname_lit, unit_arg],
        });
        static_reifies.push(Stmt::Expr(call));
    }
    // K4-2 — static fields and blocks, in declaration order, after
    // every method is defined (§15.7.14 steps 31–35). The class
    // object's local is spelled `__unit` so a static `this` (pass 2
    // rewrites it to `__unit`) resolves inside the factory too; a
    // field is an own data entry (`UNIT_STATIC_FIELD_DEFINE`), a
    // block runs inline.
    for si in static_inits {
        match si {
            UnitStatic::Field(name, init) => {
                let callee = ast.add_expr(Expr::Ident(UNIT_STATIC_FIELD_DEFINE.into()));
                let unit_arg = ast.add_expr(Expr::Ident(UNIT_PARAM.into()));
                let name_lit = ast.add_expr(Expr::String(name.into()));
                let define = ast.add_expr(Expr::Call {
                    callee,
                    args: vec![unit_arg, name_lit, init],
                });
                static_reifies.push(Stmt::Expr(define));
            }
            UnitStatic::Block(idx) => {
                let callee = ast.add_expr(Expr::Ident(format!("__sb_{binding}__{idx}")));
                let unit_arg = ast.add_expr(Expr::Ident(UNIT_PARAM.into()));
                let call = ast.add_expr(Expr::Call {
                    callee,
                    args: vec![unit_arg],
                });
                static_reifies.push(Stmt::Expr(call));
            }
        }
    }
    let ret = ast.add_expr(Expr::Ident(UNIT_PARAM.into()));
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
        body: [
            any_let("__cup", proto_init),
            any_let(UNIT_PARAM, class_init),
            Stmt::Expr(wire_call),
        ]
        .into_iter()
        .chain(static_reifies)
        .chain([Stmt::Return(Some(ret))])
        .collect(),
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
