//! Typed-receiver Member-access cluster for primitive / Symbol /
//! Arr / Map / Set / Closure / FnSig pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-64 of the decomp (chunks 1-63 = ... + RegExp accessor
//! cluster).
//!
//! Five sub-arms tried in source order:
//!
//! - **V3-18 m2.c** — `<prim>.constructor` returns `ConstPtrNull`
//!   for `Type::{I64|F64|I32|Bool|Str|Substr|BigInt|Symbol}`. tora
//!   doesn't have first-class function refs for namespaces; `typeof`
//!   on the result still routes through the typeof-Member path
//!   which returns `"function"`. `obj_val` is intentionally dropped
//!   (primitive borrows have no side effect to flush).
//! - **V3-18 m1.h.47** — `Symbol.prototype.description`. Returns the
//!   desc str the Symbol was created with (or null for `Symbol()`
//!   no-arg). Runtime helper bumps the desc's refcount so the caller
//!   can drop independently of the Symbol's lifetime.
//! - **Phase 2A** — `xs.length` on `Type::Arr(_)` reads u64 len at
//!   `ARR_LEN_OFF` of the array header.
//! - **P6.1 / P6.2** — `m.size` / `s.size` accessor property per ES
//!   §23.1.3.10 / §24.2.3.9. Both route through
//!   `__torajs_map_size` since Set storage shares the Map runtime.
//! - **T-27.c** — `f.length` / `f.name` for `Type::Closure(_)` /
//!   `Type::FnSig(_)`. `length` is a compile-time fold from the
//!   fn's static signature (param count; lifted-`__env` hidden Ptr
//!   excluded for Closure; FnSig signatures don't carry env so the
//!   raw param count is reported). `.name` (chunk 798) answers the
//!   runtime fn-addr registry — `__torajs_closure_name_str` for
//!   Closure cells (builtin-method / bound / registry chain),
//!   `__torajs_fn_name_str` for raw FnSig vaddrs — so aliases and
//!   field receivers read the registered name instead of the old
//!   call-site-ident approximation; registry miss = `""`.
//!
//! Returns `Some(op)` on hit; `None` on miss (receiver type +
//! Member name combo not in the allowlist). Caller falls through to
//! the generic Member path.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    obj: ExprId,
    obj_val: Operand,
    obj_ty: Type,
    name: &str,
) -> Option<Operand> {
    if name == "constructor"
        // RFC 20260808 knife 4 — an ARRAY receiver falls through to
        // the props-read kernel instead (`__torajs_arr_member_value`:
        // own expando first, then the Array.prototype face whose
        // `constructor` entry answers this same interned value).
        // Short-circuiting here skipped §10.1.8.1's own step, so
        // `a.constructor = {}` wrote a bag entry no read ever saw —
        // and the `a.constructor[Symbol.species] = …` that follows it
        // in the t262 create-species family landed its write on the
        // BUILTIN cell's expando instead of the user's object.
        && !matches!(obj_ty, Type::Arr(_))
        && let Some(tag) = ctor_proto_tag_of(&obj_ty)
    {
        // RFC 20260721 G3 — the receiver's builtin constructor as
        // the interned ctor VALUE (same identity the bare `Array` /
        // `String` ident read answers), so `xs.constructor` compared
        // through a generic slot holds. Was ConstPtrNull, which
        // only the AST-level `try_fold_constructor_eq` could rescue.
        let _ = obj_val;
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.builtin_ctor_value,
                vec![Operand::ConstI64(tag)],
            ),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    if obj_ty == Type::Symbol && name == "description" {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.symbol_description, vec![obj_val]),
            Type::Str,
            None,
        );
        return Some(Operand::Value(v));
    }
    if matches!(obj_ty, Type::Arr(_)) && name == "length" {
        // RC-4 F1a — a nullable-arr receiver (exec/match result)
        // may be null on miss; guard before the inline len load.
        crate::ssa_lower_nullable_guard::emit_nullable_arr_guard(ctx, obj, &obj_val);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Load(Type::I64, obj_val, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        return Some(Operand::Value(v));
    }
    if matches!(obj_ty, Type::Map | Type::Set) && name == "size" {
        // A Map / Set slot that can hold the generic undefined cell
        // (a read past the end of such an array, a `find` miss) must
        // throw here rather than hand the bare header to `map_size`,
        // which would read a bucket count out of whatever follows it.
        crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &obj_val);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.map_size, vec![obj_val]),
            Type::I64,
            None,
        );
        return Some(Operand::Value(v));
    }
    if (matches!(obj_ty, Type::Closure(_)) || matches!(obj_ty, Type::FnSig(_)))
        && (name == "length" || name == "name")
    {
        // RFC 20260722-find-miss chunk C — a Closure find/findLast
        // miss holds the undefined cell; `.length` / `.name` on it
        // must throw like bun before the static fold / cell probe
        // answers. No-op for plain receivers.
        crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &obj_val);
        // RFC 20260719-ns-static-value-reify — an ns-static value's
        // `.length` folds the shared table's spec length (`console
        // .log.length` is 0; the sig param count below would say 1).
        // `.name` stays on the runtime chain — the cell probe already
        // answers it.
        if name == "length"
            && let Some(id) = ns_static_id_of_obj(ctx, obj)
            && let Some(row) = torajs_rc::ns_static_meta(id)
        {
            return Some(Operand::ConstI64(i64::from(row.length)));
        }
        // RFC 20260725-str-method-value-reify — a reified builtin
        // method value's `.length` folds the spec meta row
        // (`indexOf.length` is 1; the checker sig says 2). `.name`
        // stays on the runtime chain — the cell probe answers it.
        if name == "length"
            && let Some((mid, fam)) = builtin_mv_mid_of_obj(ctx, obj)
            && let Some((_, arity)) = torajs_rc::any_method_meta_for(fam, mid)
        {
            return Some(Operand::ConstI64(i64::from(arity)));
        }
        return Some(lower_fn_length_or_name(ctx, obj, obj_val, obj_ty, name));
    }
    // RFC 20260721 刀 9 — `fun.prototype` on a Closure-typed
    // receiver: the runtime kernel materializes the §10.2.5 object
    // for a plain-fn cell (FLAG_FN_PROTO) and answers undefined for
    // arrows / async forms / builtin cells, so the lowering stays
    // flavor-blind. Owned Any out (the consumer drops it).
    if matches!(obj_ty, Type::Closure(_)) && name == "prototype" {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.closure_prototype_any, vec![obj_val]),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    // RFC 20260721 刀 4 — `fun.constructor` on a Closure-typed
    // receiver: flavor-keyed at runtime (async cell →
    // %AsyncFunction%, else %Function%; own expando shadows inside
    // the kernel). The interned cells are immortal, box is rc-free.
    if matches!(obj_ty, Type::Closure(_)) && name == "constructor" {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.closure_ctor_value, vec![obj_val]),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    None
}

/// The ns-static table id of a member READ receiver: a binding
/// registered at its let-decl (`const f = console.log; f.length`) or
/// the direct nested member form (`Math.max.length`).
fn ns_static_id_of_obj(ctx: &LowerCtx<'_>, obj: ExprId) -> Option<i64> {
    if let crate::ast::Expr::Ident(n) = ctx.ast.get_expr(obj)
        && let Some(id) = ctx.ns_static_locals.get(n)
    {
        return Some(*id);
    }
    crate::ssa_lower_stmt_let_decl_general::ns_static_member_init_id(ctx, obj)
}

/// The builtin `(method id, proto family)` of a member READ
/// receiver: a binding registered at its let-decl (`const m =
/// s.slice; m.length`) or the direct nested member form
/// (`s.slice.length`). The family rides along because the spec
/// `length` of one mid can differ per prototype.
fn builtin_mv_mid_of_obj(ctx: &LowerCtx<'_>, obj: ExprId) -> Option<(i64, i64)> {
    if let crate::ast::Expr::Ident(n) = ctx.ast.get_expr(obj)
        && let Some(mid_fam) = ctx.builtin_mv_locals.get(n)
    {
        return Some(*mid_fam);
    }
    crate::ssa_lower_stmt_let_decl_general::builtin_mv_member_init_mid(ctx, obj)
}

/// The builtin-proto tag whose interned ctor cell a typed
/// receiver's `.constructor` read answers (torajs-rc
/// `builtin_proto.rs` order). Struct instances stay `None` — their
/// constructor rides the class prototype chain.
fn ctor_proto_tag_of(obj_ty: &Type) -> Option<i64> {
    match obj_ty {
        Type::I64 | Type::F64 | Type::I32 => Some(0),
        Type::Arr(_) => Some(2),
        Type::Str | Type::Substr => Some(3),
        Type::Bool => Some(4),
        Type::Symbol => Some(5),
        Type::BigInt => Some(6),
        _ => None,
    }
}

/// `f.length` §10.2.5 via the AST when the receiver is a top-level
/// binding of a lifted fn value (`let f = function (…) {}` — the
/// init is an `Ident("__closure_N")` / `Closure{fn_name}` naming the
/// lifted FnDecl). Only the AST knows which params carry defaults;
/// the sig fallback over-counts defaulted tails.
fn try_ast_expected_count(ctx: &LowerCtx<'_>, obj: crate::ast::ExprId) -> Option<usize> {
    use crate::ast::{Expr, Stmt};
    let Expr::Ident(bind) = ctx.ast.get_expr(obj) else {
        return None;
    };
    let decl_name = ctx.ast.stmts.iter().find_map(|s| match s {
        Stmt::LetDecl { name, init, .. } if name == bind => match ctx.ast.get_expr(*init) {
            Expr::Ident(n) if n.starts_with("__closure_") || n.starts_with("__genexpr_") => {
                Some(n.clone())
            }
            Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
            _ => None,
        },
        _ => None,
    })?;
    let params = ctx.ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, params, .. } if *name == decl_name => Some(params),
        _ => None,
    })?;
    Some(crate::ssa_lower_member_fn_intro::expected_argument_count(
        params,
    ))
}

fn lower_fn_length_or_name(
    ctx: &mut LowerCtx<'_>,
    obj: crate::ast::ExprId,
    obj_val: Operand,
    obj_ty: Type,
    name: &str,
) -> Operand {
    let sig_id = match obj_ty {
        Type::Closure(s) | Type::FnSig(s) => s,
        _ => unreachable!(),
    };
    if name == "length" {
        if let Some(n) = try_ast_expected_count(ctx, obj) {
            return Operand::ConstI64(n as i64);
        }
        let (params, _ret) = &ctx.fn_sigs[sig_id.0 as usize];
        let visible =
            if matches!(obj_ty, Type::Closure(_)) && !params.is_empty() && params[0] == Type::Ptr {
                params.len() - 1
            } else {
                params.len()
            };
        return Operand::ConstI64(visible as i64);
    }
    // Chunk 798 — `.name` answers the runtime fn-addr registry, not
    // the call-site ident (an alias `const h = g` reads "g", a field
    // receiver `obj.f` reads its registered name — the static ident
    // approximation answered the binding name / "" for both).
    // Closure receivers route through the cell-aware chain (builtin
    // method cells, bound cells, registry); FnSig receivers are raw
    // fn body vaddrs and hit the registry directly. Miss = ES
    // anonymous-function name "".
    let fid = if matches!(obj_ty, Type::Closure(_)) {
        ctx.intrinsics.closure_name_str
    } else {
        ctx.intrinsics.fn_name_str
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![obj_val]),
        Type::Str,
        None,
    );
    Operand::Value(v)
}
