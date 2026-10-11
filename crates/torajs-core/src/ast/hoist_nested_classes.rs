//! Nested-class hoist — the `desugar_classes` pre-pass.
//!
//! `desugar_classes` snapshots `Stmt::ClassDecl` from `ast.stmts`
//! only, so a class declared inside any nested statement container
//! (a fn body, an fn-expression body, a block, a switch arm, ...)
//! never desugars and aborts loud in check.rs. This pass moves every
//! CAPTURE-FREE nested ClassDecl to the top level so the existing
//! class machinery lifts it with zero downstream changes. A class
//! *expression* outside a class body, or (K6-1) in a member body's
//! statement, reaches this pass the same way: the parser lands its
//! ClassDecl beside the use site (`buffer_class_expr`); only one in a
//! field initializer or computed key still splices at top level.
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
mod lanes;
use super::hoist_nested_classes_walk::walk_child;
use super::scope_chain::{list_names, list_names_but_classes};
use super::*;
pub(super) use lanes::HoistNames;
use lanes::{in_place_for_statics, rewrite_capturing};
use std::collections::HashMap;

pub(super) fn hoist_nested_classes(ast: &mut Ast) {
    let mut names = HoistNames {
        top: Vec::new(),
        shadow: Vec::new(),
        minted: HashMap::new(),
    };
    for s in &ast.stmts {
        collect_decl_name(s, &mut names.top);
    }

    // Program-wide ClassDecl name census (406-02). A computed STATIC
    // field leaves no trace on the class — its side-table rows match
    // by NAME — so the capturing lane may install them only when the
    // name provably has one owner. Counted once here, where the whole
    // tree is still in hand; the lane mints fresh fn-exprs, never
    // fresh ClassDecls, so the census cannot go stale mid-walk.
    let mut name_counts = super::hoist_nested_classes_census::NameCensus::default();
    for s in &ast.stmts {
        super::hoist_nested_classes_census::count_class_decl_names(s, &mut name_counts.classes);
        super::hoist_nested_classes_census::count_binding_decl_names(s, &mut name_counts.bindings);
    }
    for e in &ast.exprs {
        if let super::Expr::ArrowFn { body, .. } = e {
            for s in body {
                super::hoist_nested_classes_census::count_class_decl_names(
                    s,
                    &mut name_counts.classes,
                );
                super::hoist_nested_classes_census::count_binding_decl_names(
                    s,
                    &mut name_counts.bindings,
                );
            }
        }
    }

    super::hoist_nested_classes_census::admit_top_root_real_classes(ast, &name_counts.classes);

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
            rewrite_capturing(ast, &mut stmts, idx, &mut counter, &mut names);
        }
    }
    for s in &mut stmts {
        walk_child(
            ast,
            s,
            &mut names,
            &mut hoisted,
            &mut counter,
            &name_counts,
            false,
        );
    }
    ast.stmts = stmts;

    // The frames around each arena body, read off the tree as the
    // statement walk left it: a class it hoisted now sits in
    // `hoisted`, at the top level.
    let chains = super::scope_chain::chains_by_arrow(ast, &[&ast.stmts, &hoisted]);

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
        let eid = ExprId(i as u32);
        let around = chains.get(&eid).or_else(|| names.minted.get(&eid));
        names.shadow = super::scope_chain::arrow_frames(ast, around, eid, &body);
        walk_container(
            ast,
            &mut body,
            &mut names,
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
    names: &mut HoistNames,
    hoisted: &mut Vec<Stmt>,
    counter: &mut u32,
    name_counts: &super::hoist_nested_classes_census::NameCensus,
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
    // in desugar's parent validation. Everything else this list
    // declares is a binding around its classes.
    let depth = names.shadow.len();
    list_names_but_classes(stmts, &mut names.shadow);
    let sibling_start = names.top.len();
    let lifted_start = hoisted.len();
    for s in stmts.iter() {
        if let Stmt::ClassDecl { name, .. } = s {
            names.top.push(name.clone());
        }
    }
    let lifted_names_start = names.top.len();

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
        // K4-2 — a unit-class candidate runs its static initializers
        // inside `__cunit_<C>`, at the declaration's own position,
        // so the in-place lane has nothing left to get right for it.
        let unit_shape =
            multi_eval && super::class_unit_admit::admits_shape(ast, &stmts[idx], None);
        if !class_is_capture_free(ast, &stmts[idx], &names.top, &names.shadow)
            || (observable
                && !unit_shape
                && super::capturing_classes::decline::would_claim(ast, &stmts[idx]))
        {
            // The other half: a class that DOES read an outer local
            // cannot be lifted, and takes the runtime-value lane
            // instead (RFC 20260814-capturing-nested-class). Whatever
            // that lane declines stays here and stays loud.
            rewrite_capturing(ast, stmts, idx, counter, names);
            continue;
        }
        let old_name = match &stmts[idx] {
            Stmt::ClassDecl { name, .. } => name.clone(),
            _ => unreachable!(),
        };
        // RFC 20260917-class-eval-unit K1 — a unit-class candidate whose
        // name another class shares takes the collision rename, so the
        // reference census below counts this class's references only.
        let collides = super::hoist_nested_classes_rename::name_collides(
            &names.top,
            sibling_start,
            &old_name,
            unit_shape,
            name_counts,
        );
        let mut residual: Option<String> = None;
        if collides {
            let lifted = &mut hoisted[lifted_start..];
            let new_name = super::hoist_nested_classes_rename::apply_collision_rename(
                ast, stmts, lifted, idx, &old_name, counter,
            );
            names.top.push(new_name.clone());
            residual = Some(new_name);
        } else {
            names.top.push(old_name.clone());
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
            .unwrap_or_else(|| names.top.last().cloned().unwrap_or_default());
        if unit_shape
            && super::hoist_nested_classes_unit::take_unit_lane(
                ast,
                stmts,
                idx,
                (&binding, name_counts),
                hoisted,
            )
        {
            continue;
        }
        if unit_shape
            && observable
            && in_place_for_statics(
                ast,
                stmts,
                idx,
                counter,
                (&hoisted[lifted_start..], &binding),
                names,
            )
        {
            names.top.pop();
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

    // K6-1 — a class this level just hoisted took its member bodies
    // along, and `stmts` now holds only its residual, so the walk
    // below never sees them. A class expression written in one of
    // those bodies (its use-site splice put the ClassDecl there)
    // would stay put and abort loud in check. Scan them here, after
    // this level's hoist and over this level's entries only (a nested
    // walk scans what it hoists itself): an inner class extending the
    // outer lands after it in `hoisted`, and the outer's name is
    // already in `names.top`. No frame is around them any more.
    let lifted_end = hoisted.len();
    let around = std::mem::take(&mut names.shadow);
    for j in lifted_start..lifted_end {
        if !matches!(hoisted[j], Stmt::ClassDecl { .. }) {
            continue;
        }
        let mut cls = std::mem::replace(&mut hoisted[j], Stmt::Multi(Vec::new()));
        walk_child(ast, &mut cls, names, hoisted, counter, name_counts, true);
        hoisted[j] = cls;
    }
    names.shadow = around;

    // What this level still binds once its hoisted classes are gone —
    // a class the capturing lane kept is a local now. A name this
    // level hoisted under resolves at the top level instead, which is
    // where its residual binding (if any) is read from too.
    names.shadow.truncate(depth);
    let lifted = names.top[lifted_names_start..].to_vec();
    let mut local: Vec<String> = Vec::new();
    list_names(stmts, &mut local);
    names
        .shadow
        .extend(local.into_iter().filter(|n| !lifted.contains(n)));
    for s in stmts.iter_mut() {
        walk_child(ast, s, names, hoisted, counter, name_counts, multi_eval);
    }
    names.shadow.truncate(depth);
}
