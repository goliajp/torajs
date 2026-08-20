//! Whether a nested class may be hoisted at all — the admission half
//! of `hoist_nested_classes.rs`, split off at the file-size cap.
//!
//! The parent walks the tree and moves what it is allowed to move;
//! this answers what that is. The same boundary
//! `capturing_classes` / `capturing_classes::decline` draws, one lane
//! over.

use super::*;

/// Whether anything this class does AT ITS DEFINITION is observable
/// from outside it, which is what makes moving the class observable
/// too (420-01 / 420-02).
///
/// Two such things, both §15.7.14: a computed member NAME is evaluated
/// there (methods and accessors carry the parser's `__ccm_<n>__`
/// sentinel as their name; a computed static field leaves its row in
/// the side table instead, since it never becomes a `ClassMethod`),
/// and every static field initializer / static block RUNS there.
///
/// A class with neither can be lifted to the top level with nothing to
/// see: its bodies only run when something calls them.
pub(super) fn class_evaluation_is_observable(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::ClassDecl {
        name,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    else {
        return false;
    };
    !static_init.is_empty()
        || methods
            .iter()
            .chain(static_methods.iter())
            .any(|m| m.name.starts_with("__ccm_"))
        || ast
            .class_computed_static_fields
            .iter()
            .any(|(c, _, _)| c == name)
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
pub(super) fn class_is_capture_free(ast: &Ast, s: &Stmt, top_names: &[String]) -> bool {
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
        // A key (or static init) that reads `this` — a generator
        // state-machine field read minted by the lifted-local rewrite
        // — pins the class to its declaring scope: hoisted to module
        // top the reify would silently evaluate against
        // `__module_this` (undefined). `free_vars` cannot see it
        // (This is not an Ident), so ask the this-walker directly;
        // the capturing lane evaluates the key in place, which is
        // where §15.7.14 puts it.
        if super::capturing_classes::expr_says_this(ast, e) || !body_free(&[], &[Stmt::Expr(e)]) {
            return false;
        }
    }
    true
}
