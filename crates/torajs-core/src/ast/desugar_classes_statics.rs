//! `desugar_classes` static-member rewrite table builder (chunk 177,
//! 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs` (pre-extract was the
//! Pass 2.5 region between Pass 2 expr-arena rewrite and Pass 3
//! stmt-list rewrite). Pure function — consumes `class_index`, returns
//! a `HashMap<(ClassName, MemberName), ReplacementIdent>` consumed
//! later in Pass 3 to rewrite `Expr::Member { obj: Ident("ClassName"),
//! name }` into a plain `Expr::Ident(__sf_<C>__<n> | __sm_<C>__<m>)`.
//!
//! Two construction stages, original ordering preserved:
//!   1. Each class's own statics (StaticInit::Field + static method)
//!      register `__sf_<C>__<n>` / `__sm_<C>__<m>` keyed by
//!      `(C, name)`.
//!   2. V3-18 wedge — static inheritance per ES §15.7: walk every
//!      class's parent chain, alias inherited names to the parent's
//!      binding via `.entry().or_insert_with(...)` (sub's own statics
//!      already entered first, so they take precedence).
//!
//! Body verbatim from pre-extract; the shadowed-local `parent_map` at
//! the original construction site stays here (Pass 1 declared the
//! same name in outer scope at line 181; Pass 2.5 was already shadowing
//! it). No `&mut Ast` mutation — pure data computation.

use super::desugar_classes_super::ClassIndexEntry;
use super::*;
use std::collections::HashMap;

pub(super) fn build_static_member_rewrites(
    class_index: &[ClassIndexEntry],
) -> HashMap<(String, String), String> {
    // M-OO.4 — collect static-member rewrite tables: keys are
    // `(ClassName, member_name)` → flat replacement ident
    // (`__sf_<C>__<n>` for fields, `__sm_<C>__<m>` for methods). After
    // emitting the desugared decls, a second walk over `ast.exprs`
    // rewrites every `Expr::Member { obj: Ident("ClassName"), name }`
    // whose key is in the table to a plain `Expr::Ident(replacement)`.
    let mut static_member_rewrites: HashMap<(String, String), String> = HashMap::new();
    for (_, cname, _, _, _, sis, _, _, sms) in class_index {
        // P8.3-A3 — only StaticInit::Field entries are addressable as
        // `ClassName.member`; static blocks have no member name and are
        // emitted as `__sb_<C>__<idx>` named fns called at top level,
        // so they do not contribute to the static-member rewrite table.
        for si in sis {
            if let StaticInit::Field(sf) = si {
                static_member_rewrites.insert(
                    (cname.clone(), sf.name.clone()),
                    format!("__sf_{cname}__{}", sf.name),
                );
            }
        }
        for sm in sms {
            static_member_rewrites.insert(
                (cname.clone(), sm.name.clone()),
                format!("__sm_{cname}__{}", sm.name),
            );
        }
    }
    // V3-18 wedge — static inheritance per ES spec §15.7. When
    // `class Sub extends Base { ... }`, `Sub.greet` should resolve
    // to `Base.greet` (and `Sub.count` to `Base.count`), unless Sub
    // overrides them with its own static. Pre-fix
    // `Sub.<inherited_static>` failed at typecheck with
    // 'unknown identifier `Sub`' because the rewrite table only
    // recorded each class's own statics.
    //
    // Walk every class's parent chain, alias inherited static names
    // to the parent's __sf_/__sm_ binding. Sub's own statics already
    // take precedence (entered above). Multi-level chains (Sub →
    // Mid → Base) work transitively because the loop visits the
    // chain in order.
    let parent_map: HashMap<String, Option<String>> = class_index
        .iter()
        .map(|(_, c, _, p, _, _, _, _, _)| (c.clone(), p.clone()))
        .collect();
    let mut class_static_index: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    for (_, cname, _, _, _, sis, _, _, sms) in class_index {
        class_static_index.insert(
            cname.clone(),
            (
                // P8.3-A3 — same Field-only filter as the rewrite table:
                // static blocks have no member name to inherit.
                sis.iter()
                    .filter_map(|si| match si {
                        StaticInit::Field(sf) => Some(sf.name.clone()),
                        StaticInit::Block(_) => None,
                    })
                    .collect(),
                sms.iter().map(|sm| sm.name.clone()).collect(),
            ),
        );
    }
    for (_, cname, _, parent, _, _, _, _, _) in class_index {
        let mut cur = parent.clone();
        while let Some(p) = cur {
            if let Some((p_sfs, p_sms)) = class_static_index.get(&p) {
                for sf_name in p_sfs {
                    let key = (cname.clone(), sf_name.clone());
                    static_member_rewrites
                        .entry(key)
                        .or_insert_with(|| format!("__sf_{p}__{sf_name}"));
                }
                for sm_name in p_sms {
                    let key = (cname.clone(), sm_name.clone());
                    static_member_rewrites
                        .entry(key)
                        .or_insert_with(|| format!("__sm_{p}__{sm_name}"));
                }
            }
            cur = parent_map.get(&p).cloned().flatten();
        }
    }
    static_member_rewrites
}
