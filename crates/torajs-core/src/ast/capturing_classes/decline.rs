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
pub(super) fn decline_reason(ast: &Ast, s: &Stmt, name_unique: bool) -> Option<&'static str> {
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
        // Static init routes since 394-05 — the emit wraps `this`
        // readers and blocks into `(function () { … }).call(K)`.
        static_init,
        ctor,
        methods,
        static_methods,
    } = s
    else {
        return Some("it is not a class declaration");
    };
    // `extends` admits two parent shapes: a binding this lane minted
    // for a sibling (blade 5 — by the time this class asks, the
    // α-rename has written that `__cc<N>_…` spelling into its
    // `parent` field; 405-01 opened the static-carrying half: the
    // class-side `Object.setPrototypeOf(D, P)` link resolves
    // inherited statics through the function value's user
    // [[Prototype]] chain), or a top-level ROOT real class (405-01
    // face 2 — every runtime face is probed: instance methods ride
    // `Object.create(P.prototype)` + the twin-guarded chain
    // dispatch, statics ride the function value's chain into the
    // class object, `instanceof` walks the chain, and `super(…)`
    // calls the `__ctorany_<P>` receiver-polymorphic ctor twin the
    // admit registers below). A derived / generic / abstract /
    // name-shadowed real class stays out — root-ness is what
    // guarantees the ctor twin never drops its mint.
    if let Some(p) = parent
        && !ast.es5_parent_classes.contains(p)
        && !ast.top_root_real_classes.contains(p)
    {
        return Some(if p.starts_with("__cc") {
            "it extends a class this lane could not claim"
        } else {
            "it extends another class"
        });
    }
    if *is_abstract {
        return Some("it is abstract");
    }
    if !type_params.is_empty() {
        return Some("it has type parameters");
    }
    // Static fields and static blocks run at class-evaluation time
    // with `this` bound to the class object (§15.7.14). A plain field
    // initializer inlines as the assignment the spec performs anyway;
    // one that says `this` — and a static block, whose body always
    // may — is wrapped by the emit into
    // `(function () { … }).call(K)`, which hands the body exactly
    // that binding (394-05; the wrapper registers in `fn_expr_exprs`
    // like every other function this lane mints). So neither declines
    // anymore.
    //
    // A static field with a COMPUTED name routes since 406-02 — its
    // key rides the same `__ccmk_<C>_<n>` binding a computed method
    // reads, and the store is `Object.defineProperty(K, key, { data
    // descriptor })` (CreateDataProperty's attributes), whose target
    // and key are receiver-safe positions. What still gates it is
    // OWNERSHIP: the side-table rows match by class NAME and the
    // field leaves no trace on the class itself, so a name shared by
    // two ClassDecls could install a sibling's fields — the silent
    // direction. The hoist's program-wide census answers uniqueness.
    if !name_unique
        && ast
            .class_computed_static_fields
            .iter()
            .any(|(c, _, _)| c == name)
    {
        return Some(
            "it has a static field with a computed name and shares its class name \
             with another class",
        );
    }
    if let Some(m) = methods.iter().chain(static_methods.iter()).find(|m| {
        m.is_abstract
            || m.visibility != Visibility::Public
            || (m.name.starts_with("__") && sentinel_index(&m.name).is_none())
    }) {
        return Some(if m.is_abstract {
            "it has an abstract method"
        } else if m.visibility != Visibility::Public {
            "it has a private or protected member"
        } else {
            "it has a generator or otherwise compiler-rewritten method"
        });
    }
    // A STATIC accessor and a computed STATIC name used to decline
    // here, both for one reason: either lowering puts the binding
    // itself in an argument (`Object.defineProperty(K, …)`) or under a
    // runtime key (`K[<key>] = …`), and neither was a receiver-safe
    // use shape, so taking them turned the whole class back into
    // `unknown identifier __this`. Rotation 397 admitted the
    // `defineProperty` TARGET argument — §20.1.2.4 never invokes it —
    // and once the key is handed over as data, the keyed store is gone
    // too, along with the reason it could not be admitted (that shape
    // excludes `.call` / `.apply` / `.bind` by NAME, three names a
    // runtime key defeats).
    //
    // A computed name on an accessor rode along once the key stopped
    // being the obstacle: it is the same `__ccmk_<C>_<n>` binding a
    // computed method already reads, and `descriptor_fields` does not
    // care whether the name it is installing under was written out or
    // evaluated. A getter and a setter sharing one key are two
    // MethodDefinitions and so evaluate their key twice, which is the
    // spec's own shape — the second call keeps the first half
    // (§10.1.6.3 step 4).
    // A static body's `super.m(…)` lowers (405-01 face 3: the home
    // object is the class, so the base is the parent CLASS —
    // `P.m.call(this, …)`). What still has no meaning is a bare
    // `super(…)` CALL outside a constructor — spec grammar confines
    // SuperCall to derived ctors, so a static body spelling it stays
    // declined rather than being handed an invented semantics.
    if parent.is_some() {
        let static_super_call = static_methods
            .iter()
            .any(|m| super::extends::body_says_super_call(ast, &m.body))
            || static_init.iter().any(|si| match si {
                super::super::StaticInit::Field(f) => {
                    super::extends::body_says_super_call(ast, &[Stmt::Expr(f.init)])
                }
                super::super::StaticInit::Block(v) => super::extends::body_says_super_call(ast, v),
            });
        if static_super_call {
            return Some("a static member of it calls super()");
        }
    }
    // A body naming a compiler-minted global is not ordinary user
    // nesting: `__cm_gen_<C>__<m>` is the top-level generator method
    // the parser hoisted out (it cannot capture either), and a
    // `__supercall__*` either belongs to a parent link rejected above
    // or — parent admitted — is the marker the emit rewrites, so it
    // is exempt below. The caller already decided this class
    // captures; the walk here only screens for those names, so the
    // prebound set need cover no more than what would otherwise
    // report a `__` name spuriously. The admitted parent's minted
    // binding is a real name in scope, so a body reading it (`P.x`
    // spelled with the source name, α-renamed by now) is prebound
    // rather than "compiler minted".
    let mut prebound = vec![name.clone(), "arguments".to_string()];
    if let Some(p) = parent {
        prebound.push(p.clone());
    }
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
    let allow_supercall = parent.is_some();
    let synthetic_free = |params: &[Param], body: &[Stmt]| -> bool {
        let mut bound = prebound.clone();
        bound.extend(params.iter().map(|p| p.name.clone()));
        free_vars_of_body(ast, &bound, body)
            .iter()
            .any(|n| n.starts_with("__") && !(allow_supercall && n.starts_with("__supercall__")))
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
