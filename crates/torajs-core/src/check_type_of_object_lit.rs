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

use torajs_wtf8::Wtf8;

use crate::ast::PropKey;
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
pub(crate) fn accessor_slot(name: &Wtf8) -> Option<(&'static str, &Wtf8)> {
    if let Some(prop) = name.strip_prefix("__getter_") {
        return Some(("__getter_", prop));
    }
    name.strip_prefix("__setter_").map(|p| ("__setter_", p))
}

/// The omit list rides an identifier-only spelling (destructuring
/// rest excludes bound names), so a key that is not a `&str` is never
/// a spread marker.
pub(crate) fn spread_omit_set(name: &Wtf8) -> Option<Vec<&str>> {
    let name = name.as_str()?;
    if name == "__spread__" {
        return Some(Vec::new());
    }
    name.strip_prefix("__spread_omit__:")
        .map(|s| s.split(',').filter(|k| !k.is_empty()).collect())
}

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    fields: &[(PropKey, ExprId)],
) -> Result<Type, String> {
    let mut field_tys: Vec<(PropKey, Type)> = Vec::new();
    // Rotation 267 — a spread whose source is Any has no static
    // field list, so the whole literal leaves the struct layer and
    // answers Any (the dynobj lane's runtime CopyDataProperties
    // names the fields). Remaining fields still typecheck below for
    // side effects.
    let mut dynobj_lane = false;
    for (n, eid) in fields {
        // S2.24 刀 4 — a CoverInitializedName field (`{ x = D }`)
        // reaching expression position is the §13.2.5.1 early error:
        // only a destructuring re-read (which the parser expands
        // before check ever runs) may consume it.
        if ast.objlit_cover_init_exprs.contains(eid) {
            return Err(format!(
                "shorthand property initializer `{n} = ...` is only valid in a destructuring pattern"
            ));
        }
        if let Some(omit) = spread_omit_set(n) {
            let src_ty = checker.type_of(ast, *eid)?;
            // Any-typed spread source → the whole literal goes to the
            // dynobj lane (see above). That holds for the
            // destructuring-rest omit form too: the lane's kernel walk
            // already excludes the named keys, so this gate had no
            // missing implementation behind it.
            if matches!(src_ty, Type::Any) {
                dynobj_lane = true;
                continue;
            }
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
                        (PropKey::from(prop), (**ret).clone())
                    }
                    _ => (sn.clone(), st.clone()),
                };
                if omit.iter().any(|o| sn == *o) {
                    continue;
                }
                if let Some(pos) = field_tys.iter().position(|(k, _)| *k == sn) {
                    field_tys[pos] = (sn, st);
                } else {
                    field_tys.push((sn, st));
                }
            }
        } else if ast.objlit_computed_keys.contains_key(eid) {
            // RFC 20260725-objlit-computed-key 刀 1 — a computed-key
            // field has no static name, so it contributes nothing to
            // the struct layer. Key and value still typecheck (side
            // effects are real). RFC 20260809 刀 1 — the whole
            // literal answers Any, the same exit the any-spread arm
            // takes: only the dynobj lane can ToPropertyKey the key
            // and name the property, and this holds at EVERY
            // position (return值 / argument / field init), not just
            // the declaration sites the degrade collector covers —
            // `return { [Symbol.dispose]() {} }` previously panicked
            // at the struct-lane lowering.
            let key_eid = ast.objlit_computed_keys[eid];
            checker.type_of(ast, key_eid)?;
            checker.type_of(ast, *eid)?;
            dynobj_lane = true;
        } else if n == "__proto__" && !ast.objlit_shorthand_proto_exprs.contains(eid) {
            // Rotation 434 — §B.3.1: a `__proto__: v` PropertyName
            // field is a [[Prototype]] set, not an own data field;
            // only the dynobj lane can express it
            // (`emit_dynobj_proto_field`), so the whole literal
            // answers Any, the same exit the computed-key arm takes.
            // The property SHORTHAND spelling stays an ordinary own
            // field and keeps the struct lane.
            checker.type_of(ast, *eid)?;
            dynobj_lane = true;
        } else {
            let ty = checker.type_of(ast, *eid)?;
            if let Some(pos) = field_tys.iter().position(|(k, _)| k == n) {
                field_tys[pos] = (n.clone(), ty);
            } else {
                field_tys.push((n.clone(), ty));
            }
        }
    }
    if dynobj_lane {
        return Ok(Type::Any);
    }
    Ok(Type::Struct(field_tys))
}
