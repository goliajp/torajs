//! `instanceof` answer-set computation — which runtime tags mean
//! "an instance of class C". Two halves, matching the two ways an
//! instance wears its identity:
//!
//!   * [`compute_descendant_tags`] — the CONSTANT half: every class
//!     on a `class_parents` chain ending at the target contributes
//!     its baked class tag, compared by `ICmp::Eq` at the site.
//!   * [`generic_half_tags`] — the NAME-IDENTITY half: a generic
//!     class's instances wear per-specialization anon-pool tags
//!     minted while Pass 2 lowers the mono factories, which the
//!     constant chain can never list. Every spec row carries its
//!     class's name (404-01), so one `__torajs_generic_tag_match`
//!     call per generic class in the answer set — the target itself
//!     when generic (405-03), plus every generic DESCENDANT on the
//!     chain (rotation 411: `new Wide<string>() instanceof Box`
//!     where `Wide<U> extends Box<U>`) — covers all its
//!     specializations by row-name identity.
//!
//! Pure reads over `LowerCtx`; the emit half stays in the parent
//! module.

use std::collections::HashMap;

use crate::ssa_lower::LowerCtx;

pub(super) fn compute_descendant_tags(ctx: &LowerCtx<'_>, class_name: &str) -> Vec<u32> {
    descendant_tags(&ctx.ast.class_parents, ctx.class_name_to_tag, class_name)
}

/// The constant tags of `class_name` and of every class whose
/// `class_parents` chain reaches it, sorted; empty for a name that
/// is no declared class.
pub(crate) fn descendant_tags(
    class_parents: &HashMap<String, Option<String>>,
    class_name_to_tag: &HashMap<String, u32>,
    class_name: &str,
) -> Vec<u32> {
    let mut descendant_tags: Vec<u32> = Vec::new();
    if !class_parents.contains_key(class_name) {
        return descendant_tags;
    }
    for c in class_parents.keys() {
        if chain_reaches(class_parents, c, class_name)
            && let Some(tag) = class_name_to_tag.get(c)
        {
            descendant_tags.push(*tag);
        }
    }
    descendant_tags.sort();
    descendant_tags.dedup();
    descendant_tags
}

/// 405-03 — the runtime tag of a GENERIC class (a name in
/// `class_name_to_tag` that also has a generic struct decl). Its
/// instances never wear this tag — the placeholder row it names is
/// what the specialization rows copy their identity from, and the
/// name comparison in `__torajs_generic_tag_match` is keyed off it.
pub(super) fn generic_class_tag(ctx: &LowerCtx<'_>, class_name: &str) -> Option<i64> {
    if !is_generic_class(ctx, class_name) {
        return None;
    }
    ctx.class_name_to_tag.get(class_name).map(|t| *t as i64)
}

/// Does `class_name`'s instances wear per-specialization tags? Two
/// ways: the class declares type params (`generic_struct_decls`), or
/// the implicit-generics pass genericized its factory — an
/// un-annotated ctor parameter turns `__new_C` into a template whose
/// monos are `__new_C$$…`, and `write_class_tag` mints each mono its
/// own tag with the class's name. The class itself carries no type
/// params, so the first table never lists it, and the answer set had
/// only the template's constant tag: `new C(1) instanceof C` answered
/// false for every class whose ctor parameter was un-annotated, and
/// the private brand check on such an instance (the same membership
/// test) refused the class's own `this.#x = v`.
/// The class tags a typed slot of the class `class_name` admits, the
/// class's and every subclass's, when the class tag alone decides it:
/// `None` for a name that is no declared class and for a class with a
/// generic half ([`generic_half_tags`]), whose instances need the name
/// comparison too.
pub(crate) fn class_tags_for_catch(ctx: &LowerCtx<'_>, class_name: &str) -> Option<Vec<u32>> {
    let tags = compute_descendant_tags(ctx, class_name);
    (!tags.is_empty() && generic_half_tags(ctx, class_name).is_empty()).then_some(tags)
}

fn is_generic_class(ctx: &LowerCtx<'_>, class_name: &str) -> bool {
    if ctx.generic_struct_decls.contains_key(class_name) {
        return true;
    }
    let prefix = format!("__new_{class_name}$$");
    ctx.fn_table.keys().any(|k| k.starts_with(&prefix))
}

/// The name-identity half's tag set (see the module doc): the target
/// class itself when generic, plus every generic descendant on the
/// `class_parents` chain.
pub(super) fn generic_half_tags(ctx: &LowerCtx<'_>, class_name: &str) -> Vec<i64> {
    let mut tags: Vec<i64> = Vec::new();
    if let Some(t) = generic_class_tag(ctx, class_name) {
        tags.push(t);
    }
    if ctx.ast.class_parents.contains_key(class_name) {
        for c in ctx.ast.class_parents.keys() {
            if c != class_name
                && is_generic_class(ctx, c)
                && chain_reaches(&ctx.ast.class_parents, c, class_name)
                && let Some(tag) = ctx.class_name_to_tag.get(c)
            {
                tags.push(*tag as i64);
            }
        }
    }
    tags.sort();
    tags.dedup();
    tags
}

/// Does `c`'s `class_parents` chain (self included) reach `target`?
/// Depth-capped like the pre-split walk — the parent map is acyclic
/// by construction, the cap is a defensive bound only.
fn chain_reaches(class_parents: &HashMap<String, Option<String>>, c: &str, target: &str) -> bool {
    let mut cur = Some(c.to_string());
    let mut depth = 0u32;
    while let Some(name) = cur {
        if depth > 64 {
            break;
        }
        if name == target {
            return true;
        }
        cur = class_parents.get(&name).and_then(|p| p.clone());
        depth += 1;
    }
    false
}
