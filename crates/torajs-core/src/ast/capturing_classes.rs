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
pub(crate) mod decline;
mod extends;

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Stmt};

mod install;
mod own_binding;
mod this_sites;
use decline::decline_reason;
pub(crate) use decline::{EXPR_HERITAGE_REASON, unclaimed_class_message};
use install::{define_member, descriptor_fields};
pub(super) use this_sites::{expr_says_this, this_sites};

/// Is this a CLASS BINDING this lane mints — the container-facing
/// `__cc<N>_<user name>` or the class-scope `__cci<N>_<user name>`
/// §14.2.3 gives it a second one of (see `own_binding`)?
///
/// The lane's other minted spellings all put a letter where these put
/// a digit (`__cca<N>_` aliases, `__ccm_<n>__` member sentinels,
/// `__ccmk_<C>_<n>` computed keys, `__ccp<N>` heritage bindings), so
/// the digit is what tells them apart — the inner one wears its `i`
/// ahead of the digits for the same reason.
///
/// Asked outside the AST passes by the top-level data-global gate:
/// a desugar-minted `__`-prefixed name stays a main-local there, but
/// this one is a USER class name in disguise — α-renamed, holding the
/// ES5 constructor function — and a named function reading the class
/// it was declared next to has to find it. Without the carve-out
/// `function g() { return K; }` answered `unknown ident __cc0_K`, and
/// the `typeof` spelling of the same read answered `"undefined"`
/// (§13.5.3's answer for an unresolvable reference).
pub(crate) fn is_es5_class_binding(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("__cci")
        .or_else(|| name.strip_prefix("__cc"))
    else {
        return false;
    };
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    digits > 0 && rest[digits..].starts_with('_')
}

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
    name_counts: &std::collections::HashMap<String, u32>,
) -> bool {
    let name_unique = match &stmts[idx] {
        Stmt::ClassDecl { name, .. } => name_counts.get(name).copied().unwrap_or(0) <= 1,
        _ => false,
    };
    if let Some(why) = decline_reason(ast, &stmts[idx], name_unique) {
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
    // §14.2.3 gives a class declaration TWO bindings, and they can
    // only be told apart when the body reads its own name. When it
    // does, the body gets one of its own — immutable, holding the
    // class — and `new` above stays the container's, mutable. Asked
    // on the SOURCE name, before the rename below rewrites it.
    let inner = own_binding::is_read_inside_class(ast, &stmts[idx], &old)
        .then(|| format!("__cci{n}_{old}"));
    // Every class this lane claims is a faithful `extends` target for
    // a later sibling (blade 5; 405-01 opened the static-carrying
    // half — `Object.setPrototypeOf(D, P)` links the class side now
    // that a function value carries a user [[Prototype]] chain).
    // Recorded under the minted name — the rename below writes that
    // same spelling into every sibling's `parent` field.
    ast.es5_parent_classes.insert(new.clone());
    // Where this class's `super(…)` lands, plus the real-parent
    // twin request (405-01) — see `extends::record_claim_tables`.
    extends::record_claim_tables(ast, &stmts[idx], &new, inner.as_deref().unwrap_or(&new));
    super::hoist_nested_classes_rename::rename_in_stmts(ast, stmts, &old, &new);
    if let Some(i) = &inner {
        super::hoist_nested_classes_rename::rename_in_class_bodies(ast, &mut stmts[idx], &new, i);
    }
    alias::mint_unique_aliases(ast, stmts, &new, counter);
    let taken = std::mem::replace(&mut stmts[idx], Stmt::Multi(Vec::new()));
    stmts[idx] = lower_to_es5(ast, taken, &old, inner);
    true
}

/// The declaration index a `__ccm_<n>__` sentinel carries. The parser
/// mints `n` off a program-wide counter, so within one class it sorts
/// the computed members back into element order — which is the order
/// §15.7.14 evaluates their keys in.
fn sentinel_index(member_name: &str) -> Option<usize> {
    member_name
        .strip_prefix("__ccm_")?
        .trim_end_matches('_')
        .parse()
        .ok()
}

/// Where the evaluated key of computed member `n` of class `cname`
/// parks. The name is the parser's, not this module's: a computed
/// INSTANCE field is already rewritten to `(this as any)[__ccmk_<C>_<n>]
/// = <init>` in the constructor prefix, and that reference has to find
/// something.
fn key_binding(cname: &str, n: usize) -> String {
    format!("__ccmk_{cname}_{n}")
}

/// The computed members THIS class declared, by sentinel index, in
/// element order.
///
/// `class_computed_keys` is keyed by class NAME, and a name is not
/// unique — three functions each declaring `class K { [k]() {…} }`
/// share one entry set, so asking the table by name answers with all
/// three. Ownership is read back off the class itself instead: a
/// computed method keeps its `__ccm_<n>__` sentinel as the member
/// name, and a computed instance field is by now a
/// `(this as any)[__ccmk_<C>_<n>]` write in the constructor prefix.
pub(super) fn own_computed_members(
    ast: &Ast,
    cname: &str,
    ctor_body: &[Stmt],
    members: &[&str],
) -> Vec<usize> {
    let mut ns: Vec<usize> = members.iter().filter_map(|n| sentinel_index(n)).collect();
    let prefix = format!("__ccmk_{cname}_");
    collect_key_refs(ast, ctor_body, &prefix, &mut ns);
    ns.sort_unstable();
    ns.dedup();
    ns
}

/// Every `<prefix><n>` identifier anywhere in `body`.
fn collect_key_refs(ast: &Ast, body: &[Stmt], prefix: &str, out: &mut Vec<usize>) {
    let mut pending: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = pending.pop() {
        for e in stmt_exprs(s) {
            collect_key_refs_in_expr(ast, e, prefix, out);
        }
        pending.extend(stmt_children_ref(s));
    }
}

fn collect_key_refs_in_expr(ast: &Ast, root: ExprId, prefix: &str, out: &mut Vec<usize>) {
    let mut pending = vec![root];
    while let Some(eid) = pending.pop() {
        match ast.get_expr(eid) {
            Expr::Ident(n) => {
                if let Some(n) = n.strip_prefix(prefix).and_then(|r| r.parse::<usize>().ok()) {
                    out.push(n);
                }
            }
            // An arrow's body is a statement list, not a child expr.
            Expr::ArrowFn { body, .. } => collect_key_refs(ast, body, prefix, out),
            _ => {}
        }
        pending.extend(expr_children(ast, eid));
    }
}

/// The key expression of each member in `ns`, dropping any the parser
/// did not record (there is none today; a missing one would mean the
/// sentinel outlived its side-table entry, and emitting nothing for it
/// keeps that loud downstream rather than inventing a key).
pub(super) fn keys_of(ast: &Ast, cname: &str, ns: &[usize]) -> Vec<(usize, ExprId)> {
    ns.iter()
        .filter_map(|n| {
            let key = ast
                .class_computed_keys
                .get(&(cname.to_string(), format!("__ccm_{n}__")))?;
            Some((*n, *key))
        })
        .collect()
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
/// `inner` is the class-scope binding when the body reads its own
/// name (§14.2.3). Everything emitted here is the class object
/// itself, so it all stands on that one; the container's mutable
/// binding is minted last, as an alias. Without it there is a single
/// binding and it is the container's.
fn lower_to_es5(ast: &mut Ast, class: Stmt, src_name: &str, inner: Option<String>) -> Stmt {
    let Stmt::ClassDecl {
        name,
        parent,
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
    install::drop_static_this_sites(ast, src_name, &static_methods, &static_init);
    // The pair, or the single binding standing in for both.
    let (name, outer) = match inner {
        Some(i) => (i, Some(name)),
        None => (name, None),
    };
    // §15.7.14 evaluates every ComputedPropertyName once, in element
    // order, at class-definition time — ahead of anything a method or
    // an initializer does, because those run later (on call, on
    // construction). So all the keys come first, and what the class
    // body says about them is a read of the binding.
    let ctor_body: &[Stmt] = ctor.as_ref().map_or(&[], |c| c.body.as_slice());
    let member_names: Vec<&str> = methods
        .iter()
        .chain(static_methods.iter())
        .filter_map(|m| m.name.as_str())
        .collect();
    // Computed STATIC fields (406-02) — their sentinels come from the
    // side table by class name; `decline_reason` admits them only for
    // a program-unique name, so the rows are provably this class's.
    let static_cf: Vec<(usize, ExprId)> = ast
        .class_computed_static_fields
        .iter()
        .filter(|(c, _, _)| c == src_name)
        .filter_map(|(_, sent, init)| sentinel_index(sent).map(|n| (n, *init)))
        .collect();
    let mut own = own_computed_members(ast, src_name, ctor_body, &member_names);
    own.extend(static_cf.iter().map(|(n, _)| *n));
    own.sort_unstable();
    own.dedup();
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
    let (params, body) = match ctor {
        Some(c) => (c.params, c.body),
        None => match &parent {
            Some(p) => extends::implicit_derived_ctor(ast, p, &name),
            None => (Vec::new(), Vec::new()),
        },
    };
    if let Some(p) = &parent {
        extends::rewrite_super_sites(ast, &body, p, false, &name);
    }
    let ctor_eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type: None,
        body,
    });
    ast.fn_expr_exprs.insert(ctor_eid);
    out.push(Stmt::LetDecl {
        mutable: outer.is_none(),
        name: name.clone(),
        type_ann: Some("any".to_string()),
        init: ctor_eid,
        is_var: false,
    });
    // The prototype link goes in BEFORE any member lands: members
    // install on the linked prototype, not the one the function was
    // born with.
    if let Some(p) = &parent {
        extends::proto_chain_stmts(ast, &name, p, &mut out);
    }
    for (m, on_prototype) in methods
        .into_iter()
        .map(|m| (m, true))
        .chain(static_methods.into_iter().map(|m| (m, false)))
    {
        // `super.m(…)` in an instance body reads through the parent's
        // prototype; a static body's home object is the class itself,
        // so its super base is the parent CLASS (405-01 face 3).
        if let Some(p) = &parent {
            extends::rewrite_super_sites(ast, &m.body, p, !on_prototype, &name);
        }
        let eid = ast.add_expr(Expr::ArrowFn {
            params: m.params,
            return_type: m.return_type,
            body: m.body,
        });
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
    if let Some(o) = outer {
        out.push(own_binding::outer_alias_stmt(ast, o, &name));
    }
    Stmt::Multi(out)
}
