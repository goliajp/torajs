//! Why a captured-scope class stays off the ES5 lane.
//!
//! Split out of the parent for size, but the boundary is a real one:
//! the parent answers "what the lane does with the ones it takes",
//! this answers "which ones it takes at all". The computed-key
//! vocabulary (`sentinel_index`, `own_computed_members`,
//! `key_binding`) stays in the parent because both halves speak it.

use super::super::free_vars::free_vars_of_body;
use super::super::{Ast, Param, Stmt, Visibility};
use super::{key_binding, own_computed_members, sentinel_index};

/// Why this class stays off the lane, phrased for the person who wrote
/// it. `None` means it routes.
///
/// A nested class only reaches the checker when the hoist declined it,
/// and the hoist declines exactly the ones that read something from
/// around them — so whoever asks this question already knows the class
/// captures, and what is missing is the SECOND half of the sentence.
pub(super) fn decline_reason(ast: &Ast, s: &Stmt) -> Option<&'static str> {
    let Stmt::ClassDecl {
        name,
        type_params,
        parent,
        is_abstract,
        // A computed INSTANCE field never reaches here: the parser
        // sends it straight to `field_inits`, which by now is a keyed
        // write in the constructor prefix. So `fields` holds only
        // ordinary declared names, and there is nothing to ask it.
        fields: _,
        static_init,
        ctor,
        methods,
        static_methods,
    } = s
    else {
        return Some("it is not a class declaration");
    };
    if parent.is_some() {
        return Some("it extends another class");
    }
    if *is_abstract {
        return Some("it is abstract");
    }
    if !type_params.is_empty() {
        return Some("it has type parameters");
    }
    // Static fields and static blocks both run at class-evaluation
    // time with `this` bound to the class object, and an initializer
    // saying `this` would read the ENCLOSING `this` once inlined at
    // the store. A static METHOD has no such problem: `K.s =
    // function () { … this … }` called as `K.s()` binds `this` to K,
    // which is what §10.2.1.2 wants.
    //
    // A static field with a COMPUTED name parks in its own side table
    // instead of in `static_init`, so it has to be asked about
    // separately — but it is a static field and declines as one.
    if !static_init.is_empty()
        || ast
            .class_computed_static_fields
            .iter()
            .any(|(c, _, _)| c == name)
    {
        return Some("it has a static field or a static block");
    }
    if let Some(m) = methods.iter().chain(static_methods.iter()).find(|m| {
        m.is_abstract
            || m.accessor_kind.is_some()
            || m.visibility != Visibility::Public
            || (m.name.starts_with("__") && sentinel_index(&m.name).is_none())
    }) {
        return Some(if m.accessor_kind.is_some() {
            "it has a getter or a setter"
        } else if m.is_abstract {
            "it has an abstract method"
        } else if m.visibility != Visibility::Public {
            "it has a private or protected member"
        } else {
            "it has a generator or otherwise compiler-rewritten method"
        });
    }
    // A computed name on a static member lowers to `K[<key>] = …`,
    // and the object position of a KEYED member cannot join the
    // receiver-safe use shapes that keep the constructor's `this`
    // promoted. That list excludes `.call` / `.apply` / `.bind`
    // because those read the binding in order to invoke it — an
    // exclusion spelled as three names, which a key that is only
    // known at run time defeats. An instance member is unaffected:
    // it lowers onto `K.prototype`, where the binding still stands
    // under a named member.
    if static_methods
        .iter()
        .any(|m| sentinel_index(&m.name).is_some())
    {
        return Some("a static method in it has a computed name");
    }
    // A body naming a compiler-minted global is not ordinary user
    // nesting: `__cm_gen_<C>__<m>` is the top-level generator method
    // the parser hoisted out (it cannot capture either), and
    // `__supercall__*` belongs to a parent link rejected above. The
    // caller already decided this class captures; the walk here only
    // screens for those names, so the prebound set need cover no more
    // than what would otherwise report a `__` name spuriously.
    let mut prebound = vec![name.clone(), "arguments".to_string()];
    // The ctor prefix a computed instance field turns into reads the
    // evaluated key out of `__ccmk_<C>_<n>`; this lane declares those
    // (see `lower_to_es5`), so they are bound, not minted-and-free.
    let ctor_body: &[Stmt] = ctor.as_ref().map_or(&[], |c| c.body.as_slice());
    let member_names: Vec<&str> = methods
        .iter()
        .chain(static_methods.iter())
        .map(|m| m.name.as_str())
        .collect();
    prebound.extend(
        own_computed_members(ast, name, ctor_body, &member_names)
            .into_iter()
            .map(|n| key_binding(name, n)),
    );
    let synthetic_free = |params: &[Param], body: &[Stmt]| -> bool {
        let mut bound = prebound.clone();
        bound.extend(params.iter().map(|p| p.name.clone()));
        free_vars_of_body(ast, &bound, body)
            .iter()
            .any(|n| n.starts_with("__"))
    };
    let ctor_synthetic = ctor
        .as_ref()
        .is_some_and(|c| synthetic_free(&c.params, &c.body));
    if ctor_synthetic
        || methods
            .iter()
            .chain(static_methods.iter())
            .any(|m| synthetic_free(&m.params, &m.body))
    {
        return Some("a body in it names something the compiler minted");
    }
    None
}
