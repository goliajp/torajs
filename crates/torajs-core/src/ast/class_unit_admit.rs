//! Admission to the per-evaluation lane (RFC 20260917-class-eval-unit):
//! which nested class may become a unit class. `class_unit` holds what
//! an admitted class then turns into. Split out of it (verbatim move)
//! when K5's family rule pushed the file past its line limit.

use super::class_unit_census::{Occurrences, names_class, returns_value};
use super::{Ast, PropKey, StaticInit, Stmt};

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
    // K3 — a private FIELD. It is a layout slot like any other, and
    // the per-evaluation hidden slot already rides beside the private
    // rows; what makes it admissible is the K2b call-boundary guard,
    // which refuses any body that reaches a private element on
    // another evaluation's receiver.
    let private_field = |k: &PropKey| k.lossy().starts_with("__priv_");
    // K2 — a private method, or either half of a private accessor
    // (`__getter___priv_C__m`); a public accessor stays out.
    let private_method = |k: &PropKey| {
        let k = k.lossy();
        let bare = k
            .strip_prefix("__getter_")
            .or_else(|| k.strip_prefix("__setter_"))
            .unwrap_or(&k);
        bare.starts_with("__priv_")
    };
    // A runtime-value parent anywhere (`class D extends f()`) reaches
    // its parent's constructor through the by-cell registry, which no
    // unit class object is in.
    // K4-2 — a plain static FIELD or a static block rides too: the
    // initializer / block runs inside `__cunit_C` on the evaluation's
    // class object (`__unit.x = init`), in declaration order after
    // the methods, which is §15.7.14's order and position. A computed
    // static field or a field spelled like a method stays out.
    let static_init_admits = static_init.iter().all(|si| match si {
        StaticInit::Field(sf) => {
            (plain(&sf.name) || private_field(&sf.name))
                && !static_methods.iter().any(|m| m.name == sf.name)
        }
        StaticInit::Block(_) => true,
    });
    if parent.is_some()
        || !ast.es5_value_parents.is_empty()
        || !type_params.is_empty()
        || *is_abstract
        || !static_init_admits
        || ast
            .class_computed_static_fields
            .iter()
            .any(|(c, _, _)| c == name)
        // K4-1 — a plain static METHOD rides the unit: its body takes
        // the evaluation's class object as a leading `__unit` param
        // (a static `this` spells it), and `__cunit_C` defines one
        // mortal face per evaluation onto that object.
        // K4-3 — a static ACCESSOR rides the same way: its two faces
        // take `__unit` first and reify as one pair per evaluation.
        // K4-4 — a static PRIVATE method / accessor / field rides as
        // an own entry of the evaluation's class object, so the brand
        // probe (`dynobj_has(receiver, key)`) already answers per
        // evaluation for a foreign object. Another evaluation's class
        // object carries the same key; the call-boundary guard
        // (`method_call_unit_guard`) refuses it.
        || !static_methods
            .iter()
            .all(|m| (plain(&m.name) || private_method(&m.name)) && m.type_params.is_empty())
        || !fields.iter().all(|(k, _)| plain(k) || private_field(k))
        || !methods
            .iter()
            .all(|m| (plain(&m.name) || private_method(&m.name)) && m.type_params.is_empty())
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
    {
        return false;
    }
    // Static method bodies count too (K4-1): `static m() { return
    // S.k }` would read the registered template's `k`.
    let mut bodies: Vec<&[Stmt]> = methods
        .iter()
        .chain(static_methods.iter())
        .map(|m| m.body.as_slice())
        .collect();
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
    // A static initializer naming the class would read the registered
    // template, exactly like a method body (W8).
    for si in static_init {
        match si {
            StaticInit::Field(sf) => in_bodies.expr(ast, sf.init, name),
            StaticInit::Block(b) => in_bodies.list(ast, b, name),
        }
    }
    let has_private = methods
        .iter()
        .chain(static_methods.iter())
        .any(|m| private_method(&m.name));
    in_bodies.name == 0 && !in_bodies.arguments && !(has_private && in_bodies.fn_exprs)
}

/// Whether every reference to `name` in the program sits in the
/// container that declares it (`container[idx]`, excluded), and
/// outside any class body there: a sibling class is hoisted too, and
/// from up there `new D()` builds through the registered template, not
/// through this evaluation's `D` (`new E().make() instanceof D` answered
/// false).
pub(super) fn refs_stay_local(ast: &Ast, container: &[Stmt], idx: usize, name: &str) -> bool {
    let mut in_container = Occurrences {
        skip_class_bodies: true,
        ..Occurrences::default()
    };
    for (j, s) in container.iter().enumerate() {
        if j != idx {
            in_container.stmt(ast, s, name);
        }
    }
    let program = ast.exprs.iter().filter(|e| names_class(e, name)).count();
    in_container.name == program
}
