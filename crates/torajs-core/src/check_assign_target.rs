//! `Expr::Assign { target: Expr::Member | Expr::Index, value }`
//! typecheck sub-arms pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Assign` arm
//! as chunk-94 of the type_of_inner decomp.
//!
//! Two sub-paths bundled (matches the dispatch shape of the inner
//! match on the target's `Expr` variant):
//!
//! - **`obj.field = value` (Member-target)** — M1.4 / M-OO.5 / P3.2
//!   / T-27 / T-29 / P9.4 / P8.2 / V3-05 / V3-06 dispatch cascade:
//!   1. **M-OO.5 readonly enforcement** — `ast.readonly_fields`
//!      lookup; reject writes from outside the same class (TS
//!      restricts to constructor only; we approximate with same-
//!      class). Visibility (Private/Protected) was enforced
//!      already by the receiver `type_of(obj)` read-path traversal.
//!   2. **Type::Any** (P3.2) — dynobj_set; accept any value.
//!   3. **Type::Function** (T-27) — closure props_dynobj; accept
//!      any value.
//!   4. **Type::Array** (T-29) — arrprops side-table; accept any
//!      value.
//!   5. **Type::RegExp + "lastIndex"** (P9.4) — accept Number RHS.
//!   6. **Type::Struct** receiver:
//!      - **Accessor setter** (P8.2) — `accessor_setters[(cls, field)]`
//!        lookup; validate RHS against the setter's value-param
//!        type (`__this` first, then user-declared param).
//!      - **V3-06 empty array literal** — `this.kids = []` in ctor:
//!        bare empty literal gets element type from declared field
//!        array type.
//!      - **Direct struct field** (V3-05) — `is_assignable_to_resolved`
//!        on field_ty vs value_ty.
//!      Return the assigned-to type (no rhs consume since chunk
//!      566 — member stores share, mirroring chunk 564's
//!      assign-ident contract).
//! - **`arr[i] = value` (Index-target)** — M1.4: obj must be
//!   `Array<T>`; index must be Number; value type must match elem.
//!   `Array<Any>[i] = <concrete>` allowed per TS spec (box at
//!   ssa-lower via `__torajs_arr_set_any`).
//! - **fallback** — `Err("invalid assignment target")`.
//!
//! Caller dispatch: `check_assign_target` matches on the inner
//! Expr variant and routes to the right helper.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::resolve_class_ref;
use crate::check::{Checker, Type};
use crate::check_assignable::is_assignable_to_resolved;

mod setter;
use setter::{try_accessor_setter, try_objlit_setter};

pub(crate) fn check_member(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    obj: ExprId,
    field: String,
    value: ExprId,
) -> Result<Type, String> {
    // RFC 20260715-nominal-class-identity — a class instance's type is
    // its NAME (`ClassRef(C)`); every structural step below needs the
    // shape behind it.
    let obj_ty_nominal = checker.type_of(ast, obj)?;
    // Cow flavor — the common already-resolved Struct receiver
    // borrows instead of deep-cloning its whole field list per
    // assignment (rotation 268: O(N²) clone stall on wide structs).
    let obj_ty = crate::check_resolve_class_ref::resolve_class_ref_cow(
        &obj_ty_nominal,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    enforce_readonly(checker, ast, obj, &field)?;
    // Chunk 566 — member assignment does NOT consume the rhs:
    // ssa_lower's member-store lanes now share (slot/bucket +1,
    // source keeps its stake), mirroring the chunk-564 assign-ident
    // contract. The historical consume both leaked the source's
    // stake (props lanes) and loudly rejected legal alias-rhs
    // writes.
    if matches!(*obj_ty, Type::Any | Type::Function(..) | Type::Array(_)) {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    // A namespace with a runtime singleton (RFC 20260801
    // ns-object-value) is a REAL dynobj — `Math.length = 1` is an
    // ordinary expando write on it, so the store rides the any
    // lanes like every other dynobj write. Namespaces without a
    // minted singleton keep the loud reject (admitting them would
    // only move the same failure into the lowerer).
    if matches!(*obj_ty, Type::Object("Math" | "JSON" | "Reflect")) {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    // §27.2.4 static-slot patch (rotation 448) — `Promise.resolve =
    // fn` / `Promise.reject = fn` rides the any lanes into the
    // interned ctor cell's expando dict, the one store the static
    // call sites' patch consult reads back
    // (`ssa_lower_call_promise_resolve`). Only the two CONSULTED
    // names are admitted: an accepted write nobody reads back is a
    // silent-wrong faucet (`Promise.all = fn` would still run the
    // builtin), so every other name keeps the loud reject.
    if matches!(*obj_ty, Type::Object("Promise")) && matches!(field.as_str(), "resolve" | "reject")
    {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    if matches!(*obj_ty, Type::RegExp) && field == "lastIndex" {
        let value_ty = checker.type_of(ast, value)?;
        if !matches!(value_ty, Type::Number) {
            return Err(format!(
                "type mismatch assigning to `RegExp.lastIndex`: expected number, got {value_ty:?}"
            ));
        }
        return Ok(Type::Number);
    }
    // EXPANDO on an ECMAScript global object (`Array.myproperty = 1`)
    // → Any: ordinary extensible objects, so the write lands in the
    // singleton's own-entry dict and the matching read comes back out
    // of it via the any-member lane. Restricted to names tr does NOT
    // model, for the same reason the Promise arm above admits exactly
    // two — see `check_type_of_member_global_miss`.
    if crate::check_type_of_member_global_miss::expando_write_admitted(&obj_ty, &field) {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    let Type::Struct(fields) = &*obj_ty else {
        return Err(format!(
            "field assignment target must be a struct, got {obj_ty:?}"
        ));
    };
    if let Some(t) = try_accessor_setter(checker, ast, eid, &obj_ty_nominal, &field, value)? {
        return Ok(t);
    }
    if let Some(t) = try_objlit_setter(checker, ast, fields, &field, value)? {
        return Ok(t);
    }
    let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == &field) else {
        if let Some(t) = try_class_dynamic_write(checker, ast, &obj_ty_nominal, &field, value)? {
            return Ok(t);
        }
        return Err(format!("no field `{field}` on type {obj_ty:?}"));
    };
    let field_ty = field_ty.clone();
    // V3-06 — `this.kids = []` in a class constructor: bare empty
    // array literal gets its element type from the field's declared
    // array type.
    if matches!(ast.get_expr(value), Expr::Array(els) if els.is_empty())
        && matches!(
            resolve_class_ref(
                &field_ty,
                &checker.class_structs,
                &checker.aliases,
                &checker.generic_alias_decls
            ),
            Type::Array(_)
        )
    {
        return Ok(field_ty);
    }
    let value_ty = checker.type_of(ast, value)?;
    // RFC 20260714-dstr-residual blade 3, field-store face (rotation
    // 455) — a generator lift turns a destructure group temp's
    // `LetDecl` into `this.<temp> = init` before check runs, so the
    // iterator-lane verdict has to be asked here too: a non-indexable
    // source (a custom iterable behind `any`, a Set, a generator)
    // must be STEPPED per §13.15.5.3, not indexed. Recording is the
    // whole job — the lifted field is already `any`, and the lowerer's
    // field-store walk boxes the stepped Array<Any> into it.
    let _ = crate::check_stmt_let_decl::pick_ary_destr_lane(checker, ast, value, &value_ty);
    if !is_assignable_to_resolved(
        &field_ty,
        &value_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return Err(format!(
            "type mismatch assigning to `{field}`: field is {field_ty:?}, value is {value_ty:?}"
        ));
    }
    Ok(field_ty)
}

/// Rotation 373 — the write mirror of the read side's class-instance
/// terminal-miss admission (RFC 20260804 blade 4): §10.1.9.2
/// OrdinarySet on a name the layout never carries is an expando
/// definition, not a type error — `this._v = v` inside an untyped
/// class body is ordinary JS and bun runs it. The lowering boxes the
/// receiver and rides the any-member set lane, whose runtime tail
/// (RFC 20260714-struct-dynamic-props blade 2) owns the +24 expando
/// dict, the non-extensible gate, and the accessor dispatch (a
/// getter-only property throws the §10.1.9 readonly TypeError there).
///
/// A `__priv_` mangled name is narrower (§13.15.2 PutValue →
/// PrivateSet): a declared private METHOD or getter-only accessor
/// admits — the write must throw a CATCHABLE TypeError at runtime
/// (compound assignment reads first, computes, then dies at PutValue),
/// supplied by the lowering's static-throw arm (method) / the runtime
/// accessor kernel (getter-only). An UNDECLARED private name keeps the
/// loud reject: §13.1 requires lexical resolution, never a runtime
/// answer.
///
/// A name the class chain declares as a METHOD keeps the loud reject:
/// method call sites dispatch statically (`__cm_<C>__<name>` direct
/// call), so an expando shadow the runtime would honor is invisible to
/// them — admitting the write would be a silent-wrong, not a feature.
/// Anonymous Struct receivers keep the loud reject too: an object
/// literal's fields are statically known, so a miss there is
/// overwhelmingly a typo (the read side's recorded diagnostic-posture
/// boundary).
fn try_class_dynamic_write(
    checker: &mut Checker,
    ast: &Ast,
    obj_ty_nominal: &Type,
    field: &str,
    value: ExprId,
) -> Result<Option<Type>, String> {
    let Some(cname) = crate::check_type_of_member_accessor::class_name_of(obj_ty_nominal, ast)
    else {
        return Ok(None);
    };
    if field.starts_with("__priv_") {
        let is_method = matches!(
            checker.globals.get(&format!("__cm_{cname}__{field}")),
            Some(Type::Function(..))
        );
        let is_getter = ast
            .accessor_getters
            .contains_key(&(cname.clone(), field.to_string()));
        if !(is_method || is_getter) {
            let raw = field
                .strip_prefix("__priv_")
                .and_then(|r| r.split_once("__").map(|(_, f)| f))
                .unwrap_or(field);
            return Err(format!(
                "private field `#{raw}` is not declared in this class"
            ));
        }
    } else if class_chain_declares_method(checker, ast, &cname, field) {
        return Ok(None);
    }
    let _ = checker.type_of(ast, value)?;
    Ok(Some(Type::Any))
}

/// Walk `cname`'s parent chain probing `__cm_<C>__<name>` — the shape
/// the read side's class-method arm resolves. Inherited methods count:
/// their call sites are just as statically dispatched.
fn class_chain_declares_method(checker: &Checker, ast: &Ast, cname: &str, field: &str) -> bool {
    let mut cur = Some(cname.to_string());
    while let Some(c) = cur {
        if matches!(
            checker.globals.get(&format!("__cm_{c}__{field}")),
            Some(Type::Function(..))
        ) {
            return true;
        }
        cur = ast.class_parents.get(&c).cloned().flatten();
    }
    false
}

fn enforce_readonly(checker: &Checker, ast: &Ast, obj: ExprId, field: &str) -> Result<(), String> {
    let obj_class: Option<String> = match ast.get_expr(obj) {
        Expr::This => checker.current_class.clone(),
        Expr::Ident(n) => checker.lookup(n).and_then(|info| info.declared_class),
        _ => None,
    };
    let Some(cls) = obj_class.as_deref() else {
        return Ok(());
    };
    if !ast
        .readonly_fields
        .contains(&(cls.to_string(), field.to_string()))
    {
        return Ok(());
    }
    if checker.current_class.as_deref() != Some(cls) {
        return Err(format!(
            "M-OO.5: cannot write readonly member `{cls}.{field}` from {}",
            checker
                .current_class
                .as_deref()
                .map(|c| format!("class `{c}`"))
                .unwrap_or_else(|| "outside any class".to_string())
        ));
    }
    Ok(())
}

pub(crate) fn check_index(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    obj: ExprId,
    index: ExprId,
    value: ExprId,
) -> Result<Type, String> {
    // Cluster #6 (rotation 441) — the read side resolves a nominal
    // class instance to its field struct before dispatching
    // (`check_type_of_index` since RFC 20260715); the write side
    // never did, so every keyed WRITE on a `new C()` receiver fell
    // past all the struct arms into the array reject — the
    // cpn-class-expr-accessors family's `c[1 + 1] = 2`.
    let raw_obj_ty = checker.type_of(ast, obj)?;
    let obj_ty = crate::check::resolve_class_ref(
        &raw_obj_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    // Chunk 745 — struct receiver + compile-time literal index:
    // `g[0] = v` ≡ `g."0" = v` per ES ToPropertyKey (§7.1.19);
    // delegate to the member-assignment checker (field lookup /
    // readonly / setter / assignability). Mirrors the read-side
    // lane in `check_type_of_index`.
    if matches!(obj_ty, Type::Struct(_))
        && let Some(name) = crate::ast::literal_prop_key(ast, index)
    {
        return check_member(checker, ast, eid, obj, name, value);
    }
    let idx_ty = checker.type_of(ast, index)?;
    // L3b #13 — an `any` receiver admits string keys (ES
    // ToPropertyKey); every other receiver keeps the number-only
    // spec narrow. Symbol keys ride the same receiver: §6.1.7 makes
    // them the other half of the property-key domain, and §7.1.19
    // step 2 hands them to the store uncoerced (mirror of the
    // read-side gate in `check_type_of_index`).
    // Cluster #1 blade 4 — an `any` key on an `any` receiver rides
    // the keyed set kernel's runtime ToPropertyKey dispatch (mirror
    // of the read-side admit in `check_type_of_index`).
    // Cluster #4 (rotation 438) — a STRUCT-typed key joins: §7.1.19
    // ToPropertyKey coerces an object key through toString/valueOf at
    // runtime, so the lowering boxes it and rides the keyed set
    // kernel (read-side mirror in `check_type_of_index`).
    if idx_ty != Type::Number
        && !(matches!(obj_ty, Type::Any)
            && matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined | Type::Struct(_)
            ))
        // Rotation 346 — a STRUCT receiver under an UNDEFINED key:
        // §7.1.19 ToPropertyKey(undefined) is the fixed string key
        // "undefined". Cluster #3 (rotation 442) — STRING / SYMBOL /
        // ANY keys join, completing the read-side mirror (the read
        // gate has admitted this exact domain since chunk 753 / L3b
        // #13): the keyed set kernel's member_set struct arm
        // dispatches layout field / inherited accessor / expando
        // (rotation 441 blade 3c), so the shadow-split concern that
        // once kept dynamic string keys out no longer holds — the
        // cpn-class accessor family's `c[String(1 + 1)] = 2`.
        && !(matches!(obj_ty, Type::Struct(_))
            && matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined | Type::Struct(_)
            ))
        // An ARRAY receiver admits an `any` key: §7.1.19 decides
        // element-vs-property from the runtime value, so the write
        // rides the keyed set kernel with the receiver boxed (the
        // read-side Array arm's write mirror — S12.6.3_A5's
        // `var x = 0; a[x] = v`, where a `var` binding reads as
        // Any). String / Symbol / Undefined keys join (cluster #3,
        // rotation 442), completing the read-side mirror: the keyed
        // set kernel spells the key per §7.1.19 and routes a
        // canonical array-index spelling ("0") to the ELEMENT store
        // (length update included), so the shadow-split concern that
        // kept string keys out no longer holds — S15.4_A1.1_T4's
        // `x["0"] = 0`.
        && !(matches!(obj_ty, Type::Array(_))
            && matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined
            ))
    {
        return Err(format!("index must be number, got {idx_ty:?}"));
    }
    if matches!(obj_ty, Type::Array(_))
        && matches!(
            idx_ty,
            Type::String | Type::Symbol | Type::Any | Type::Undefined
        )
    {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    // Any-dynamic-access RFC (20260704) S3-set — TS `any` admits
    // every index write; runtime dispatch (kind-aware Arr / silent
    // primitive no-op / explicit roadmap TypeError) happens in
    // `__torajs_any_index_set`. The value still typechecks for its
    // own side effects / diagnostics.
    if matches!(obj_ty, Type::Any) {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    // Rotation 346 — the struct + undefined-key pair the number
    // gate above admitted: the lowering boxes the struct receiver
    // and rides the keyed set kernel. String / Symbol / Any keys
    // ride the same box (cluster #3, rotation 442 — see the gate
    // note above); field types are heterogeneous so the static
    // answer is Any, the read side's mirror.
    if matches!(obj_ty, Type::Struct(_))
        && matches!(
            idx_ty,
            Type::String | Type::Symbol | Type::Any | Type::Undefined | Type::Struct(_)
        )
    {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    // Cluster #6 (rotation 441) — a STRUCT receiver under a DYNAMIC
    // number key: §7.1.19 spells it in decimal at runtime, and the
    // keyed set kernel dispatches layout field / accessor / expando
    // exactly as the literal lane's member store would (the read
    // side has admitted this pair since chunk 753 — `Struct(_) =>
    // Ok(Any)` — so the write half was the asymmetry). The lowering
    // boxes the receiver and rides the keyed kernel.
    if matches!(obj_ty, Type::Struct(_)) && idx_ty == Type::Number {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    // A FUNCTION receiver — `h[0] = 11` on a fn value is the §7.1.19
    // stringified expando write member_set's closure arm owns (the
    // 405-01 residue closed the runtime in rotation 408; this admits
    // the TYPED spelling the any lane already reaches). The lowering
    // boxes the receiver and rides the any index-set kernel.
    if matches!(obj_ty, Type::Function(..))
        && matches!(
            idx_ty,
            Type::Number | Type::Any | Type::String | Type::Undefined
        )
    {
        let _ = checker.type_of(ast, value)?;
        return Ok(Type::Any);
    }
    let Type::Array(elem) = &obj_ty else {
        // The singleton-backed namespaces again (member arm above)
        // — `Math[0] = 1` is a keyed expando write on the dynobj.
        if matches!(obj_ty, Type::Object("Math" | "JSON" | "Reflect")) {
            let _ = checker.type_of(ast, value)?;
            return Ok(Type::Any);
        }
        return Err(format!(
            "index assignment target must be an array, got {obj_ty:?}"
        ));
    };
    let elem_ty = (**elem).clone();
    let value_ty = checker.type_of(ast, value)?;
    if !is_assignable_to_resolved(
        &elem_ty,
        &value_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return Err(format!(
            "type mismatch on element assignment: array of {elem_ty:?}, value is {value_ty:?}"
        ));
    }
    // Chunk 567 — element assignment shares the rhs (mirror of the
    // member-store contract above); no consume.
    Ok(elem_ty)
}
