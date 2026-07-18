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
    Vec<String>,           // type_params
    Option<String>,        // parent
    Vec<(String, String)>, // fields
    Vec<StaticInit>,       // static_init
    Option<ClassCtor>,     // ctor
    Vec<ClassMethod>,      // methods
    Vec<ClassMethod>,      // static_methods
);

/// RFC 20260718-error-message-own-prop 刀 3 — §9.2.2 [[Construct]]
/// this-TDZ: a derived class whose USER ctor never calls `super()`
/// must throw a ReferenceError when its implicit `return this` is
/// reached (test262 `NativeError/*-super.js`). Statically decidable
/// here: the deep super-site walk finding zero sites in the whole
/// body means no path can initialize `this`, so the desugar appends
/// the `__torajs_ctor_no_super_throw()` raiser at the body tail —
/// side effects before it still run, matching the runtime order.
/// (A ctor with a CONDITIONAL super() has ≥1 site and is left
/// untouched — the true runtime-flag TDZ is a recorded boundary.)
/// Runs on the mutable class_index BEFORE it freezes; the synthetic
/// derived default ctors just synthesized carry a super() and skip.
pub(super) fn append_no_super_throw(ast: &mut Ast, class_index: &mut [ClassIndexEntry]) {
    for (_, cname, _tp, parent, _, _, ctor, _, _) in class_index.iter_mut() {
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
        let callee = ast.add_expr(Expr::Ident("__torajs_ctor_no_super_throw".to_string()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: Vec::new(),
        });
        c.body.push(Stmt::Expr(call));
    }
}

pub(super) fn rewrite_super_ctor_calls(ast: &mut Ast, class_index: &[ClassIndexEntry]) {
    // Pass 1.5 — rewrite `super(args)` inside each subclass's ctor body
    // into a Call to `__cm_<Parent>__ctor(__this, args)`. Must run before
    // pass 2 (which rewrites `Expr::This` and method-call shapes).
    for (_, cname, _tp, parent, _, _, ctor, _, _) in class_index {
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
            ast.exprs[eid.0 as usize] = Expr::Call {
                callee,
                args: new_args,
            };
        }
    }
}

pub(super) fn rewrite_super_method_calls(ast: &mut Ast, class_index: &[ClassIndexEntry]) {
    // V3-18 wedge — Pass 1.6: rewrite `super.<m>(args)` (encoded
    // as a Call to ident `__supercall__<m>`) inside each subclass's
    // method bodies into `__cm_<Parent>__<m>(__this, args)`. Walks
    // every method body of every class with an `extends` clause.
    for (_, cname, _tp, parent, _, _, ctor, methods, static_methods) in class_index {
        let Some(parent_name) = parent.as_ref() else {
            continue;
        };
        let mut sites: Vec<(ExprId, String, Vec<ExprId>)> = Vec::new();
        if let Some(c) = ctor.as_ref() {
            for s in &c.body {
                collect_supercall_in_stmt(ast, s, &mut sites);
            }
        }
        for m in methods.iter().chain(static_methods.iter()) {
            for s in &m.body {
                collect_supercall_in_stmt(ast, s, &mut sites);
            }
        }
        for (eid, m_name, args) in sites {
            let _ = cname; // diag context only
            let callee = ast.add_expr(Expr::Ident(format!("__cm_{parent_name}__{m_name}")));
            let this_id = ast.add_expr(Expr::This);
            let mut new_args = Vec::with_capacity(args.len() + 1);
            new_args.push(this_id);
            new_args.extend(args);
            ast.exprs[eid.0 as usize] = Expr::Call {
                callee,
                args: new_args,
            };
        }
    }
}
