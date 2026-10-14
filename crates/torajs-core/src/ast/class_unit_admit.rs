//! Admission to the per-evaluation lane (RFC 20260917-class-eval-unit):
//! which nested class may become a unit class. `class_unit` holds what
//! an admitted class then turns into. Split out of it (verbatim move)
//! ahead of K5's family rule.

use super::class_unit_census::{Occurrences, names_class, returns_value, static_self_refs};
use super::{Ast, Expr, PropKey, StaticInit, Stmt};

/// Whether the ClassDecl `cls` (about to be hoisted out of a
/// multi-evaluation container) has a unit-class shape. The name-keyed
/// reference census is [`refs_stay_local`], asked after the hoist's
/// rename has made the name unique.
///
/// `assumed_parent` is the family lookahead's hypothesis (K5): the
/// sibling named so is about to become a unit class, which its own
/// admission depends on (`hoist_nested_classes_unit::family_admits`).
pub(super) fn admits_shape(ast: &Ast, cls: &Stmt, assumed_parent: Option<&str>) -> bool {
    let Stmt::ClassDecl {
        name,
        decl_id,
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
    // K5-4 — a runtime-value parent elsewhere (`class D extends f()`)
    // is no bar: a unit class object reached that way runs its
    // template's constructor twin (`super_call`'s fallback).
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
    // K5-1 — `extends` is admitted for ONE heritage: a unit class of
    // the same container, by name. `__cunit_C(parent)` then wires this
    // evaluation's class object and prototype under that evaluation's.
    // Every other heritage keeps the shared identity.
    let unit_parent = match parent {
        None => None,
        Some(pid) => match ast.get_expr(*pid) {
            Expr::Ident(p)
                if ast.eval_unit_classes.contains(p) || assumed_parent == Some(p.as_str()) =>
            {
                Some(p.as_str())
            }
            _ => return false,
        },
    };
    if !type_params.is_empty()
        || *is_abstract
        || !static_init_admits
        // By OWNER, not by name — see `class_computed_owner`.
        || super::class_computed_owner::has_static_field(ast, *decl_id)
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
        || super::class_computed_owner::has_any(ast, *decl_id)
        || ast
            .class_computed_instance_fields
            .iter()
            .any(|(c, _, _)| c == name)
        || ast.generator_factory_classes.contains_key(name)
    {
        return false;
    }
    // W8 — an instance body naming the class would read the
    // registered template. A STATIC body's references are spelled
    // `__unit` instead (`static_self_refs`), so they only have to be
    // spellable that way.
    if static_self_refs(ast, static_methods, static_init, name).is_none() {
        return false;
    }
    let mut in_bodies = Occurrences::default();
    for m in methods {
        in_bodies.list(ast, &m.body, name);
    }
    if let Some(c) = ctor {
        if c.body.iter().any(returns_value) {
            return false;
        }
        in_bodies.list(ast, &c.body, name);
    }
    let instance_refs = in_bodies.name;
    in_bodies.statics(ast, static_methods, static_init, name);
    let has_private = methods
        .iter()
        .chain(static_methods.iter())
        .any(|m| private_method(&m.name));
    // K6-1 — a class written inside a member body: its own methods'
    // `__this` is ITS instance, but a unit class's brand guard appends
    // `__this` to every private-element site in the outer's bodies,
    // this inner body's included; and when the inner class takes the
    // runtime-value lane its methods become `function` expressions
    // after this census ran. Either way the brand cannot be compared
    // here, so such an outer class keeps the shared identity.
    if instance_refs != 0
        || in_bodies.arguments
        || (has_private && (in_bodies.fn_exprs || in_bodies.holds_class))
    {
        return false;
    }
    // K5-1 — a derived unit class. Its bodies are hoisted like any
    // class's, so they may not name the parent (the name would read
    // the registered template, W8's reason). A `super.x` / `super[k]`
    // site reads its base off the running evaluation's home (K5-3).
    // In a program that re-links prototypes `super.m()` takes the
    // same runtime base. Private elements ride: an instance
    // one's brand compares at the declaring class's depth of the
    // chain (K5-2); a static one is an own entry of the one class
    // object that declared it, which a derived class object does not
    // carry.
    let Some(p) = unit_parent else {
        return true;
    };
    let mut in_derived = Occurrences::default();
    if let Some(c) = ctor {
        in_derived.list(ast, &c.body, p);
    }
    for m in methods.iter().chain(static_methods.iter()) {
        in_derived.list(ast, &m.body, p);
    }
    for si in static_init {
        match si {
            StaticInit::Field(sf) => in_derived.expr(ast, sf.init, p),
            StaticInit::Block(b) => in_derived.list(ast, b, p),
        }
    }
    in_derived.name == 0
}

/// [`refs_stay_local`] for a class the hoist is about to give a
/// collision name: the rename reaches every reference in the
/// container, so what decides is only that none of them sits inside a
/// sibling class body (hoisted, it would read the template).
pub(super) fn refs_avoid_class_bodies(
    ast: &Ast,
    container: &[Stmt],
    idx: usize,
    name: &str,
) -> bool {
    let mut outside = Occurrences {
        skip_class_bodies: true,
        ..Occurrences::default()
    };
    let mut everywhere = Occurrences::default();
    for (j, s) in container.iter().enumerate() {
        if j != idx {
            outside.stmt(ast, s, name);
            everywhere.stmt(ast, s, name);
        }
    }
    outside.name == everywhere.name
}

/// Whether every reference to `name` in the program sits in the
/// container that declares it (`container[idx]`, excluded), and
/// outside any class body there: a sibling class is hoisted too, and
/// from up there `new D()` builds through the registered template, not
/// through this evaluation's `D` (`new E().make() instanceof D` answered
/// false). The class's own static bodies count as local.
pub(super) fn refs_stay_local(ast: &Ast, container: &[Stmt], idx: usize, name: &str) -> bool {
    let mut in_container = Occurrences {
        skip_class_bodies: true,
        ..Occurrences::default()
    };
    for (j, s) in container.iter().enumerate() {
        if j != idx {
            in_container.stmt(ast, s, name);
            // K5 — a sibling's heritage is evaluated HERE, where the
            // class is written, and the residual `let B =
            // __cunit_B(A)` keeps reading this container's binding.
            // True only when that sibling becomes a unit class itself,
            // which `family_admits` is there to guarantee.
            if let Stmt::ClassDecl {
                parent: Some(pid), ..
            } = s
            {
                in_container.expr(ast, *pid, name);
            }
        }
    }
    // The class's own static bodies name it as `__unit` once it is a
    // unit class (`static_self_refs`), which keeps them local too.
    let own = match &container[idx] {
        Stmt::ClassDecl {
            static_methods,
            static_init,
            ..
        } => static_self_refs(ast, static_methods, static_init, name).map_or(0, |v| v.len()),
        _ => 0,
    };
    let program = ast.exprs.iter().filter(|e| names_class(e, name)).count();
    in_container.name + own == program
}
