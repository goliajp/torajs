//! Nested-class hoist — the `desugar_classes` pre-pass.
//!
//! `desugar_classes` snapshots `Stmt::ClassDecl` from `ast.stmts`
//! only, so a class declared inside any nested statement container
//! (a fn body, an fn-expression body, a block, a switch arm, ...)
//! never desugars and aborts loud in check.rs. This pass moves every
//! CAPTURE-FREE nested ClassDecl to the top level so the existing
//! class machinery lifts it with zero downstream changes — the same
//! strategy the parser already applies to class *expressions*
//! (`parse_primary_class_expr` buffers them to `synth_classes` and
//! splices at top level).
//!
//! Capture-free means: every ctor / method / static body resolves
//! its free identifiers against top-level names (plus the class's
//! own bindings), and the `extends` parent is a top-level or global
//! name. A class reading an enclosing fn's local stays where it is
//! and keeps the loud not-yet-supported abort.
//!
//! Renaming: the hoisted class keeps its source name unless that
//! name collides with a top-level binding or another hoisted class
//! (two fn bodies each declaring `class C`). On collision the class
//! and every reference in its ENCLOSING CONTAINER subtree are
//! α-renamed to `__hc<N>_<name>`.
//!
//! Recorded semantic deviations (same register as the class-expr
//! L3b entries):
//! - class evaluation moves to program top level: static inits /
//!   computed keys evaluate once at startup, not per enclosing-fn
//!   call, and every call shares one class identity;
//! - block-scope visibility widens to the whole program (a name-only
//!   inner shadow of the same identifier inside the container would
//!   be renamed with it).

use super::hoist_nested_classes_admit::{class_evaluation_is_observable, class_is_capture_free};
use super::hoist_nested_classes_walk::walk_child;
use super::*;

pub(super) fn hoist_nested_classes(ast: &mut Ast) {
    // Top-level visible names — the prebound set for the capture
    // check plus the collision domain for renaming.
    let mut top_names: Vec<String> = Vec::new();
    for s in &ast.stmts {
        collect_decl_name(s, &mut top_names);
    }

    // Program-wide ClassDecl name census (406-02). A computed STATIC
    // field leaves no trace on the class — its side-table rows match
    // by NAME — so the capturing lane may install them only when the
    // name provably has one owner. Counted once here, where the whole
    // tree is still in hand; the lane mints fresh fn-exprs, never
    // fresh ClassDecls, so the census cannot go stale mid-walk.
    let mut name_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for s in &ast.stmts {
        super::hoist_nested_classes_census::count_class_decl_names(s, &mut name_counts);
    }
    for e in &ast.exprs {
        if let super::Expr::ArrowFn { body, .. } = e {
            for s in body {
                super::hoist_nested_classes_census::count_class_decl_names(s, &mut name_counts);
            }
        }
    }

    super::hoist_nested_classes_census::admit_top_root_real_classes(ast, &name_counts);

    let mut hoisted: Vec<Stmt> = Vec::new();
    let mut counter: u32 = 0;

    // Stmt tree first. Top level itself never hoists (already there);
    // only nested containers are scanned.
    let mut stmts = std::mem::take(&mut ast.stmts);

    // RFC 20260815 knife 2a — a TOP-LEVEL class whose heritage was
    // extracted to a `__ccp<N>` value binding cannot take the static
    // lane (it keys on a class NAME), and the container walk below
    // only scans nested lists. Route it through the capturing lane
    // here; whatever the lane declines stays and stays loud.
    for idx in 0..stmts.len() {
        let value_parent = matches!(&stmts[idx], Stmt::ClassDecl { parent, .. }
            if ast
                .parent_ident_name(*parent)
                .is_some_and(|p| ast.es5_value_parents.contains(p)));
        if value_parent {
            super::capturing_classes::try_rewrite_capturing_class(
                ast,
                &mut stmts,
                idx,
                &mut counter,
                &name_counts,
            );
        }
    }
    for s in &mut stmts {
        walk_child(
            ast,
            s,
            &mut top_names,
            &mut hoisted,
            &mut counter,
            &name_counts,
            false,
        );
    }
    ast.stmts = stmts;

    // Fn-expression / arrow bodies live in the expr arena. Nested
    // arrows are separate arena entries, so this flat scan covers
    // arbitrarily deep nesting. The arena GROWS while it runs: the
    // capturing-class lane turns a class body into function
    // expressions, and those bodies are themselves scan sites — hence
    // a length re-read per step rather than a fixed range.
    let mut i = 0;
    while i < ast.exprs.len() {
        let mut body = match &mut ast.exprs[i] {
            Expr::ArrowFn { body, .. } => std::mem::take(body),
            _ => {
                i += 1;
                continue;
            }
        };
        walk_container(
            ast,
            &mut body,
            &mut top_names,
            &mut hoisted,
            &mut counter,
            &name_counts,
            true,
        );
        if let Expr::ArrowFn { body: b, .. } = &mut ast.exprs[i] {
            *b = body;
        }
        i += 1;
    }

    ast.stmts.extend(hoisted);
}

fn collect_decl_name(s: &Stmt, out: &mut Vec<String>) {
    match s {
        Stmt::FnDecl { name, .. }
        | Stmt::LetDecl { name, .. }
        | Stmt::ClassDecl { name, .. }
        | Stmt::TypeDecl { name, .. } => out.push(name.clone()),
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => collect_decl_name(inner, out),
        _ => {}
    }
}

/// Scan one nested statement list: hoist this level's capture-free
/// ClassDecls first (top-down — a hoisted outer class becomes a
/// top-level name that unlocks an inner `extends` of it), then
/// recurse into each statement's child containers.
pub(super) fn walk_container(
    ast: &mut Ast,
    stmts: &mut Vec<Stmt>,
    top_names: &mut Vec<String>,
    hoisted: &mut Vec<Stmt>,
    counter: &mut u32,
    name_counts: &std::collections::HashMap<String, u32>,
    multi_eval: bool,
) {
    // A `Multi` holding a ClassDecl is the parse_stmt wrapper's
    // use-site splice of a class EXPRESSION (393-01). Flatten it into
    // this list before anything else: `Multi` is a transparent
    // sequence (no scope), and the capturing lane's α-rename runs
    // over THE LIST THAT HOLDS THE CLASS — left inside the Multi, the
    // rename would miss the sibling statements the parser already
    // rewrote to the synth name (`new F()` → `new __ClassExpr_<id>()`).
    let mut i = 0;
    while i < stmts.len() {
        let has_class = matches!(&stmts[i], Stmt::Multi(v)
            if v.iter().any(|s| matches!(s, Stmt::ClassDecl { .. })));
        if has_class {
            let Stmt::Multi(v) = std::mem::replace(&mut stmts[i], Stmt::Multi(Vec::new())) else {
                unreachable!("matched Multi above");
            };
            stmts.splice(i..i + 1, v);
            continue; // re-examine i — the flattened head may nest again
        }
        i += 1;
    }
    // Sibling class names count as resolvable during the capture
    // check: they either hoist together or the straggler fails loud
    // in desugar's parent validation.
    let sibling_start = top_names.len();
    for s in stmts.iter() {
        if let Stmt::ClassDecl { name, .. } = s {
            top_names.push(name.clone());
        }
    }

    for idx in 0..stmts.len() {
        if !matches!(stmts[idx], Stmt::ClassDecl { .. }) {
            continue;
        }
        // 420-01 / 420-02 — §15.7.14 walks a class's elements once, in
        // order, where the class is written: computed member NAMES are
        // evaluated there and static initializers RUN there. Hoisting
        // moves all of that to wherever the hoisted class lands, which
        // is the END of the program. `function f() { class C {
        // [obj](){} } }` therefore called `obj.toString()` once, after
        // the last top-level statement, instead of once per call to
        // `f`, and the same relocation carried a throwing key out of an
        // enclosing `try`. The in-place lane below gets this right
        // already — it emits the key binding and the static installs at
        // the class's own position — so route there when it will take
        // the class. A computed INSTANCE field never needed the route:
        // its ctor prefix reads `__ccmk_<C>_<n>`, a free name that
        // already fails the capture check below.
        let observable = class_evaluation_is_observable(ast, &stmts[idx]);
        if !class_is_capture_free(ast, &stmts[idx], top_names)
            || (observable
                && super::capturing_classes::decline::would_claim(ast, &stmts[idx], name_counts))
        {
            // The other half: a class that DOES read an outer local
            // cannot be lifted, and takes the runtime-value lane
            // instead (RFC 20260814-capturing-nested-class). Whatever
            // that lane declines stays here and stays loud.
            super::capturing_classes::try_rewrite_capturing_class(
                ast,
                stmts,
                idx,
                counter,
                name_counts,
            );
            continue;
        }
        let old_name = match &stmts[idx] {
            Stmt::ClassDecl { name, .. } => name.clone(),
            _ => unreachable!(),
        };
        // RFC 20260917-class-eval-unit K1 — a unit-class candidate whose
        // name another class shares takes the collision rename, so the
        // reference census below counts this class's references only.
        let unit_shape = multi_eval && super::class_unit::admits_shape(ast, &stmts[idx]);
        // Collision domain: everything visible at top level once the
        // hoist lands. Sibling entries pushed above shadow the check
        // for the class's own occurrence, so count duplicates.
        let collides = top_names.iter().filter(|n| **n == old_name).count() > 1
            || top_names[..sibling_start].contains(&old_name)
            || (unit_shape && name_counts.get(&old_name).copied().unwrap_or(0) > 1);
        let mut residual: Option<String> = None;
        if collides {
            let new_name = format!("__hc{}_{}", *counter, old_name);
            *counter += 1;
            // The rename is the hoist's, not the program's: `.name`,
            // the `[class X]` print and the instance's `X { … }` prefix
            // keep the source spelling, through the same table a named
            // class expression's binding uses (`class_display_name`).
            // A class expression that already carries a display name
            // carries it on. Before this `.name` answered `__hc0_D`.
            let display = ast
                .class_expr_display_names
                .get(&old_name)
                .cloned()
                .unwrap_or_else(|| old_name.clone());
            ast.class_expr_display_names
                .insert(new_name.clone(), display);
            // The parser recorded, at the token, that a `this` in a
            // static body means THIS class — and it recorded it as the
            // NAME the source used, which `desugar_classes` pass 2
            // later mints. Renaming without remapping leaves those
            // sites naming a binding that no longer exists, and
            // `typeof` of an unresolvable name answers "undefined"
            // rather than saying so: the class renamed for a collision
            // starts answering wrong QUIETLY. Only the sites still
            // registered under the old name move, so a class nested
            // inside one of these bodies keeps its own.
            if let Stmt::ClassDecl { static_methods, .. } = &stmts[idx] {
                let sites: Vec<ExprId> = static_methods
                    .iter()
                    .flat_map(|m| super::capturing_classes::this_sites(ast, &m.body))
                    .collect();
                for eid in sites {
                    if ast
                        .static_this_sites
                        .get(&eid)
                        .is_some_and(|c| *c == old_name)
                    {
                        ast.static_this_sites.insert(eid, new_name.clone());
                    }
                }
            }
            super::hoist_nested_classes_rename::rename_in_stmts(ast, stmts, &old_name, &new_name);
            top_names.push(new_name.clone());
            residual = Some(new_name);
        } else {
            top_names.push(old_name);
        }
        // §14.2.3 — the scope the class was WRITTEN in holds the
        // outer mutable binding, and for a renamed class that scope
        // is this container, not the top level the class is moving
        // to. So the declaration leaves one behind in its place.
        // Every reference here has just been renamed to match, and
        // `class_globals` reads the init's spelling as the class
        // object it is (a synthesized dunder the reference rewrite
        // does not touch). A class that did NOT collide keeps the
        // top-level binding `class_globals` mints for it, which is
        // the same widening its hoist already records.
        let binding = residual
            .clone()
            .unwrap_or_else(|| top_names.last().cloned().unwrap_or_default());
        if unit_shape && super::class_unit::refs_stay_local(ast, stmts, idx, &binding) {
            let mut cls = std::mem::replace(&mut stmts[idx], Stmt::Multi(Vec::new()));
            let (slot, unit_fn) = super::class_unit::make_unit(ast, &mut cls, &binding);
            stmts[idx] = slot;
            hoisted.push(cls);
            hoisted.push(unit_fn);
            continue;
        }
        let slot = match residual {
            Some(name) => {
                let init = ast.add_expr(Expr::Ident(format!("__class_{name}")));
                Stmt::LetDecl {
                    mutable: true,
                    name,
                    type_ann: Some("any".to_string()),
                    init,
                    is_var: false,
                }
            }
            None => Stmt::Multi(Vec::new()),
        };
        let cls = std::mem::replace(&mut stmts[idx], slot);
        hoisted.push(cls);
    }

    for s in stmts.iter_mut() {
        walk_child(ast, s, top_names, hoisted, counter, name_counts, multi_eval);
    }
}
