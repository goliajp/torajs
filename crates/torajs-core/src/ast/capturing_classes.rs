//! A nested `class` declaration that reads something from the scope
//! around it.
//!
//! ```text
//! { let a = 7; class K { m() { return a } }; new K().m() }   // bun: 7
//! ```
//!
//! [`super::hoist_nested_classes`] gets a nested class into the class
//! machinery by lifting it to the top level, and a top-level body sees
//! no outer local — so it lifts only the capture-free ones and leaves
//! the rest loud. This module is the other half: the ones it leaves.
//!
//! ## Why not an `__env` channel on the methods
//!
//! The obvious mirror of [`super::nested_fns_capture`] is to give
//! method bodies an env. But a class method is dispatched statically
//! (`__cm_<C>__<m>(__this, …)` through a vtable), so an env channel
//! means touching the ABI, the vtable, `class_layouts` and
//! `method_owners` at once — and it still would not answer the real
//! question, which is IDENTITY: `function outer(){ class K {…} }`
//! mints a FRESH class per call of `outer`, closed over that call's
//! environment. tr models a class as a static entity — one vtable, one
//! layout, one tag — and that is the thing per-call identity
//! contradicts.
//!
//! So a class whose identity varies per evaluation belongs on the
//! runtime-value lane, which tr already has (`__torajs_construct`,
//! rotation 250). The textbook way onto it is the ES5 constructor
//! pattern — exactly what Babel and `tsc --target es5` emit:
//!
//! ```text
//! const K: any = function (p) { this.x = p };
//! K.prototype.m = function () { return a + this.x };
//! ```
//!
//! Everything that shape needs already works (RFC
//! 20260814-capturing-nested-class records the probe table): the
//! function expressions get their env from [`super::lift_arrow_fns`],
//! `new K()` routes through the runtime construct, and `instanceof`
//! answers off the prototype link.
//!
//! ## What routes (blades 1-3)
//!
//! Only a shape this lane reproduces faithfully, and only one that is
//! REJECTED today — a whitelist, so no program that currently answers
//! correctly can be pulled in. Constructor, plain public instance and
//! static methods, and accessors; a static body may say `this`, and
//! either kind of member may carry a computed name. Static fields and
//! static blocks route too (394-05): a `this`-free initializer inlines
//! as the plain assignment, and a `this`-reading one — or a block —
//! wraps into `(function () { … }).call(K)` (see
//! `install::call_bound_to_class`). No `extends`, no computed-name
//! STATIC field, no computed-name accessor, no type params, and no
//! compiler-minted free name in a body (`__cm_gen_*` forwarders to a
//! hoisted generator method, `__supercall__*`). Everything else keeps
//! today's loud abort.
//!
//! Recorded deviations are in the RFC: the binding is `const`, `.name`
//! is empty, a declare-only field does not materialize, and the class
//! name is no longer a type name. Every member is installed with the
//! attributes §15.7.14 gives it (see `descriptor_fields`) — statics
//! included, since rotation 397.

mod alias;
mod binding_names;
mod computed_keys;
pub(crate) mod decline;
mod extends;

use super::{Ast, Expr, Stmt};

mod install;
pub(crate) mod new_target;
mod own_binding;
pub(in crate::ast) mod private_fields;
mod return_override;
mod this_sites;
pub(crate) use binding_names::{is_es5_class_binding, is_es5_inner_class_binding};
use computed_keys::{key_binding, sentinel_index};
pub(super) use computed_keys::{keys_of, own_computed_members};
use decline::decline_reason;
pub(crate) use decline::{EXPR_HERITAGE_REASON, unclaimed_class_message};
use install::{define_member, descriptor_fields};
pub(super) use this_sites::{expr_says_this, this_sites};

/// Rewrite the class at `stmts[idx]` when this lane covers it.
/// Returns whether it did.
///
/// The binding is α-renamed first, over the statement list that holds
/// it. Receiver promotion — the thing that gives the constructor's
/// `this` somewhere to come from — pairs a binding to its uses by
/// NAME, and refuses when the name is declared more than once in the
/// program: two functions each declaring `class K` would mint two
/// `let K` and take each other down. A minted name is unique by
/// construction, so the question never arises.
pub(super) fn try_rewrite_capturing_class(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    idx: usize,
    counter: &mut u32,
) -> bool {
    if let Some(why) = decline_reason(ast, &stmts[idx]) {
        // Record it HERE, where the decision is made. The checker is
        // what finally reports it, and by then the tree has moved.
        if let Stmt::ClassDecl { name, .. } = &stmts[idx] {
            let name = name.clone();
            ast.unclaimed_class_reasons.push((name, why));
        }
        return false;
    }
    let Stmt::ClassDecl { name, .. } = &stmts[idx] else {
        unreachable!("routes() matched a ClassDecl");
    };
    let old = name.clone();
    let n = *counter;
    *counter += 1;
    let new = format!("__cc{n}_{old}");
    debug_assert!(is_es5_class_binding(&new));
    // §14.2.3 gives a class declaration TWO bindings: an immutable
    // one in the class's own scope, which the body reads, and a
    // mutable one in the scope the class was written in, which
    // everything else reads. Both are minted, always. rotation 586-05
    // argued a body that reads nothing of itself cannot tell them
    // apart and can have one — `extends` is the witness that it can:
    // a sibling's `super(…)` reaches the parent through the same
    // binding the container holds, so a later `P = null` redirected
    // it and `new Q()` died. The class object needs a cell no write
    // can reach.
    let inner = format!("__cci{n}_{old}");
    // Every class this lane claims is a faithful `extends` target for
    // a later sibling (blade 5; 405-01 opened the static-carrying
    // half — `Object.setPrototypeOf(D, P)` links the class side now
    // that a function value carries a user [[Prototype]] chain).
    // Recorded under the minted name — the rename below writes that
    // same spelling into every sibling's `parent` field.
    ast.es5_parent_classes.insert(new.clone());
    super::hoist_nested_classes_rename::rename_in_stmts(ast, stmts, &old, &new);
    super::hoist_nested_classes_rename::rename_in_class_bodies(ast, &mut stmts[idx], &new, &inner);
    // Where this class's `super(…)` lands — its construct twin when a
    // sibling extends it (`new_target`) — plus the real-parent twin
    // request (405-01); see `extends::record_claim_tables`.
    let own = match new_target::wants_twin(ast, stmts, &stmts[idx], &new) {
        true => new_target::twin_name(&inner),
        false => inner.clone(),
    };
    extends::record_claim_tables(ast, &stmts[idx], &new, &own);
    alias::mint_unique_aliases(ast, stmts, &new, counter);
    let taken = std::mem::replace(&mut stmts[idx], Stmt::Multi(Vec::new()));
    stmts[idx] = lower_to_es5(ast, taken, &old, inner);
    true
}

/// `class K { constructor(p){…} m(){…} }` →
/// `const K: any = function (p) {…}; K.prototype.m = function () {…};`
///
/// The function expressions register in `fn_expr_exprs` rather than
/// reading as arrows: that is what gives them a `.prototype` and a
/// dynamic `this`, both of which a class body assumes.
///
/// `src_name` is the name the class had in the source — the α-rename
/// that gives the binding a program-unique spelling already ran, but
/// both the computed-key side table and the `__ccmk_<C>_<n>` reference
/// the parser baked into the constructor prefix are keyed by the
/// original.
///
/// `inner` is the class-scope binding (§14.2.3). Everything emitted
/// here is the class object itself, so it all stands on that one; the
/// container's mutable binding is minted last, as an alias to it.
fn lower_to_es5(ast: &mut Ast, class: Stmt, src_name: &str, inner: String) -> Stmt {
    let Stmt::ClassDecl {
        name,
        decl_id,
        parent,
        fields,
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = class
    else {
        unreachable!("routes() matched a ClassDecl");
    };
    // This lane's extends machinery keys on the parent BINDING NAME
    // (the α-rename wrote the minted spelling into the heritage
    // Ident). `decline_reason` already refused any class whose
    // heritage is not a bare identifier, so reading the name back
    // here cannot miss. The node itself is consumed by this rewrite —
    // tombstone it, or the orphan Ident reads as a use of the parent
    // binding to every whole-arena analysis (see
    // `Ast::tombstone_expr`).
    let parent_id = parent;
    let parent: Option<String> = ast.parent_ident_name(parent).map(str::to_string);
    if let Some(pid) = parent_id {
        ast.tombstone_expr(pid);
    }
    install::drop_static_this_sites(ast, decl_id, &static_methods, &static_init);
    let outer = name;
    let name = inner;
    // §15.7.14 evaluates every ComputedPropertyName once, in element
    // order, at class-definition time — ahead of anything a method or
    // an initializer does, because those run later (on call, on
    // construction). So all the keys come first, and what the class
    // body says about them is a read of the binding.
    let (own, static_cf) = computed_keys::own_members_and_static_fields(
        ast,
        (src_name, decl_id),
        ctor.as_ref().map_or(&[], |c| c.body.as_slice()),
        (&methods, &static_methods),
    );
    let mut out: Vec<Stmt> = Vec::new();
    // §15.7.14 step 5 (rotation 410) — a value-shaped heritage is
    // gated at class-definition time, BEFORE any member key
    // evaluates: null passed statically (`es5_null_parents` — a
    // legal shape of its own), everything else asks the runtime
    // kernel whether the value is a constructor.
    if let Some(p) = &parent
        && ast.es5_value_parents.contains(p)
        && !ast.es5_null_parents.contains(p)
    {
        let callee = ast.add_expr(Expr::Ident("__torajs_heritage_check".to_string()));
        let pv = ast.add_expr(Expr::Ident(p.clone()));
        out.push(Stmt::Expr(ast.add_expr(Expr::Call {
            callee,
            args: vec![pv],
        })));
    }
    for (n, key) in keys_of(ast, src_name, &own) {
        // 419-01 — ToPropertyKey here, not at whoever reads the
        // binding. A method's keyed install happens at this same
        // position either way, but a FIELD's is the ctor-prefix write,
        // so an unconverted box moved the §7.1.19 conversion (and any
        // `toString` throw with it) to construction time.
        let key_conv = ast.add_expr(Expr::Ident("__torajs_class_computed_key".to_string()));
        let key_any = ast.add_expr(Expr::Call {
            callee: key_conv,
            args: vec![key],
        });
        out.push(Stmt::LetDecl {
            mutable: false,
            name: key_binding(src_name, n),
            type_ann: Some("any".to_string()),
            init: key_any,
            is_var: false,
        });
    }
    // A derived class with no ctor gets the implicit forwarding one
    // §15.7.14 hands it; an explicit body has its super sites lowered
    // against the parent BINDING (blade 5 — the α-rename already put
    // the lane's minted spelling into `parent`).
    let forwards =
        parent.is_some() && (ctor.is_none() || ast.field_init_synth_ctors.contains(src_name));
    let (params, mut body) = extends::ctor_parts(ast, ctor, parent.as_deref(), src_name, &name);
    private_fields::define_in_prefix(ast, &fields, (src_name, forwards), &mut body);
    if let Some(p) = &parent {
        extends::rewrite_super_sites(ast, &body, p, false, &name);
        extends::rewrite_super_prop_sites(ast, &body, p, false, &name);
        return_override::reshape(ast, &mut body);
    }
    let twin = new_target::twin_name(&name);
    let twin = (ast.es5_ctor_forward.get(&outer) == Some(&twin))
        .then(|| new_target::twin_stmt(ast, &params, &body, twin));
    let shown = super::class_globals::class_display_name(ast, src_name).to_string();
    // A body reading `arguments` by index has no construct face, so its
    // `new` runs the call channel too and the guard cannot tell the two
    // apart (recorded residue: its [[Call]] does not throw).
    if !super::arguments_object_walkers::body_has_non_length_arguments_touch(ast, &body) {
        let guard = new_target::call_guard(ast, &shown);
        body.insert(0, guard);
    }
    let ctor_eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type: None,
        body,
    });
    ast.fn_expr_exprs.insert(ctor_eid);
    // The class object is this function, bound under the minted name
    // — and NamedEvaluation would hand it that mint as `.name`. The
    // rename is the lowering's, not the program's: record the source
    // spelling as the expression's self-name, which the fn-name
    // registry lets win over the binding (§15.5.5). Before this
    // `.name` answered `__cci1_C`. Read through the display table: a
    // class the hoist renamed for a name collision (`__hc0_C`) still
    // answers the name it was written with.
    ast.fn_expr_self_names.insert(ctor_eid, shown);
    // §20.2.3.5 — this function IS the class object, so its source
    // text is the class declaration's; without the span `String(C)`
    // printed a native `function` form.
    if let Some(span) = ast.class_decl_spans.get(src_name).copied() {
        ast.set_expr_span(ctor_eid, span);
    }
    out.push(Stmt::LetDecl {
        mutable: false,
        name: name.clone(),
        type_ann: Some("any".to_string()),
        init: ctor_eid,
        is_var: false,
    });
    out.extend(twin);
    // The prototype link goes in BEFORE any member lands: members
    // install on the linked prototype, not the one the function was
    // born with.
    if let Some(p) = &parent {
        extends::proto_chain_stmts(ast, &name, p, &mut out);
    }
    for (mut m, on_prototype) in methods
        .into_iter()
        .map(|m| (m, true))
        .chain(static_methods.into_iter().map(|m| (m, false)))
    {
        // `super.m(…)` in an instance body reads through the parent's
        // prototype; a static body's home object is the class itself,
        // so its super base is the parent CLASS (405-01 face 3).
        if let Some(p) = &parent {
            extends::rewrite_super_sites(ast, &m.body, p, !on_prototype, &name);
            extends::rewrite_super_prop_sites(ast, &m.body, p, !on_prototype, &name);
        }
        let eid =
            super::desugar_async_ssa_gate::method_value(ast, (decl_id, !on_prototype), &mut m);
        ast.set_expr_span(eid, m.span);
        ast.fn_expr_exprs.insert(eid);
        // An instance method hangs off `.prototype`; a static one hangs
        // off the constructor itself, which is what makes `K.s()` bind
        // `this` to K.
        let mut recv = ast.add_expr(Expr::Ident(name.clone()));
        if on_prototype {
            recv = ast.add_expr(Expr::Member {
                obj: recv,
                name: "prototype".to_string(),
            });
        }
        // Every member a class declares is NON-enumerable (§15.7.14),
        // and an assignment makes an enumerable one — `for (const k in
        // new K())` used to answer with the method names. So every
        // member is installed with the attributes the class gave it,
        // the static ones included. That last part is new in rotation
        // 397: a static member passes the BINDING to `defineProperty`
        // rather than standing under `.prototype`, and until the target
        // argument joined the receiver-safe use shapes, doing so took
        // the constructor's `this` off the promotion lane. Statics
        // stayed assignments and stayed wrongly enumerable, and a
        // static accessor or a computed static name — neither of which
        // an assignment can even spell — was declined outright.
        let key = match m.name.as_str().and_then(sentinel_index) {
            Some(n) => ast.add_expr(Expr::Ident(key_binding(src_name, n))),
            None => ast.add_expr(Expr::String(m.name.clone().into())),
        };
        let fields = descriptor_fields(ast, m.accessor_kind, eid);
        out.push(Stmt::Expr(define_member(ast, recv, key, fields)));
    }
    install::install_static_inits(ast, static_init, &name, parent.as_deref(), &mut out);
    install::install_computed_static_fields(
        ast,
        static_cf,
        &name,
        parent.as_deref(),
        src_name,
        &mut out,
    );
    // Last, so the container's binding is only ever handed a fully
    // installed class — every read of it happens after this point.
    out.push(own_binding::outer_alias_stmt(ast, outer, &name));
    Stmt::Multi(out)
}
