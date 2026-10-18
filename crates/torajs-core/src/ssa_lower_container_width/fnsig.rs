//! Signature faces of the container-width injection: a fn-typed slot
//! widens its param / return faces by the width table, recursing into
//! container faces through the array-elem widener.

use super::widen_arr_elem;
use crate::ast::PropKey;
use crate::num_width::{SlotKey, WidthTable, fn_type_canon, split_fn_type};
use crate::ssa::Type;
use crate::ssa_lower::intern_fn_sig;

/// The container half of a signature face: an `Arr` or fn-typed param
/// / return widens through the face's own projection key. Anything
/// else is already answered by the scalar test at the call site.
///
/// The recursion terminates on the interner: `parse_type` builds a
/// composite's parts before it interns the composite, and interning
/// either reuses an existing id or appends, so a part's id is always
/// smaller than its container's. A face therefore descends through a
/// strictly decreasing id and cannot cycle. (A self-referential
/// spelling never reaches here — `parse_type` recurses on the alias
/// first. Struct layouts are the one shape that IS interned
/// reserve-first, for the recursive generic alias, so the argument above
/// does not hold for them: an `Obj` face, like an `Obj` ELEMENT of an
/// array face, goes to `widen_struct_fields` and its own cycle guard.)
fn widen_face(
    ty: Type,
    ann: Option<&str>,
    key: &SlotKey,
    table: &WidthTable,
    arr_layouts: &mut crate::ssa::ArrLayouts,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
) -> Type {
    match ty {
        Type::Arr(_) => widen_arr_elem(ty, ann, key, table, arr_layouts, struct_layouts, fn_sigs),
        // A fn-typed param is itself a signature whose faces live on
        // the projections of THIS key — the flow site glued them onto
        // the resident's Ret / Param classes the same way. Without
        // this, `via = (f: NumsFn): number[] => f(7)` read `f`'s
        // result off the annotation's narrow SigId while the argument
        // actually passed had already widened (553-01).
        Type::Closure(_) | Type::FnSig(_) => {
            widen_fn_sig_by_key(ty, key, table, arr_layouts, struct_layouts, fn_sigs)
        }
        // A struct face (`() => A`) widens its number fields by the
        // projection's field points, which the resident's own Ret joined;
        // left narrow, `f().right` read a resident's f64 field as I64
        // bits. `widen_struct_fields` guards its own cycles through Obj
        // and Arr but descends into fn-typed fields and back here, so a
        // face whose types lead back to themselves is left at parse width.
        Type::Obj(_)
            if !cyclic(
                ty,
                arr_layouts,
                struct_layouts,
                fn_sigs,
                &mut (vec![], vec![]),
            ) =>
        {
            super::widen_struct_fields(ty, key, table, arr_layouts, struct_layouts, fn_sigs)
        }
        _ => ty,
    }
}

/// Does `ty` lead back to a type on the walk's path through Obj fields,
/// Arr elements and fn-typed parameters and results? `walk` is that path
/// and the types already proven to lead nowhere back.
fn cyclic(
    ty: Type,
    arr_layouts: &crate::ssa::ArrLayouts,
    struct_layouts: &[Vec<(PropKey, Type)>],
    fn_sigs: &[(Vec<Type>, Type)],
    walk: &mut (Vec<Type>, Vec<Type>),
) -> bool {
    let parts: Vec<Type> = match ty {
        Type::Obj(s) => struct_layouts[s.0 as usize]
            .iter()
            .map(|(_, f)| *f)
            .collect(),
        Type::Arr(a) => vec![arr_layouts[a.0 as usize]],
        Type::Closure(s) | Type::FnSig(s) => {
            let (ps, r) = &fn_sigs[s.0 as usize];
            ps.iter().copied().chain(std::iter::once(*r)).collect()
        }
        _ => return false,
    };
    if walk.0.contains(&ty) {
        return true;
    }
    if walk.1.contains(&ty) {
        return false;
    }
    walk.0.push(ty);
    let hit = parts
        .into_iter()
        .any(|p| cyclic(p, arr_layouts, struct_layouts, fn_sigs, walk));
    walk.0.pop();
    if !hit {
        walk.1.push(ty);
    }
    hit
}

/// F1 (§5.6) — widen a fn-type annotation's number faces per the
/// width table. The annotation's canonical spelling is the nominal
/// class key shared by every slot spelled with it and every function
/// flowing through those slots (`num_width/fnsig.rs`); its `__ret` /
/// `__p{i}` projections answer whether any resident's face is
/// f64-possible. All-integral residents keep the narrow signature.
///
/// A face is not always a scalar. An array-returning signature (`(n:
/// number) => number[]`) has its number domain one level down, in the
/// ELEMENT of the `__ret` projection, and asking only the scalar
/// question left the slot's signature narrow while the resident's own
/// `Ret` widened — the call then read an `Arr(F64)` through an
/// `Arr(I64)` slot and handed back the f64 bit pattern as an integer
/// (rotation 554: the shape only became reachable once `__fn(P)->(R)`
/// stopped decoding as an array of fns). Container faces therefore
/// recurse through `widen_face` on the same projection key.
pub(crate) fn widen_fn_sig(
    parsed: Type,
    ann: Option<&str>,
    key: &SlotKey,
    table: &WidthTable,
    arr_layouts: &mut crate::ssa::ArrLayouts,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
) -> Type {
    let (sid, is_closure) = match parsed {
        Type::Closure(s) => (s, true),
        Type::FnSig(s) => (s, false),
        _ => return parsed,
    };
    // Only a slot SPELLED with the canonical fn shape has a nominal
    // class to ask. An alias (`type NumsFn = (n: number) => Nums`)
    // reaches `fn_type_canon` as the bare alias name and answers None
    // — and for the same reason the analysis never unioned the slot
    // onto any class, so its widths live on its own key's projections.
    // Falling through to F5 is what the un-annotated global binding
    // already does (`ssa_lower_toplevel_globals::infer`); returning
    // the parse width instead left `via = (f: NumsFn): number[] =>
    // f(7)` reading `f`'s result through the narrow annotation SigId
    // while the argument actually passed had widened (553-01).
    let Some(canon) = ann.and_then(fn_type_canon) else {
        return widen_fn_sig_by_key(parsed, key, table, arr_layouts, struct_layouts, fn_sigs);
    };
    let Some((param_anns, ret_ann)) = split_fn_type(&canon) else {
        return widen_fn_sig_by_key(parsed, key, table, arr_layouts, struct_layouts, fn_sigs);
    };
    let ck = SlotKey::Class(canon.clone());
    let (param_tys, ret_ty) = fn_sigs[sid.0 as usize].clone();
    let mut new_params = param_tys.clone();
    for (i, pt) in new_params.iter_mut().enumerate() {
        let pk = SlotKey::Field(Box::new(ck.clone()), PropKey::from(format!("__p{i}")));
        if *pt == Type::I64
            && param_anns.get(i).copied() == Some("number")
            && table.field_is_f64(&ck, format!("__p{i}"))
        {
            *pt = Type::F64;
        } else {
            *pt = widen_face(
                *pt,
                param_anns.get(i).copied(),
                &pk,
                table,
                arr_layouts,
                struct_layouts,
                fn_sigs,
            );
        }
    }
    let new_ret = if ret_ty == Type::I64 && ret_ann == "number" && table.field_is_f64(&ck, "__ret")
    {
        Type::F64
    } else {
        let rk = SlotKey::Field(Box::new(ck.clone()), "__ret".into());
        widen_face(
            ret_ty,
            Some(ret_ann),
            &rk,
            table,
            arr_layouts,
            struct_layouts,
            fn_sigs,
        )
    };
    if new_params == param_tys && new_ret == ret_ty {
        return parsed;
    }
    let id = intern_fn_sig(fn_sigs, new_params, new_ret);
    if is_closure {
        Type::Closure(id)
    } else {
        Type::FnSig(id)
    }
}

/// F5 (§5.6 ②.6) — widen an fn-typed struct field's signature faces
/// by the field's own slot key: the fill site's `fn_value_flow` glued
/// the key's `__ret` / `__p{i}` projections onto the resident
/// function's Ret / Param classes, so the query needs no canonical
/// spelling (struct layouts carry no annotation strings). Scalar faces
/// widen on the I64 parse default (the sole widenable spelling);
/// container faces recurse through `widen_face`, per the note on
/// [`widen_fn_sig`].
pub(crate) fn widen_fn_sig_by_key(
    parsed: Type,
    key: &SlotKey,
    table: &WidthTable,
    arr_layouts: &mut crate::ssa::ArrLayouts,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
) -> Type {
    let (sid, is_closure) = match parsed {
        Type::Closure(s) => (s, true),
        Type::FnSig(s) => (s, false),
        _ => return parsed,
    };
    let (param_tys, ret_ty) = fn_sigs[sid.0 as usize].clone();
    let mut new_params = param_tys.clone();
    for (i, pt) in new_params.iter_mut().enumerate() {
        let pk = SlotKey::Field(Box::new(key.clone()), PropKey::from(format!("__p{i}")));
        if *pt == Type::I64 && table.field_is_f64(key, format!("__p{i}")) {
            *pt = Type::F64;
        } else {
            *pt = widen_face(*pt, None, &pk, table, arr_layouts, struct_layouts, fn_sigs);
        }
    }
    let new_ret = if ret_ty == Type::I64 && table.field_is_f64(key, "__ret") {
        Type::F64
    } else {
        let rk = SlotKey::Field(Box::new(key.clone()), "__ret".into());
        widen_face(
            ret_ty,
            None,
            &rk,
            table,
            arr_layouts,
            struct_layouts,
            fn_sigs,
        )
    };
    if new_params == param_tys && new_ret == ret_ty {
        return parsed;
    }
    let id = intern_fn_sig(fn_sigs, new_params, new_ret);
    if is_closure {
        Type::Closure(id)
    } else {
        Type::FnSig(id)
    }
}
