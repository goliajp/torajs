//! `Expr::ObjectLit { fields }` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::ObjectLit` arm
//! as chunk-91 of the type_of_inner decomp.
//!
//! Spread members (encoded with sentinel name `__spread__`) unfold
//! into the source struct's fields. Inline members win on key
//! collision per JS spec. Final type is a freshly-merged
//! `Type::Struct` preserving order: spread sources first (in
//! textual order), then inline members; later re-occurrences of a
//! key REPLACE the earlier slot's type and position.
//!
//! The destructuring-rest desugar (chunk 707) encodes its omit set
//! in the sentinel name — `__spread_omit__:p,q` unfolds the source
//! minus the named keys, so `const { p, ...rest } = o` types `rest`
//! as `o`'s struct without `p`. See [`spread_omit_set`].

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

/// Decode a spread sentinel field name: `__spread__` answers an empty
/// omit set; `__spread_omit__:p,q` answers `{p, q}`; anything else is
/// a regular member (`None`). Shared decode contract with
/// `ssa_lower_object_lit`'s unfold.
/// Split an object-literal accessor slot name into its prefix and the
/// property it stands for: `__getter_b` → `("__getter_", "b")`. A plain
/// field answers `None`. Shared decode contract with
/// `ssa_lower_object_lit`'s spread unfold (RFC 20260714-objlit-accessor).
pub(crate) fn accessor_slot(name: &str) -> Option<(&'static str, &str)> {
    if let Some(prop) = name.strip_prefix("__getter_") {
        return Some(("__getter_", prop));
    }
    name.strip_prefix("__setter_").map(|p| ("__setter_", p))
}

pub(crate) fn spread_omit_set(name: &str) -> Option<Vec<&str>> {
    if name == "__spread__" {
        return Some(Vec::new());
    }
    name.strip_prefix("__spread_omit__:")
        .map(|s| s.split(',').filter(|k| !k.is_empty()).collect())
}

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    fields: &[(String, ExprId)],
) -> Result<Type, String> {
    let mut field_tys: Vec<(String, Type)> = Vec::new();
    for (n, eid) in fields {
        if let Some(omit) = spread_omit_set(n) {
            let src_ty = checker.type_of(ast, *eid)?;
            let Type::Struct(src_fields) = &src_ty else {
                return Err(format!(
                    "object spread source must be a struct, got {src_ty:?}"
                ));
            };
            for (sn, st) in src_fields.iter() {
                // RFC 20260714-objlit-accessor blade 3 — CopyDataProperties
                // (ES §7.3.25) reaches each own key through [[Get]], so an
                // accessor on the source contributes the getter's RESULT as
                // a DATA property. `__getter_b` is a layout field holding
                // the getter closure; copying it verbatim would have given
                // `rest` a `__getter_b` function instead of a `b` value.
                // A setter is not a source of data — its property already
                // came from the paired getter, and a lone setter reads
                // `undefined` (recorded gap: it drops out here instead).
                let (sn, st) = match accessor_slot(sn) {
                    Some(("__setter_", _)) => continue,
                    Some(("__getter_", prop)) => {
                        let Type::Function(_, ret) = st else {
                            return Err(format!("accessor `{prop}` is not a getter closure"));
                        };
                        (prop.to_string(), (**ret).clone())
                    }
                    _ => (sn.clone(), st.clone()),
                };
                if omit.contains(&sn.as_str()) {
                    continue;
                }
                if let Some(pos) = field_tys.iter().position(|(k, _)| *k == sn) {
                    field_tys[pos] = (sn, st);
                } else {
                    field_tys.push((sn, st));
                }
            }
        } else {
            let ty = checker.type_of(ast, *eid)?;
            if let Some(pos) = field_tys.iter().position(|(k, _)| k == n) {
                field_tys[pos] = (n.clone(), ty);
            } else {
                field_tys.push((n.clone(), ty));
            }
        }
    }
    Ok(Type::Struct(field_tys))
}
