//! W4 (ann-width RFC §5.4 D2) — container element width injection.
//!
//! `parse_type` resolves `number[]` to the I64-elem default; the
//! module-wide num_width analysis decides per *alias class* whether
//! every value reaching the array's elements is provably integral.
//! Each annotation-consuming site calls `widen_arr_elem` with its
//! slot key right after parse_type: when the elem face is `number`
//! and the class is f64-possible, the array type re-interns with an
//! F64 element (recursively for `number[][]` — the inner face keys
//! through the Elem spelling, which the table canonicalizes onto the
//! congruence-closed class).
//!
//! The runtime is width-agnostic here: array slots are always 8 bytes
//! (`torajs-arr` layout), so this changes only the compiler-side
//! interpretation — load/store/print sites all read
//! `arr_layouts[arr_id]` and follow automatically.

use crate::ast::PropKey;
use crate::num_width::{SlotKey, WidthTable, fn_type_canon, split_fn_type};
use crate::ssa::{StructId, Type};
use crate::ssa_lower::{intern_arr_layout, intern_fn_sig};

/// Intern a struct layout by structural equality (mirrors the
/// TypeDecl intern walk); widened twins get fresh StructIds so
/// differently-classed slots of one structural annotation can carry
/// different field widths.
fn intern_struct_layout(
    layouts: &mut Vec<Vec<(PropKey, Type)>>,
    fields: Vec<(PropKey, Type)>,
) -> StructId {
    for (i, ex) in layouts.iter().enumerate() {
        if *ex == fields {
            return StructId(i as u32);
        }
    }
    let id = StructId(layouts.len() as u32);
    layouts.push(fields);
    id
}

/// Widen a parsed / inferred container type's number faces per the
/// width table: array elems (`widen_arr_elem`) and struct fields.
/// `key` is the slot (or literal Anon origin / nominal Class) holding
/// the container. An I64 field is a number-domain candidate by
/// construction; the explicit `: i64` struct-field narrow spelling is
/// an accepted edge (recorded in rfc §5.4 — the analysis has no field
/// annotations, and a fract write into such a field was bit-punning
/// silent-wrong before W4 anyway).
pub(crate) fn widen_container_ty(
    parsed: Type,
    ann: Option<&str>,
    key: &SlotKey,
    table: &WidthTable,
    arr_layouts: &mut Vec<Type>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
) -> Type {
    match parsed {
        Type::Arr(_) => widen_arr_elem(parsed, ann, key, table, arr_layouts),
        Type::Obj(_) => {
            widen_struct_fields(parsed, key, table, arr_layouts, struct_layouts, fn_sigs)
        }
        Type::Closure(_) | Type::FnSig(_) => {
            widen_fn_sig(parsed, ann, key, table, arr_layouts, fn_sigs)
        }
        _ => parsed,
    }
}

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
/// first. `Obj` faces are deliberately not walked: struct layouts are
/// the one shape that IS interned reserve-first, for the recursive
/// generic alias, so the argument above does not hold for them.)
fn widen_face(
    ty: Type,
    ann: Option<&str>,
    key: &SlotKey,
    table: &WidthTable,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
) -> Type {
    match ty {
        Type::Arr(_) => widen_arr_elem(ty, ann, key, table, arr_layouts),
        // A fn-typed param is itself a signature whose faces live on
        // the projections of THIS key — the flow site glued them onto
        // the resident's Ret / Param classes the same way. Without
        // this, `via = (f: NumsFn): number[] => f(7)` read `f`'s
        // result off the annotation's narrow SigId while the argument
        // actually passed had already widened (553-01).
        Type::Closure(_) | Type::FnSig(_) => {
            widen_fn_sig_by_key(ty, key, table, arr_layouts, fn_sigs)
        }
        _ => ty,
    }
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
    arr_layouts: &mut Vec<Type>,
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
        return widen_fn_sig_by_key(parsed, key, table, arr_layouts, fn_sigs);
    };
    let Some((param_anns, ret_ann)) = split_fn_type(&canon) else {
        return widen_fn_sig_by_key(parsed, key, table, arr_layouts, fn_sigs);
    };
    let ck = SlotKey::Class(canon.clone());
    let (param_tys, ret_ty) = fn_sigs[sid.0 as usize].clone();
    let mut new_params = param_tys.clone();
    for (i, pt) in new_params.iter_mut().enumerate() {
        let pk = SlotKey::Field(Box::new(ck.clone()), PropKey::from(format!("__p{i}")));
        if *pt == Type::I64
            && param_anns.get(i).copied() == Some("number")
            && table.field_is_f64(&ck, &format!("__p{i}"))
        {
            *pt = Type::F64;
        } else {
            *pt = widen_face(
                *pt,
                param_anns.get(i).copied(),
                &pk,
                table,
                arr_layouts,
                fn_sigs,
            );
        }
    }
    let new_ret = if ret_ty == Type::I64 && ret_ann == "number" && table.field_is_f64(&ck, "__ret")
    {
        Type::F64
    } else {
        let rk = SlotKey::Field(Box::new(ck.clone()), "__ret".into());
        widen_face(ret_ty, Some(ret_ann), &rk, table, arr_layouts, fn_sigs)
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
    arr_layouts: &mut Vec<Type>,
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
        if *pt == Type::I64 && table.field_is_f64(key, &format!("__p{i}")) {
            *pt = Type::F64;
        } else {
            *pt = widen_face(*pt, None, &pk, table, arr_layouts, fn_sigs);
        }
    }
    let new_ret = if ret_ty == Type::I64 && table.field_is_f64(key, "__ret") {
        Type::F64
    } else {
        let rk = SlotKey::Field(Box::new(key.clone()), "__ret".into());
        widen_face(ret_ty, None, &rk, table, arr_layouts, fn_sigs)
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

/// Does `ty` reach `target` through Obj fields / Arr elems?
/// Self-referential layouts (linked lists, cycle fixtures) must not
/// be widened at all: re-interning the outer layout while the
/// recursive field keeps the old sid would split one logical type
/// into two disagreeing layouts (bit-punning on the inner hop), and
/// closing the recursion needs graph interning. Cyclic types keep
/// the parse-default widths — the pre-W4 behavior (recorded as a D3
/// carve in rfc §5.4).
fn ty_reaches(
    ty: Type,
    target: StructId,
    arr_layouts: &[Type],
    struct_layouts: &[Vec<(PropKey, Type)>],
    seen: &mut Vec<StructId>,
) -> bool {
    match ty {
        Type::Obj(s) => {
            if s == target {
                return true;
            }
            if seen.contains(&s) {
                return false;
            }
            seen.push(s);
            struct_layouts[s.0 as usize]
                .clone()
                .iter()
                .any(|(_, f)| ty_reaches(*f, target, arr_layouts, struct_layouts, seen))
        }
        Type::Arr(id) => ty_reaches(
            arr_layouts[id.0 as usize],
            target,
            arr_layouts,
            struct_layouts,
            seen,
        ),
        _ => false,
    }
}

/// Widen a struct type's I64 fields whose alias-class field points
/// are f64-possible, re-interning the layout. Recurses into Arr /
/// Obj fields through the Field-spelled keys (the table
/// canonicalizes them onto the congruence-closed classes).
pub(crate) fn widen_struct_fields(
    parsed: Type,
    key: &SlotKey,
    table: &WidthTable,
    arr_layouts: &mut Vec<Type>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
) -> Type {
    let Type::Obj(sid) = parsed else {
        return parsed;
    };
    let mut seen = Vec::new();
    if struct_layouts[sid.0 as usize]
        .clone()
        .iter()
        .any(|(_, f)| ty_reaches(*f, sid, arr_layouts, struct_layouts, &mut seen))
    {
        // Self-referential layout: every such sid is nominal (the
        // generic-instantiation memo or the non-generic reserved-sid
        // pass are the only producers of a layout that mentions its
        // own sid), so widening IN PLACE is sound — the recursive
        // field keeps pointing at the same sid and every consumer of
        // the key sees the widened layout. The analysis joins all
        // consuming slots of one key onto its Class spelling
        // (alias.rs::register_inst_key), so the first consumer's
        // query already answers the joined width — no lowering-order
        // sensitivity. (Pre-fix this arm bailed entirely: the W4 D3
        // carve that kept `Rec<number>` fract fields bit-punning.)
        widen_struct_fields_in_place(
            sid,
            key,
            table,
            arr_layouts,
            struct_layouts,
            fn_sigs,
            &mut Vec::new(),
        );
        return parsed;
    }
    let layout = struct_layouts[sid.0 as usize].clone();
    let mut widened = layout.clone();
    let mut changed = false;
    for (fname, fty) in widened.iter_mut() {
        let fkey = SlotKey::Field(Box::new(key.clone()), fname.clone());
        let new_fty = match *fty {
            Type::I64 if table.field_is_f64(key, fname.clone()) => Type::F64,
            Type::Arr(_) => widen_arr_elem(*fty, None, &fkey, table, arr_layouts),
            Type::Obj(_) => {
                widen_struct_fields(*fty, &fkey, table, arr_layouts, struct_layouts, fn_sigs)
            }
            // F5 — fn-typed fields widen by the field's own key
            // projections (the dispatch face reads this signature).
            Type::FnSig(_) | Type::Closure(_) => {
                widen_fn_sig_by_key(*fty, &fkey, table, arr_layouts, fn_sigs)
            }
            _ => *fty,
        };
        if new_fty != *fty {
            *fty = new_fty;
            changed = true;
        }
    }
    if !changed {
        return parsed;
    }
    Type::Obj(intern_struct_layout(struct_layouts, widened))
}

/// In-place variant for self-referential (nominal) layouts: mutate
/// `struct_layouts[sid]` directly instead of re-interning, so the
/// recursive field's same-sid reference stays consistent. `visiting`
/// breaks mutual cycles (`A<T>` ↔ `B<T>`); a sid is widened at most
/// once per top-level call — re-entry is idempotent anyway since the
/// I64 faces are already F64. Non-cyclic Obj fields keep the original
/// re-intern semantics (their sids may be structurally shared by
/// unrelated slots — mutating them would leak width across slots).
fn widen_struct_fields_in_place(
    sid: StructId,
    key: &SlotKey,
    table: &WidthTable,
    arr_layouts: &mut Vec<Type>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    visiting: &mut Vec<StructId>,
) {
    if visiting.contains(&sid) {
        return;
    }
    visiting.push(sid);
    let mut widened = struct_layouts[sid.0 as usize].clone();
    for (fname, fty) in widened.iter_mut() {
        let fkey = SlotKey::Field(Box::new(key.clone()), fname.clone());
        match *fty {
            Type::I64 if table.field_is_f64(key, fname.clone()) => *fty = Type::F64,
            Type::Arr(_) => *fty = widen_arr_elem(*fty, None, &fkey, table, arr_layouts),
            Type::Obj(inner) => {
                let mut seen = Vec::new();
                if struct_layouts[inner.0 as usize]
                    .clone()
                    .iter()
                    .any(|(_, f)| ty_reaches(*f, inner, arr_layouts, struct_layouts, &mut seen))
                {
                    widen_struct_fields_in_place(
                        inner,
                        &fkey,
                        table,
                        arr_layouts,
                        struct_layouts,
                        fn_sigs,
                        visiting,
                    );
                } else {
                    *fty = widen_struct_fields(
                        *fty,
                        &fkey,
                        table,
                        arr_layouts,
                        struct_layouts,
                        fn_sigs,
                    );
                }
            }
            Type::FnSig(_) | Type::Closure(_) => {
                *fty = widen_fn_sig_by_key(*fty, &fkey, table, arr_layouts, fn_sigs)
            }
            _ => {}
        }
    }
    struct_layouts[sid.0 as usize] = widened;
}

/// Widen a parsed / inferred array type's element faces per the
/// width table. `key` is the slot (or literal Anon origin) holding
/// the container. With an annotation, the `number` elem gate keeps
/// explicit `: i64[]` narrow (mirroring the scalar W1 consumer
/// gate); without one (un-annotated `let a = [1, 2]`, literal
/// origins) an I64 elem can only have come from the number-domain
/// inference, so it is widenable by construction.
pub(crate) fn widen_arr_elem(
    parsed: Type,
    ann: Option<&str>,
    key: &SlotKey,
    table: &WidthTable,
    arr_layouts: &mut Vec<Type>,
) -> Type {
    let Type::Arr(id) = parsed else {
        return parsed;
    };
    // W-ESC (RFC 20260706-typed-arr-any-escape) — a container class
    // that flows into an `any`-annotated slot re-interns with an Any
    // element: the whole access surface rides the NaN-box any tier,
    // so an any-side element-kind transition (`u[0] = "s"`) stays
    // visible to every alias. Checked before the width faces — Any
    // is the lattice top. Skips already-Any layouts (idempotent).
    if !matches!(arr_layouts[id.0 as usize], Type::Any) && table.slot_escapes_any(key) {
        return Type::Arr(intern_arr_layout(arr_layouts, Type::Any));
    }
    // RFC 20260726-array-elem-width knife 2 — the only question this
    // gate has to answer is whether the element face is the user's
    // explicit `i64` narrow spelling. It used to conflate that with
    // "can I parse this annotation string", and answered an unparsable
    // spelling as if it were the narrow one: `Nums` (alias),
    // `Array<number>` and `__nullable(number[])` all refused to widen
    // while the analysis had already joined them into one class with
    // slots that do widen. Writes hit the loud `f64 value into i64
    // array elem` refusal (four spellings bun runs fine); reads have no
    // such guard and reinterpreted the bits.
    //
    // An unrecognized spelling asks the table instead. The table is the
    // source of truth for this and the annotation is one of its inputs,
    // so every future spelling is right for free. `None` already means
    // "unannotated, widenable by construction" downstream, which is the
    // reading we want. Explicit `i64[]` still parses to `Some("i64")`
    // and stays narrow; `type Nums = i64[]` now widens instead of
    // refusing, but only when a fractional value actually reaches the
    // class — writing one into an `i64[]` is a self-contradictory
    // request, and widening matches what bun does with it.
    let elem_ann = match ann {
        Some(a) => a.strip_suffix("[]"),
        None => None,
    };
    let number_elem = matches!(elem_ann, Some("number") | None);
    let elem = arr_layouts[id.0 as usize];
    let widened_elem = match elem {
        Type::I64 if number_elem && table.elem_is_f64(key) => Type::F64,
        Type::Arr(_) => {
            let elem_key = SlotKey::Elem(Box::new(key.clone()));
            widen_arr_elem(elem, elem_ann, &elem_key, table, arr_layouts)
        }
        _ => elem,
    };
    if widened_elem == elem {
        return parsed;
    }
    Type::Arr(intern_arr_layout(arr_layouts, widened_elem))
}
