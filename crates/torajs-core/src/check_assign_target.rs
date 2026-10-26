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
//!        Return the assigned-to type (no rhs consume since chunk
//!        566 — member stores share, mirroring chunk 564's
//!        assign-ident contract).
//! - **`arr[i] = value` (Index-target)** — M1.4: obj must be
//!   `Array<T>`; index must be Number; value type must match elem.
//!   `Array<Any>[i] = <concrete>` allowed per TS spec (box at
//!   ssa-lower via `__torajs_arr_set_any`).
//! - **fallback** — `Err("invalid assignment target")`.
//!
//! Caller dispatch: `check_assign_target` matches on the inner
//! Expr variant and routes to the right helper.

use crate::ast::{Ast, Expr, ExprId, PropKey};
use crate::check::{Checker, Type, resolve_class_ref};
use crate::check_assignable::is_assignable_to_resolved;
use crate::check_stmt_let_decl_destr_lane::pick_ary_destr_lane;

mod const_lifted;
mod index_target;
mod setter;
pub(crate) use const_lifted::{is_const_lifted_field, reject_const_lifted_update};
pub(crate) use index_target::check_index;
pub(crate) use setter::setter_param;
use setter::{try_accessor_setter, try_objlit_setter};

pub(crate) fn check_member(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    obj: ExprId,
    field: String,
    value: ExprId,
    bracket_spelled: bool,
) -> Result<Type, String> {
    // RFC 20260715-nominal-class-identity — a class instance's type is
    // its NAME (`ClassRef(C)`); every structural step below needs the
    // shape behind it.
    let obj_ty_nominal =
        crate::check_type_of_member_nullable_recv::decay_for_write(checker.type_of(ast, obj)?);
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
    if is_const_lifted_field(ast, &obj_ty_nominal, &field)
        && !ast.lifted_init_assigns.contains(&eid)
    {
        return Err(format!("cannot assign to const `{field}`"));
    }
    // Chunk 566 — member assignment does NOT consume the rhs:
    // ssa_lower's member-store lanes now share (slot/bucket +1,
    // source keeps its stake), mirroring the chunk-564 assign-ident
    // contract. The historical consume both leaked the source's
    // stake (props lanes) and loudly rejected legal alias-rhs
    // writes.
    if matches!(*obj_ty, Type::Any | Type::Function(..) | Type::Array(_)) {
        checker.check_dict_write(ast, obj, Some(&field), value)?;
        crate::check_stmt_let_decl::held_any::face_class::check_store(
            checker, ast, obj, &field, value,
        )?;
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
    // A BRACKET-SPELLED literal key on an OBJECT receiver
    // (`m["a"] = 9`) — see `Ast::index_spelled_props`. §10.1.9.2
    // OrdinarySet puts it in that cell's own bag, which the
    // member-store ladder's catch-all arm has done for every one of
    // these shapes since rotation 527's write-side twin: the same key
    // spelled dynamically (`m[kd] = 9`) has landed there all along,
    // and so has the `any` spelling. Only the literal was refused,
    // and only by the line below.
    //
    // The list is exactly the shapes whose `any` spelling already
    // stores — asked one by one rather than assumed from the read
    // side's. WeakMap / WeakSet / WeakRef are NOT here: the kernel
    // answers `(wm as any)["z"] = 9` with a TypeError too, so
    // admitting them would only move the compile error to run time,
    // which is what rotation 608 did on the read side. They sit on
    // the same kernel boundary as `m[0] = 9`, and are registered
    // there rather than admitted here.
    //
    // PRIMITIVE receivers stay out for a different reason. `s["a"]
    // = 9` is a strict-mode TypeError in every engine; answering it
    // with a silent success into a Str's bag would trade a
    // compile-time refusal for a wrong answer at run time, which the
    // design principles rank strictly worse than the refusal. A
    // DOT-spelled write stays out too — `m.a = 9` is an error in TS,
    // and that is the posture the read half keeps as well.
    if bracket_spelled
        && matches!(
            &*obj_ty,
            Type::Map(..)
                | Type::Set
                | Type::MapIter(..)
                | Type::ArrIter
                | Type::RegExp
                | Type::Date
        )
    {
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
    let value_ty =
        crate::check_assignable::in_declared_order(checker, ast, &field_ty, value, value_ty);
    // RFC 20260714-dstr-residual blade 3, field-store face (rotation
    // 455) — a generator lift turns a destructure group temp's
    // `LetDecl` into `this.<temp> = init` before check runs, so the
    // iterator-lane verdict has to be asked here too: a non-indexable
    // source (a custom iterable behind `any`, a Set, a generator)
    // must be STEPPED per §13.15.5.3, not indexed. Recording is the
    // whole job — the lifted field is already `any`, and the lowerer's
    // field-store walk boxes the stepped Array<Any> into it.
    let _ = pick_ary_destr_lane(checker, ast, None, value, &value_ty);
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
    crate::check::face_slots::record_field_store(
        checker,
        ast,
        &obj_ty_nominal,
        &field,
        &field_ty,
        &value_ty,
    );
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
            .contains_key(&(cname.clone(), PropKey::from(field)));
        // §15.7 puts every PrivateName a class body declares into the
        // scope of that whole body, nested classes included. So a
        // reference that RESOLVES is legal code, and whether `this`
        // carries the brand is §7.3.32's question at run time —
        // PrivateFieldSet step 4 throws a catchable TypeError. Only an
        // unresolvable name is §13.1's early error.
        //
        // The read path has consulted `declared_private_names` for a
        // while (`check_type_of_member_terminal_miss`, `check/type_of`);
        // the write path never did, so `this.#x = 1` inside a class
        // nested in the one that declares `#x` was refused outright.
        // Not a difference of posture — one path just never took the
        // hop the other takes.
        let declared = checker.declared_privates.contains(field);
        if !(is_method || is_getter || declared) {
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
        .contains(&(cls.to_string(), PropKey::from(field)))
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
