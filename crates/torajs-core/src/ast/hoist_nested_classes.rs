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
//! - a renamed class answers `.name` with the synth name;
//! - block-scope visibility widens to the whole program (a name-only
//!   inner shadow of the same identifier inside the container would
//!   be renamed with it).

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
fn walk_container(
    ast: &mut Ast,
    stmts: &mut Vec<Stmt>,
    top_names: &mut Vec<String>,
    hoisted: &mut Vec<Stmt>,
    counter: &mut u32,
    name_counts: &std::collections::HashMap<String, u32>,
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
        if !class_is_capture_free(ast, &stmts[idx], top_names) {
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
        // Collision domain: everything visible at top level once the
        // hoist lands. Sibling entries pushed above shadow the check
        // for the class's own occurrence, so count duplicates.
        let collides = top_names.iter().filter(|n| **n == old_name).count() > 1
            || top_names[..sibling_start].contains(&old_name);
        if collides {
            let new_name = format!("__hc{}_{}", *counter, old_name);
            *counter += 1;
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
            top_names.push(new_name);
        } else {
            top_names.push(old_name);
        }
        let cls = std::mem::replace(&mut stmts[idx], Stmt::Multi(Vec::new()));
        hoisted.push(cls);
    }

    for s in stmts.iter_mut() {
        walk_child(ast, s, top_names, hoisted, counter, name_counts);
    }
}

/// Recurse into every child statement container of one statement.
/// `Box<Stmt>` positions (if/while bodies) recurse through here too;
/// a bare ClassDecl in single-statement position is a spec syntax
/// error and is left alone (stays loud).
fn walk_child(
    ast: &mut Ast,
    s: &mut Stmt,
    top_names: &mut Vec<String>,
    hoisted: &mut Vec<Stmt>,
    counter: &mut u32,
    name_counts: &std::collections::HashMap<String, u32>,
) {
    match s {
        Stmt::FnDecl { body, .. } => {
            walk_container(ast, body, top_names, hoisted, counter, name_counts)
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            walk_container(ast, v, top_names, hoisted, counter, name_counts)
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_child(ast, then_branch, top_names, hoisted, counter, name_counts);
            if let Some(eb) = else_branch {
                walk_child(ast, eb, top_names, hoisted, counter, name_counts);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => {
            walk_child(ast, body, top_names, hoisted, counter, name_counts)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                walk_child(ast, i, top_names, hoisted, counter, name_counts);
            }
            walk_child(ast, body, top_names, hoisted, counter, name_counts);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                walk_container(ast, &mut c.body, top_names, hoisted, counter, name_counts);
            }
            if let Some(d) = default {
                walk_container(ast, d, top_names, hoisted, counter, name_counts);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            walk_container(ast, body, top_names, hoisted, counter, name_counts);
            walk_container(ast, catch_body, top_names, hoisted, counter, name_counts);
            if let Some(fb) = finally_body {
                walk_container(ast, fb, top_names, hoisted, counter, name_counts);
            }
        }
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                walk_container(ast, &mut c.body, top_names, hoisted, counter, name_counts);
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                walk_container(ast, &mut m.body, top_names, hoisted, counter, name_counts);
            }
            for si in static_init {
                if let StaticInit::Block(v) = si {
                    walk_container(ast, v, top_names, hoisted, counter, name_counts);
                }
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => walk_child(ast, inner, top_names, hoisted, counter, name_counts),
        _ => {}
    }
}

/// Every body of the class resolves against top-level names only.
/// Instance-field initializers are already folded into the ctor body
/// by the parser (`finalize_class_field_inits`), so the ctor walk
/// covers them.
///
/// A computed member name is NOT in any body — the parser puts the key
/// expression in a side table and leaves a `__ccm_<n>__` sentinel
/// behind (`class_computed_keys`, and `class_computed_static_fields`
/// for a static field's initializer). Walking only the bodies made
/// `{ const k = "z"; class K { [k]() {…} } }` read as capture-free, so
/// it hoisted to the top level and the key no longer resolved there —
/// a warning plus a wrong answer at run time, not the loud abort every
/// other capturing shape gets.
fn class_is_capture_free(ast: &Ast, s: &Stmt, top_names: &[String]) -> bool {
    let Stmt::ClassDecl {
        name,
        type_params,
        parent,
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    else {
        return false;
    };
    if parent.is_some() {
        match ast.parent_ident_name(*parent) {
            Some(pn) => {
                if !top_names.iter().any(|t| t == pn) && !super::free_vars::is_global_name(pn) {
                    return false;
                }
            }
            // A heritage EXPRESSION reads names in the enclosing
            // scope — never capture-free for hoisting purposes.
            None => return false,
        }
    }
    let mut prebound: Vec<String> = top_names.to_vec();
    prebound.push(name.clone());
    prebound.extend(type_params.iter().cloned());
    prebound.push("arguments".into());

    let body_free = |params: &[Param], body: &[Stmt]| -> bool {
        let mut bound = prebound.clone();
        bound.extend(params.iter().map(|p| p.name.clone()));
        super::free_vars::free_vars_of_body(ast, &bound, body)
            .iter()
            // `super.m()` parses as a Call to the marker ident
            // `__supercall__m` — desugar_classes rewrites it from the
            // class's own parent link, so it is never a capture.
            .all(|n| n.starts_with("__supercall__"))
    };

    if let Some(c) = ctor
        && !body_free(&c.params, &c.body)
    {
        return false;
    }
    for m in methods.iter().chain(static_methods.iter()) {
        if !body_free(&m.params, &m.body) {
            return false;
        }
    }
    for si in static_init {
        let ok = match si {
            StaticInit::Field(f) => body_free(&[], &[Stmt::Expr(f.init)]),
            StaticInit::Block(v) => body_free(&[], v),
        };
        if !ok {
            return false;
        }
    }
    // Which side-table rows are THIS class's is a question the name
    // cannot answer — two fn bodies each declaring `class K` share a
    // key set — so the computed members are read back off the class
    // itself. A computed STATIC FIELD leaves no trace on the class at
    // all (it is neither a member nor a ctor-prefix write), so its
    // initializer is still matched by name: over-answering there
    // costs a same-named sibling's static field the hoist, which is
    // the loud direction.
    let ctor_body: &[Stmt] = ctor.as_ref().map_or(&[], |c| c.body.as_slice());
    let member_names: Vec<&str> = methods
        .iter()
        .chain(static_methods.iter())
        .map(|m| m.name.as_str())
        .collect();
    let own = super::capturing_classes::own_computed_members(ast, name, ctor_body, &member_names);
    let side_exprs = super::capturing_classes::keys_of(ast, name, &own)
        .into_iter()
        .map(|(_, key)| key)
        .chain(
            ast.class_computed_static_fields
                .iter()
                .filter(|(c, _, _)| c == name)
                .flat_map(|(c, sent, init)| {
                    // The KEY too (406-02) — it lives only under the
                    // side-table sentinel, so walking just the init
                    // read `{ let k = 5; class C { static [k] = … } }`
                    // as capture-free and hoisted it away from `k` —
                    // a warning plus a wrong answer at run time.
                    let key = ast.class_computed_keys.get(&(c.clone(), sent.clone()));
                    key.copied().into_iter().chain([*init])
                }),
        );
    for e in side_exprs {
        if !body_free(&[], &[Stmt::Expr(e)]) {
            return false;
        }
    }
    true
}
