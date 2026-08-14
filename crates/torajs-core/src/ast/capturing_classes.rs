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
mod decline;
mod extends;

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Stmt};

mod install;
use decline::decline_reason;
use install::{define_member, descriptor_fields};

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
    let new = format!("__cc{}_{}", *counter, old);
    *counter += 1;
    // Every class this lane claims is a faithful `extends` target for
    // a later sibling (blade 5; 405-01 opened the static-carrying
    // half — `Object.setPrototypeOf(D, P)` links the class side now
    // that a function value carries a user [[Prototype]] chain).
    // Recorded under the minted name — the rename below writes that
    // same spelling into every sibling's `parent` field.
    ast.es5_parent_classes.insert(new.clone());
    // Where this class's `super(…)` lands — see the field doc: a
    // ctor-less class forwards, and skipping it transitively keeps
    // every hop off the rest-param promotion bar. The parent's own
    // target is already resolved (siblings lower top-down), so one
    // lookup composes the chain.
    if let Stmt::ClassDecl { ctor, parent, .. } = &stmts[idx] {
        let target = match (ctor, parent) {
            (None, Some(p)) => ast
                .es5_ctor_forward
                .get(p)
                .cloned()
                .unwrap_or_else(|| p.clone()),
            _ => new.clone(),
        };
        ast.es5_ctor_forward.insert(new.clone(), target);
    }
    super::hoist_nested_classes_rename::rename_in_stmts(ast, stmts, &old, &new);
    alias::mint_unique_aliases(ast, stmts, &new, counter);
    let taken = std::mem::replace(&mut stmts[idx], Stmt::Multi(Vec::new()));
    stmts[idx] = lower_to_es5(ast, taken, &old);
    true
}

/// What to say about a `ClassDecl` that reached the checker.
///
/// Exactly one shape gets there, and it is ordinary code: a class
/// nested inside a block or a function body that reads a binding from
/// the scope around it. Such a class cannot be lifted to the top level
/// (nothing up there resolves what it reads), and this lane covers
/// only part of the class surface. Calling that "internal" and asking
/// "desugar didn't run?" reads as a compiler bug report to someone who
/// wrote perfectly good TypeScript; name what is actually missing.
///
/// The reason comes from the side table the hoist filled, not from
/// re-deciding here: the tree has moved since. A static method's
/// `this` is gone from the body it was turned down for by the time
/// this runs, so re-deciding answered a DIFFERENT reason — or none at
/// all, which printed the "the class desugar did not claim it"
/// fallback at code that was turned down for a nameable reason.
pub(crate) fn unclaimed_class_message(ast: &Ast, s: &Stmt) -> String {
    let name = match s {
        Stmt::ClassDecl { name, .. } => name.as_str(),
        _ => "?",
    };
    let why = ast
        .unclaimed_class_reasons
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, why)| *why)
        .or_else(|| decline_reason(ast, s))
        .unwrap_or("the class desugar did not claim it");
    // A `__ClassExpr_<id>` here is a class EXPRESSION the parser
    // spliced next to its use site (393-01) — that spelling means
    // nothing to whoever wrote it, but the binding name might.
    let shown = if name.starts_with("__ClassExpr_") {
        match ast.class_expr_display_names.get(name) {
            Some(d) => format!("the class expression bound to `{d}`"),
            None => "an anonymous class expression".to_string(),
        }
    } else {
        format!("class `{name}`")
    };
    format!(
        "{shown} is declared inside a block or a function body and reads a \
         binding from around it, which is not supported yet because {why}"
    )
}

/// Every `this` node in `body`. A nested `function` expression binds
/// its own, so descending into one over-answers — the safe direction
/// for both callers: a registration cleared one time too many only
/// sends that `this` down the same channel a function expression would
/// have used anyway, and the hoist's remap (what makes this
/// `pub(super)`) moves only sites still registered under the name it is
/// renaming. It does not descend into a nested class body, so a class
/// inside one of these keeps its own registrations either way.
pub(super) fn this_sites(ast: &Ast, body: &[Stmt]) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut pending: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = pending.pop() {
        for e in stmt_exprs(s) {
            this_sites_in_expr(ast, e, &mut out);
        }
        pending.extend(stmt_children_ref(s));
    }
    out
}

/// Does this expression say `this` anywhere under it — arrow bodies
/// included? Asked of a static field's initializer, which runs at
/// class-evaluation time with `this` bound to the class object: a
/// `this`-free one has nothing to lose by being inlined at the store,
/// while one that reads `this` must wrap into the
/// `(function () { … }).call(K)` form the emit mints (394-05) — bare
/// at the store it would silently pick up the ENCLOSING receiver.
pub(super) fn expr_says_this(ast: &Ast, root: ExprId) -> bool {
    let mut sites = Vec::new();
    this_sites_in_expr(ast, root, &mut sites);
    !sites.is_empty()
}

fn this_sites_in_expr(ast: &Ast, root: ExprId, out: &mut Vec<ExprId>) {
    let mut pending = vec![root];
    while let Some(eid) = pending.pop() {
        match ast.get_expr(eid) {
            Expr::This => out.push(eid),
            // An arrow's body is a statement list, not a child expr.
            Expr::ArrowFn { body, .. } => out.extend(this_sites(ast, body)),
            _ => {}
        }
        pending.extend(expr_children(ast, eid));
    }
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
fn lower_to_es5(ast: &mut Ast, class: Stmt, src_name: &str) -> Stmt {
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
    // The parser recorded, at the token, that every `this` in a static
    // member body means the class object, and `desugar_classes` pass 2
    // turns each recorded site into the class NAME. That mint is wrong
    // twice over here: the name has been α-renamed away, and what the
    // renamed binding holds is a function value rather than a class.
    // Drop the registration and those reads become ordinary function
    // `this` — which is what `K.s = function () { … }` invoked as
    // `K.s()` delivers anyway, and it is the same object §10.2.1.2
    // asked for.
    for m in &static_methods {
        for eid in this_sites(ast, &m.body) {
            ast.static_this_sites.remove(&eid);
        }
    }
    // §15.7.14 evaluates every ComputedPropertyName once, in element
    // order, at class-definition time — ahead of anything a method or
    // an initializer does, because those run later (on call, on
    // construction). So all the keys come first, and what the class
    // body says about them is a read of the binding.
    let ctor_body: &[Stmt] = ctor.as_ref().map_or(&[], |c| c.body.as_slice());
    let member_names: Vec<&str> = methods
        .iter()
        .chain(static_methods.iter())
        .map(|m| m.name.as_str())
        .collect();
    let own = own_computed_members(ast, src_name, ctor_body, &member_names);
    let mut out: Vec<Stmt> = Vec::new();
    for (n, key) in keys_of(ast, src_name, &own) {
        let key_any = ast.add_expr(Expr::As {
            expr: key,
            ty_ann: "any".to_string(),
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
            Some(p) => extends::implicit_derived_ctor(ast, p),
            None => (Vec::new(), Vec::new()),
        },
    };
    if let Some(p) = &parent {
        extends::rewrite_super_sites(ast, &body, p, false);
    }
    let ctor_eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type: None,
        body,
    });
    ast.fn_expr_exprs.insert(ctor_eid);
    out.push(Stmt::LetDecl {
        mutable: false,
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
            extends::rewrite_super_sites(ast, &m.body, p, !on_prototype);
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
        let key = match sentinel_index(&m.name) {
            Some(n) => ast.add_expr(Expr::Ident(key_binding(src_name, n))),
            None => ast.add_expr(Expr::String(m.name.clone())),
        };
        let fields = descriptor_fields(ast, m.accessor_kind, eid);
        out.push(Stmt::Expr(define_member(ast, recv, key, fields)));
    }
    install::install_static_inits(ast, static_init, &name, parent.as_deref(), &mut out);
    Stmt::Multi(out)
}
