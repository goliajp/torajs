//! What a scope binds — the census side of the shadow-aware class
//! reference rewrite (`class_globals_shadow`).
//!
//! Two questions live next door to each other and are easy to
//! confuse: "which names does this walk still see as classes" is the
//! walk's, next door; "which names does this scope rebind" is this
//! one's. Keeping the second here leaves the walk reading as a walk.
//!
//! And the second question has two answers, one per kind of scope. A
//! FUNCTION scope is collected to its own boundary at any block
//! depth, because `var` reaches the whole body and one conservative
//! set has to serve it. A BLOCK scope binds only what is written
//! directly in it, because that is all it can shadow — collecting a
//! block the function way would cost every enclosing statement its
//! own `C` as well.

use super::Stmt;
use super::desugar_with::walk::stmt_children_ref;
use std::collections::HashSet;

/// The names a nested BLOCK contributes to the enclosing FUNCTION
/// scope — `var` at any depth, and the §B.3.3 `function` a block
/// declares. A `let` / `const` / `class` in there belongs to its own
/// block and is left to `collect_block_decls`; collecting it here is
/// what made a deep `const R` hide `R` for the whole body.
fn collect_fn_scoped_only(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name, is_var: true, ..
            }
            | Stmt::FnDecl { name, .. } => {
                out.insert(name.clone());
            }
            _ => {}
        }
        if matches!(s, Stmt::FnDecl { .. }) {
            continue;
        }
        for c in stmt_children_ref(s) {
            collect_fn_scoped_only(std::slice::from_ref(c), out);
        }
    }
}

pub(super) fn collect_scope_decls(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        // Anything that is not written directly in this list sits in a
        // scope of its own; only what that scope hands UP to the
        // function (a `var`, a §B.3.3 `function`) is collected here.
        if !matches!(
            s,
            Stmt::LetDecl { .. } | Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } | Stmt::Multi(_)
        ) {
            collect_fn_scoped_only(std::slice::from_ref(s), out);
            continue;
        }
        match s {
            Stmt::LetDecl { name, .. }
            | Stmt::FnDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Multi(b) => collect_scope_decls(b, out),
            _ => {}
        }
    }
}

/// The names one BLOCK scope binds: the lexical declarations written
/// directly in it.
///
/// `var` is not one of them — it belongs to the enclosing function
/// scope, which [`collect_scope_decls`] already collects. A nested
/// `Block` is not descended into either: it opens its own frame, and
/// what it binds is invisible from out here. `Multi` IS descended
/// into, precisely because it is not a scope — it is a compiler-made
/// grouping whose declarations land in this one.
pub(super) fn collect_block_decls(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            }
            | Stmt::FnDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Multi(b) => collect_block_decls(b, out),
            Stmt::ExportDecl { inner: Some(i), .. } => {
                collect_block_decls(std::slice::from_ref(i), out)
            }
            _ => {}
        }
    }
}

/// The class whose own scope a synthesized top-level item belongs to,
/// if any. `desugar_classes` hoists every ctor / method / static
/// block / static field initialiser / computed key out of the class
/// and names it after the class it came from, so the name is the only
/// record left that the body was written inside the class scope.
///
/// The prefixes are tried longest-first (`__cmany_` before `__cm_`),
/// and so are the class names: a member name can contain `__`, and a
/// class can be called `C_3`, so splitting at the first separator
/// answers the wrong thing — the same trap `current_class` fell into
/// until rotation 585.
pub(super) fn owner_class<'a>(name: &str, classes: &'a HashSet<String>) -> Option<&'a String> {
    const PREFIXES: [&str; 9] = [
        "__ctorany_",
        "__cmany_",
        "__smany_",
        "__ccmk_",
        "__new_",
        "__cm_",
        "__sm_",
        "__sb_",
        "__sf_",
    ];
    let rest = PREFIXES.iter().find_map(|p| name.strip_prefix(p))?;
    classes
        .iter()
        .filter(|c| {
            rest.strip_prefix(c.as_str())
                .is_some_and(|tail| tail.is_empty() || tail.starts_with('_'))
        })
        .max_by_key(|c| c.len())
}
