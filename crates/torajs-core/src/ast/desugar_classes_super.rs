//! `desugar_classes` super-call rewrite passes (chunk 176, 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs`:
//!   * `rewrite_super_ctor_calls` — pre-extract was "Pass 1.5" (rewrite
//!     `super(args)` inside each subclass's ctor body into a Call to
//!     `__cm_<Parent>__ctor(__this, __new_target, args)`).
//!   * `rewrite_super_method_calls` — pre-extract was "Pass 1.6"
//!     (rewrite `super.<m>(args)` — encoded as Call to ident
//!     `__supercall__<m>` — into `__cm_<Parent>__<m>(__this, args)`).
//!
//! Both passes walk `class_index` (built by Pass 1) and mutate the
//! `ast.exprs` arena in place. They must run **before Pass 2** (which
//! rewrites `Expr::This` and method-call shapes).
//!
//! Body verbatim from pre-extract. The `class_index` 9-tuple is passed
//! by reference; we declare a `ClassIndexEntry` type alias here so the
//! sub-fn signatures stay readable (the main fn keeps the inline shape
//! for now). `collect_super_in_stmt` / `collect_supercall_in_stmt`
//! now live in the `super_collect` sibling (chunk 350) and are
//! reached via an explicit `use` path since `pub(super)` items in
//! sibling modules don't leak through `use super::*;`.

use super::super_collect::{collect_super_in_stmt, collect_supercall_in_stmt};
use super::*;

pub(super) type ClassIndexEntry = (
    usize,
    String,
    Vec<String>,            // type_params
    Option<String>,         // parent
    Vec<(PropKey, String)>, // fields
    Vec<StaticInit>,        // static_init
    Option<ClassCtor>,      // ctor
    Vec<ClassMethod>,       // methods
    Vec<ClassMethod>,       // static_methods
    u32,                    // decl_id (RFC 20260921) — the name is not unique
);

/// RFC 20260718-error-message-own-prop 刀 3 — §9.2.2 [[Construct]]
/// this-TDZ: a derived class whose USER ctor never calls `super()`
/// must throw a ReferenceError when its implicit `return this` is
/// reached (test262 `NativeError/*-super.js`). Statically decidable
/// here: the deep super-site walk finding zero sites in the whole
/// body means no path can initialize `this`, so the desugar appends
/// the `__torajs_this_binding_throw(false)` raiser at the body tail —
/// side effects before it still run, matching the runtime order.
/// (A ctor with a CONDITIONAL super() has ≥1 site; the runtime flag
/// in `desugar_classes_this_tdz` covers it.)
/// Runs on the mutable class_index BEFORE it freezes; the synthetic
/// derived default ctors just synthesized carry a super() and skip.
pub(super) fn append_no_super_throw(ast: &mut Ast, class_index: &mut [ClassIndexEntry]) {
    for (_, cname, _tp, parent, _, _, ctor, _, _, _) in class_index.iter_mut() {
        if parent.is_none() {
            continue;
        }
        // Only a ctor the USER spelled out counts — a parser-synth
        // field-init ctor (`class D extends A { f = 1 }`) has no
        // super() either, but per §15.7.14 the class simply has the
        // implicit default ctor which DOES call super.
        if !ast.explicit_ctor_classes.contains(cname) {
            continue;
        }
        let Some(c) = ctor.as_mut() else { continue };
        let mut sites: Vec<(ExprId, Vec<ExprId>)> = Vec::new();
        for s in &c.body {
            collect_super_in_stmt(ast, s, &mut sites);
        }
        if !sites.is_empty() {
            continue;
        }
        let call = super::desugar_classes_this_tdz::raise_call(ast, false);
        c.body.push(Stmt::Expr(call));
    }
}

pub(super) fn rewrite_super_ctor_calls(ast: &mut Ast, class_index: &[ClassIndexEntry]) {
    // Pass 1.5 — rewrite `super(args)` inside each subclass's ctor body
    // into a Call to `__cm_<Parent>__ctor(__this, args)`. Must run before
    // pass 2 (which rewrites `Expr::This` and method-call shapes).
    for (_, cname, _tp, parent, fields, _, ctor, methods, _, _) in class_index {
        let Some(c) = ctor.as_ref() else { continue };
        let mut super_sites: Vec<(ExprId, Vec<ExprId>)> = Vec::new();
        for s in &c.body {
            collect_super_in_stmt(ast, s, &mut super_sites);
        }
        for (eid, args) in super_sites {
            let parent_name = parent.as_ref().unwrap_or_else(|| {
                panic!(
                    "M5.2: `super(...)` used in `{cname}.constructor` but `{cname}` \
                     has no `extends` clause"
                )
            });
            let callee = ast.add_expr(Expr::Ident(format!("__cm_{parent_name}__ctor")));
            let this_id = ast.add_expr(Expr::This);
            // P4.5 — `super()` forwards the current ctor's
            // __new_target through to the parent ctor so chain
            // ancestors see the actual class function that was
            // invoked via `new`, not the static ctor owner.
            let new_target_id = ast.add_expr(Expr::Ident("__new_target".into()));
            let mut new_args = Vec::with_capacity(args.len() + 2);
            new_args.push(this_id);
            new_args.push(new_target_id);
            new_args.extend(args);
            let call = Expr::Call {
                callee,
                args: new_args,
            };
            // RFC 20260820-ctor-return-override blade 3 — on a chain
            // that touches a value-returning ctor, what the parent
            // answers may BE the instance from here on (§10.2.2 step
            // 13), so the site becomes an assignment to `this` rather
            // than a bare call, and this class's own elements follow
            // the object that won. The parent's answer lands in the
            // `__sup` slot first — see `reshape_ctor` for why a bare
            // call result cannot be handed to the pick directly.
            //
            // Only the OWN fields are carried, and from `__this_in`
            // (the parameter, which goes on naming what the factory
            // minted) rather than from `this`, which has just been
            // reassigned. An ancestor's fields are deliberately left
            // behind: per §7.3.28 they were installed on the `this`
            // that ancestor's own constructor walked away from.
            //
            // Emitting `Expr::This` for the receiver — not a bare
            // ident — is what puts the assignment on the same rewrite
            // channel as the rest of the body: Pass 2 turns every one
            // of them into `Ident("__this")`, the local `reshape_ctor`
            // introduces.
            if !ast.ctor_return_override.contains(cname) {
                // §10.2.1.3 step 9 — the class's private methods are
                // installed as soon as `super(…)` has bound `this`.
                ast.exprs[eid.0 as usize] =
                    match super::private_brand::install_call(ast, cname, Expr::This) {
                        Some(install) => Expr::Sequence {
                            left: ast.add_expr(call),
                            right: install,
                        },
                        None => call,
                    };
                continue;
            }
            let call_id = ast.add_expr(call);
            let sup_slot = ast.add_expr(Expr::Ident("__sup".into()));
            let mut seq = ast.add_expr(Expr::Assign {
                target: sup_slot,
                value: call_id,
            });
            let sup_read = ast.add_expr(Expr::Ident("__sup".into()));
            let incumbent = ast.add_expr(Expr::This);
            let parent_derived = super::desugar_classes_ctor_return::is_derived(ast, parent_name);
            let picked = super::desugar_classes_ctor_return::pick_call(
                ast,
                incumbent,
                sup_read,
                parent_derived,
            );
            let target = ast.add_expr(Expr::This);
            let adopt = ast.add_expr(Expr::Assign {
                target,
                value: picked,
            });
            seq = ast.add_expr(Expr::Sequence {
                left: seq,
                right: adopt,
            });
            // Slice 2c — the private method brand is an element of its
            // own (§7.3.34 step 2), added before the fields.
            let brand = methods
                .iter()
                .any(|m| m.name.starts_with("__priv_"))
                .then(|| PropKey::from(format!("__privbrand_{cname}")));
            for fname in brand.iter().chain(fields.iter().map(|(f, _)| f)) {
                let this_now = ast.add_expr(Expr::This);
                let carry = super::desugar_classes_ctor_return::carry_call(ast, this_now, fname);
                seq = ast.add_expr(Expr::Sequence {
                    left: seq,
                    right: carry,
                });
            }
            let value = ast.add_expr(Expr::This);
            ast.exprs[eid.0 as usize] = Expr::Sequence {
                left: seq,
                right: value,
            };
        }
    }
}

/// Answers the super base spelling this pass used, so Pass 1.7 can
/// use the same one: `true` = the runtime `[[HomeObject]].[[Prototype]]`
/// read, `false` = the static parent (see `super_home_dynamic`). The
/// two passes must not mix spellings within one program, so the
/// judgment is made once, here, before either of them mutates the
/// arena.
pub(super) fn rewrite_super_method_calls(ast: &mut Ast, class_index: &[ClassIndexEntry]) -> bool {
    let dynamic = super::super_home_dynamic::program_mutates_prototypes(ast);
    // V3-18 wedge — Pass 1.6: rewrite `super.<m>(args)` (encoded
    // as a Call to ident `__supercall__<m>`) inside each subclass's
    // method bodies into `__cm_<Parent>__<m>(__this, args)`. Walks
    // every method body of every class with an `extends` clause.
    //
    // Static-method bodies rewrite separately: their super base is the
    // parent CLASS OBJECT, so the target is `__sm_<owner>__<m>(args)`
    // — no receiver param (statics don't bind one), and the owner walk
    // reads the static-method lists. Before the split every static
    // site was named `__cm_...` with a minted `this` and died loud on
    // an unknown identifier.
    for (_, cname, _tp, parent, _, static_init, ctor, methods, static_methods, _) in class_index {
        // Rotation 371 — a builtin-heritage class was STRIPPED
        // (desugar_classes_builtin_heritage sets its parent to None
        // and records the builtin in `exotic_parent`); its
        // `super.m()` sites route to the runtime super-builtin
        // re-dispatch below. Object / Iterator (non-exotic stripped
        // parents) keep the recorded loud boundary.
        let parent_name = match parent.as_ref() {
            Some(p) => p.clone(),
            None => match ast.exotic_parent.get(cname) {
                Some(p) => p.clone(),
                None => continue,
            },
        };
        let parent_name = &parent_name;
        let mut sites: Vec<(ExprId, String, Vec<ExprId>)> = Vec::new();
        if let Some(c) = ctor.as_ref() {
            for s in &c.body {
                collect_supercall_in_stmt(ast, s, &mut sites);
            }
        }
        for m in methods {
            for s in &m.body {
                collect_supercall_in_stmt(ast, s, &mut sites);
            }
        }
        for (eid, m_name, args) in sites {
            // S2.12 — `super.m()` names the nearest ancestor that
            // declares `m`, not necessarily the direct parent. Walking
            // up from the parent is what `class C extends B extends A`
            // needs when `m` lives on A; before this the rewrite named
            // `__cm_B__m` and the program died on an unknown identifier.
            // Falling back to the direct parent when nothing declares it
            // keeps the pre-existing diagnostic for a genuine typo.
            let owner = nearest_declaring(class_index, parent_name, &m_name, false);
            // 507-03 — the direct `__cm_` call names the ancestor the
            // chain had at class-definition time; when the program can
            // re-link a prototype that is no longer the answer, so the
            // site takes the same receiver-aware get-then-call kernel
            // the computed form (`super[k](args)`) already uses, which
            // reads the base when the site runs. The builtin re-dispatch
            // below is left alone: it does not name a `__cm_` owner that
            // could go stale, and the generic kernel cannot reach a
            // builtin prototype's methods at all. Decided before the
            // callee is minted so no orphan ident reaches the arena.
            let builtin_route =
                owner.is_none() && builtin_heritage_root(ast, class_index, parent_name);
            if dynamic && !builtin_route {
                let site = super::desugar_classes_super_prop_mint::SiteCtx::runtime(cname, false);
                let base = site.base(ast);
                let key = ast.add_expr(Expr::String(m_name.into()));
                let recv = site.receiver(ast);
                let pack = ast.add_expr(Expr::Array(args));
                let kernel = ast.add_expr(Expr::Ident("__torajs_super_prop_call".to_string()));
                ast.exprs[eid.0 as usize] = Expr::Call {
                    callee: kernel,
                    args: vec![base, key, recv, pack],
                };
                continue;
            }
            // Rotation 371 — no user ancestor declares `m` and the
            // heritage chain roots in a builtin (`class C extends
            // Set`): `super.m()` resolves on the BUILTIN prototype,
            // which has no `__cm_` face. Route through the runtime
            // super-builtin re-dispatch (own overrides skipped per
            // §13.3.7.3); a genuine typo still dies there as the
            // spec not-a-function TypeError instead of an unknown
            // identifier at compile time.
            let callee = match &owner {
                Some(o) => ast.add_expr(Expr::Ident(format!("__cm_{o}__{m_name}"))),
                None if builtin_route => {
                    ast.add_expr(Expr::Ident(format!("__superbuiltin__{m_name}")))
                }
                None => ast.add_expr(Expr::Ident(format!("__cm_{parent_name}__{m_name}"))),
            };
            let this_id = ast.add_expr(Expr::This);
            let mut new_args = Vec::with_capacity(args.len() + 1);
            new_args.push(this_id);
            new_args.extend(args);
            ast.exprs[eid.0 as usize] = Expr::Call {
                callee,
                args: new_args,
            };
        }
        super::desugar_classes_super_static::rewrite_static_super_calls(
            ast,
            class_index,
            (cname, parent_name),
            (static_methods.as_slice(), static_init.as_slice()),
            dynamic,
        );
    }
    dynamic
}

/// True when `start`'s heritage chain roots in a name outside the
/// class index — an `extends` of a builtin (Set / Map / Array /
/// Error / …). The checker already refused an `extends` of a name
/// that is neither a class nor a known builtin, so "not in the
/// index" is the builtin verdict here.
fn builtin_heritage_root(ast: &Ast, class_index: &[ClassIndexEntry], start: &str) -> bool {
    let mut cur = start.to_string();
    for _ in 0..64 {
        let Some(entry) = class_index.iter().find(|e| e.1 == cur) else {
            return true;
        };
        // A stripped ancestor lost its parent link but keeps the
        // builtin in `exotic_parent` — the chain roots there.
        if ast.exotic_parent.contains_key(&cur) {
            return true;
        }
        match &entry.3 {
            Some(p) => cur = p.clone(),
            None => return false,
        }
    }
    false
}

/// Walk `start` and its ancestors for the first class declaring a
/// method named `m` in the relevant list (instance methods, or the
/// statics when `statics` is set), answering that class's name.
/// `None` when nothing in the chain declares it — the caller then
/// keeps naming the direct parent so the existing "unknown
/// identifier" diagnostic still fires for a genuine typo rather than
/// being replaced by silence.
///
/// The hop bound guards a malformed `extends` cycle; a well-formed
/// hierarchy is a tree and terminates on its own.
pub(super) fn nearest_declaring(
    class_index: &[ClassIndexEntry],
    start: &str,
    m: &str,
    statics: bool,
) -> Option<String> {
    let mut cur = start.to_string();
    for _ in 0..64 {
        let entry = class_index.iter().find(|e| e.1 == cur)?;
        let list = if statics { &entry.8 } else { &entry.7 };
        if list.iter().any(|method| method.name == m) {
            return Some(cur);
        }
        cur = entry.3.clone()?;
    }
    None
}
