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
//! correctly can be pulled in. Constructor, plain public instance
//! methods, and plain public static methods named outright; a static
//! body may say `this`, instance members may carry a computed name,
//! and an instance accessor may be named outright. No `extends`, no
//! static fields or static blocks, no static or computed-name
//! accessor, no type params, and no compiler-minted free name in a
//! body (`__cm_gen_*` forwarders to a hoisted generator method,
//! `__supercall__*`). Everything else keeps today's loud abort.
//!
//! Recorded deviations are in the RFC: the binding is `const`, `.name`
//! is empty, a declare-only field does not materialize, and the class
//! name is no longer a type name.

mod decline;

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Stmt};
use decline::decline_reason;

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
    super::hoist_nested_classes_rename::rename_in_stmts(ast, stmts, &old, &new);
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
    format!(
        "class `{name}` is declared inside a block or a function body and reads a \
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
        ctor,
        methods,
        static_methods,
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
    let (params, body) = match ctor {
        Some(c) => (c.params, c.body),
        None => (Vec::new(), Vec::new()),
    };
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
    for (m, on_prototype) in methods
        .into_iter()
        .map(|m| (m, true))
        .chain(static_methods.into_iter().map(|m| (m, false)))
    {
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
        if let Some(kind) = m.accessor_kind {
            out.push(Stmt::Expr(define_accessor(ast, recv, &m.name, kind, eid)));
            continue;
        }
        let target = match sentinel_index(&m.name) {
            Some(n) => {
                let recv_any = ast.add_expr(Expr::As {
                    expr: recv,
                    ty_ann: "any".to_string(),
                });
                let key_ref = ast.add_expr(Expr::Ident(key_binding(src_name, n)));
                ast.add_expr(Expr::Index {
                    obj: recv_any,
                    index: key_ref,
                })
            }
            None => ast.add_expr(Expr::Member {
                obj: recv,
                name: m.name.clone(),
            }),
        };
        let assign = ast.add_expr(Expr::Assign { target, value: eid });
        out.push(Stmt::Expr(assign));
    }
    Stmt::Multi(out)
}

/// `Object.defineProperty(<recv>, "<name>", { get|set: <fn>,
/// configurable: true } as any)` — how §15.7.14 defines an accessor
/// member, down to the attributes: an accessor on a class is
/// configurable and NOT enumerable, which is exactly what a fresh
/// `defineProperty` gives it.
///
/// A getter and a setter of the same name arrive as two members and so
/// emit two calls. That is the spec's own shape (each MethodDefinition
/// is its own DefinePropertyOrThrow), and the second call keeps the
/// first half: a descriptor naming only `[[Set]]` leaves an existing
/// `[[Get]]` alone (§10.1.6.3 step 4).
fn define_accessor(
    ast: &mut Ast,
    recv: ExprId,
    name: &str,
    kind: super::AccessorKind,
    func: ExprId,
) -> ExprId {
    let half = match kind {
        super::AccessorKind::Getter => "get",
        super::AccessorKind::Setter => "set",
    };
    let yes = ast.add_expr(Expr::Bool(true));
    // The descriptor stays a BARE object literal. Wrapping it in
    // `as any` reads fine and even runs when hand-written, but the
    // fnexpr-this face walk requires an inline `ObjectLit` at exactly
    // this argument — an `As` in between hands it zero faces, and the
    // getter's `this` stays a capture nobody binds.
    let desc = ast.add_expr(Expr::ObjectLit {
        fields: vec![(half.to_string(), func), ("configurable".to_string(), yes)],
    });
    let key = ast.add_expr(Expr::String(name.to_string()));
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
