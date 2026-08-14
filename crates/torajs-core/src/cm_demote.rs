//! Speculative class-method rewrite demotion.
//!
//! `desugar_classes` rewrites `x.m(a)` into `__cm_<C>__m(x, a)`
//! (single owner) or `__dispatch_m(x, a)` (inheritance chain) purely
//! by method NAME — no type info exists pre-check. When `x` later
//! checks as a builtin container (Map / Set / Array / ...), that
//! rewrite was wrong: the call must dispatch to the builtin method,
//! not the user class's (`m.get(1)` on a `Map` must never become
//! `__cm_Holder__get(m, 1)` just because some class declares `get`).
//!
//! Mechanism, three hops with one decision point:
//!   1. desugar (`record_speculative_rewrite`) — before overwriting
//!      the call node, clone the ORIGINAL member-call shape into a
//!      fresh arena node (the `Expr::Member` callee node is left
//!      intact by the rewrite) and record `call → alt` in
//!      `Ast::speculative_cm_rewrites`. `this` / `__this` receivers
//!      are skipped — their type is always the enclosing class.
//!   2. check (`Checker::try_demote_cm_rewrite`) — at the Call arm,
//!      if the call is recorded and the receiver's checked type is a
//!      builtin container, typecheck the alt node instead (the full
//!      builtin member table applies) and record the demotion in
//!      `Checker::demoted_cm_rewrites`. This is the ONLY decision
//!      point; everything downstream consumes it.
//!   3. ssa_lower — restores the member-call shape in its owned AST
//!      at exactly the demoted ExprIds (before monomorphization, so
//!      cloned generic bodies inherit the restored shape), then
//!      lowering and num_width hit the regular builtin arms with no
//!      special-casing. num_width additionally bypasses its
//!      `any_class_owns_method("get")` name gate for demoted sites —
//!      the demotion is typed receiver evidence, stronger than the
//!      name guess the gate defends against.

use crate::ast::{Ast, ClassCtor, ClassMethod, Expr, ExprId, StaticInit};
use crate::check::{Checker, Type};

/// Record the original member-call shape of a name-based class-method
/// rewrite (hop 1 above). Called by `desugar_classes` just before it
/// overwrites `ast.exprs[call_idx]` with the mangled-ident call. The
/// alt node's callee still points at the intact `Expr::Member` node;
/// its args are the original user args (receiver not prepended).
pub(crate) fn record_speculative_rewrite(
    ast: &mut Ast,
    call_idx: usize,
    callee_id: ExprId,
    obj_id: ExprId,
    args: &[ExprId],
) {
    let recv_is_this = matches!(&ast.exprs[obj_id.0 as usize], Expr::This)
        || matches!(&ast.exprs[obj_id.0 as usize], Expr::Ident(n) if n == "__this");
    if recv_is_this {
        // No demotion decision exists for a `this` receiver, but the
        // intact Member callee still matters: the cmany twin mint
        // restores the member-call shape from it inside a cloned
        // any-receiver body (RFC 20260804 blade 2). The super-call
        // rewrite produces the same `__cm_` call shape WITHOUT an
        // entry here — that absence is how the mint tells a
        // (dynamic-by-spec) method call from a (static-by-spec)
        // super call.
        ast.cm_this_static_calls
            .insert(ExprId(call_idx as u32), callee_id);
        return;
    }
    let alt = ast.add_expr(Expr::Call {
        callee: callee_id,
        args: args.to_vec(),
    });
    ast.speculative_cm_rewrites
        .insert(ExprId(call_idx as u32), alt);
}

/// Builtin container / primitive types whose method calls must never
/// dispatch to a same-named user-class method. Primitives (Number /
/// Boolean / BigInt) were previously omitted on the "no false-reject
/// evidence" assumption; test262 evidence (rotation 106 probe:
/// `class X { toString() { … } }` then `(3).toString()` /
/// `"hi".toString()` / `true.toString()`) shows the same shape does
/// silent-reject on them — the speculative `__cm_X__toString` rewrite
/// requires `arg 0: ClassRef("X")` and rejects the primitive receiver
/// at the checker's arg-admit gate. Demoting sends the call back
/// through the primitive method surface (`Number.prototype.toString`,
/// `String.prototype.toString`, `Boolean.prototype.toString`) which
/// the interception arms in `ssa_lower_call_universal_methods` are
/// already wired for.
fn is_builtin_container_ty(t: &Type) -> bool {
    matches!(
        t,
        Type::Map
            | Type::Set
            | Type::WeakMap
            | Type::WeakSet
            | Type::Array(_)
            | Type::String
            | Type::Promise(_)
            | Type::MapIter
            | Type::ArrIter
            | Type::RegExp
            | Type::Date
            | Type::Number
            | Type::Boolean
            | Type::BigInt
    )
}

impl Checker {
    /// Demotion decision point (hop 2 above). Returns `Some(result)`
    /// when the call demotes — the caller returns it verbatim —
    /// `None` when the speculative rewrite stands. The receiver probe
    /// is restricted to Ident / Member shapes: their `type_of` is a
    /// pure lookup, so the extra probe call can't double-count any
    /// affine bookkeeping. A receiver whose `type_of` errors never
    /// demotes (the original path surfaces its own error).
    pub(crate) fn try_demote_cm_rewrite(
        &mut self,
        ast: &Ast,
        eid: ExprId,
        args: &[ExprId],
    ) -> Option<Result<Type, String>> {
        let &alt_id = ast.speculative_cm_rewrites.get(&eid)?;
        let &recv_eid = args.first()?;
        // Primitive literals (`(3).toString()` / `true.toString()` /
        // `10n.toString()` / `"hi".toString()`) are pure-typed like
        // Ident / Member — their `type_of` is a self-lookup with no
        // affine bookkeeping, so the probe is safe. Pre-fix these were
        // rejected here and never demoted, so the speculative
        // `__cm_<C>__toString` (name-owned by some user class) survived
        // and the checker's arg-admit gate rejected the primitive
        // receiver at "expected ClassRef(...), got Number|Boolean|
        // BigInt|String".
        // Call receivers (S2.34): `ref(42).next()` / `new C().m(…).next()`
        // — the receiver is a call RESULT, typed Any whenever the callee
        // is a fn value read off a prototype / accessor. The speculative
        // `__cm___Gen_*__next(recv)` rewrite was a guaranteed reject
        // ("expected ClassRef(__Gen_…), got Any"). Probing a Call's
        // type_of is double-probe-safe: every mutation on the call-check
        // path keys on the ExprId and re-inserts the same value
        // (expr_types / generic_call_instantiations / arity_pad_count /
        // demoted_cm_rewrites / t28 pad) — the consuming-params move
        // bitmap retired in chunk 568, so no affine state double-counts.
        // As receivers (刀 5b follow-up to S2.34): `(f() as any).next()`
        // — the cast is a pure type override over an expression the
        // Call arm already probes safely; without it the speculative
        // `__cm___Gen_*__next(recv)` rewrite survived and rejected at
        // "expected ClassRef(__Gen_…), got Any".
        // Runtime-construct receivers (rotation 394): `new K().m()`
        // where `K` holds a value rather than naming a class. The
        // result is `Any` by construction (§7.2.4 IsConstructor is a
        // run-time question), so it demotes — and it MUST, or the
        // speculative `__cm_<C>__m(recv)` survives and the call
        // silently answers some unrelated class's method body just
        // because that class is the only one owning the name. The
        // probe walks the same sub-expressions an `Expr::Call`
        // receiver does, under the same double-probe reasoning.
        // `Expr::New` stays out: it types as `ClassRef`, which never
        // demotes, and probing it would re-walk constructor arguments
        // for no decision.
        if !matches!(
            ast.get_expr(recv_eid),
            Expr::Ident(_)
                | Expr::Member { .. }
                | Expr::Call { .. }
                | Expr::NewDynamic { .. }
                | Expr::As { .. }
                | Expr::Number(_)
                | Expr::String(_)
                | Expr::Bool(_)
                | Expr::BigInt { .. }
        ) {
            return None;
        }
        let Ok(recv_ty) = self.type_of(ast, recv_eid) else {
            return None;
        };
        // 刀 4 (RFC 20260714-t262-top-clusters) — an Any receiver
        // demotes too: the member-call shape routes through
        // route_early's any-member arm into the runtime dispatcher,
        // which resolves REAL class methods through the class-methods
        // table (struct_method) — the static `__cm_<C>__m(any_recv)`
        // form was a guaranteed checker reject ("expected Struct,
        // got Any").
        // RFC 20260715-nominal-class-identity — the speculative rewrite
        // is keyed on the METHOD NAME alone (desugar runs before any
        // type is known), so `plain.m()` on a plain `{a: 1}` became
        // `__cm_C__m(plain)` whenever some class C owned an `m` — and
        // the struct-prefix subtype rule then admitted the receiver,
        // answering C's method body. A bare `Struct` is an object
        // literal or a `type P = {...}` alias: it is an instance of NO
        // class, however closely its shape matches one. Demote, and the
        // member checker rejects the call loudly.
        //
        // Exemption (rotation 179): "a class instance types as
        // `ClassRef`" holds for NON-generic classes only — a generic
        // class instantiation resolves structurally (check_type_ann
        // generic.rs substitutes `C<args>` field-by-field into a
        // `Type::Struct`; ClassRef is minted only for the recursive
        // back-edge). A Struct receiver whose field-name sequence
        // matches a generic CLASS declaration is that class's instance
        // shape — demoting it rejects every method call on generic
        // class instances. A plain literal that spells out a generic
        // class's exact field list still slips through to the
        // struct-prefix rule (narrow known face; the real fix is
        // nominal identity for generic instantiations, recorded L3b).
        let steals_by_shape = match &recv_ty {
            Type::Struct(fields) => !self.struct_is_generic_class_shape(ast, fields),
            _ => false,
        };
        if !is_builtin_container_ty(&recv_ty) && !matches!(recv_ty, Type::Any) && !steals_by_shape {
            return None;
        }
        self.demoted_cm_rewrites.insert(eid, alt_id);
        Some(self.type_of(ast, alt_id))
    }

    /// True iff the receiver's field-name sequence equals some generic
    /// CLASS declaration's field list (`generic_alias_decls` holds both
    /// generic classes and generic `type` aliases; `ast.class_parents`
    /// keeps only real classes). Field TYPES are ignored — each
    /// instantiation substitutes its own args. Order-sensitive by
    /// construction: the factory builds instances in declaration order.
    fn struct_is_generic_class_shape(&self, ast: &Ast, fields: &[(String, Type)]) -> bool {
        self.generic_alias_decls
            .iter()
            .any(|(name, (_tps, decl_fields))| {
                ast.class_parents.contains_key(name)
                    && decl_fields.len() == fields.len()
                    && decl_fields
                        .iter()
                        .zip(fields)
                        .all(|((dn, _), (rn, _))| dn == rn)
            })
    }
}

/// True iff the call receiver is `this.<field>` AND the named
/// field on class `cname` has a builtin (Array / Str / Number)
/// type annotation. Used by desugar_classes' single-owner rewrite
/// guard so `this.data.push(v)` (where `data: T[]`) doesn't get
/// rewritten as a self-recursive class-method call.
///
/// `class_index` is the snapshot built at the top of desugar_classes
/// — `(usize, name, type_params, parent, fields, ctor, methods)`.
#[allow(clippy::type_complexity)]
pub(crate) fn receiver_is_this_builtin_field(
    ast: &Ast,
    obj_id: ExprId,
    cname: &str,
    class_index: &[(
        usize,
        String,
        Vec<String>,
        Option<String>,
        Vec<(String, String)>,
        Vec<StaticInit>,
        Option<ClassCtor>,
        Vec<ClassMethod>,
        Vec<ClassMethod>,
    )],
) -> bool {
    let Expr::Member {
        obj: inner_obj,
        name: field_name,
    } = &ast.exprs[obj_id.0 as usize]
    else {
        return false;
    };
    // The This → Ident("__this") rewrite in this same desugar pass
    // may already have fired for low-ExprId nodes by the time we
    // inspect this call (Pass 2 walks 0..n). Accept either shape.
    let inner_is_this = match &ast.exprs[inner_obj.0 as usize] {
        Expr::This => true,
        Expr::Ident(n) if n == "__this" => true,
        _ => false,
    };
    if !inner_is_this {
        return false;
    }
    // Find the class entry and look up the field's type annotation.
    let cls = class_index.iter().find(|(_, n, ..)| n == cname);
    let Some((_, _, _, _, fields, _, _, _, _)) = cls else {
        return false;
    };
    let field_ty_ann = fields
        .iter()
        .find(|(fn_, _)| fn_ == field_name)
        .map(|(_, ann)| ann.as_str());
    let Some(ann) = field_ty_ann else {
        return false;
    };
    // Builtin: Array (`T[]`), `string`, or `number`. These dispatch
    // to runtime intrinsics, not user class methods.
    ann.ends_with("[]") || ann == "string" || ann == "number"
}
