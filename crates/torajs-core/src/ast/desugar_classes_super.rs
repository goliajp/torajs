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
//! are `ast` module-private fns imported via `use super::*;`
//! (child module sees parent's private items per Rust visibility).

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
