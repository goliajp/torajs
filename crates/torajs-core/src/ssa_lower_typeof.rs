//! `Expr::TypeOf { expr }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-71
//! of the decomp (chunks 1-70 = ... + `Expr::OptChain` typed-tier
//! null-check arm).
//!
//! `typeof` (ES §13.5.3) returns a string literal classifying the
//! operand. We resolve at compile time where possible (most cases)
//! and only fall back to the runtime `__torajs_any_typeof` helper
//! for genuine `Type::Any` operands. Six layers tried in source
//! order; each short-circuits to a string literal on hit:
//!
//! 1. **P1.5** — frontend `check::Type::Undefined` source returns
//!    `"undefined"` per ES §6.1.1.1. Distinct from `typeof null`
//!    which returns `"object"` (the historical JS bug spec
//!    preserves). SSA-side both lower to `ConstPtrNull` / Ptr-
//!    shaped operands, so we can't tell them apart by SSA Type
//!    alone — `expr_types` bridges.
//! 2. **V3-18 m1.h.20** — Ident global typeof match table:
//!    `undefined` → `"undefined"`; `Math` / `JSON` / `Reflect` /
//!    `globalThis` / `console` → `"object"`; constructor cluster
//!    + `parseInt`/`parseFloat`/`isNaN`/`isFinite`/`encodeURI`
//!    family → `"function"`. P4.6: `__class_<NAME>` prefix
//!    (Phase A1 `synthesize_class_globals` rewrite) → `"function"`
//!    (matches both user classes AND injected builtins like
//!    `Error`).
//! 3. **V3-18 m2.c / m2.d** — `typeof <expr>.<Object-prototype-
//!    method>` → `"function"` (`.constructor`, `.hasOwnProperty`,
//!    `.propertyIsEnumerable`, `.isPrototypeOf`, `.valueOf`,
//!    `.toString`, `.toLocaleString`).
//! 4. **Member namespace** — `typeof <ns>.<member>` on
//!    `{Math|JSON|Reflect|globalThis|console|Object|Array|String|
//!    Number|Boolean|Symbol|Date|RegExp|Error|BigInt|Promise|Map|
//!    Set}` dispatches by `member` shape:
//!    - Symbol well-known singletons → `"symbol"` (iterator,
//!      asyncIterator, toPrimitive, toStringTag, hasInstance,
//!      isConcatSpreadable, match, replace, search, split,
//!      species, unscopables).
//!    - Math constants (PI/E/LN2/LN10/LOG2E/LOG10E/SQRT2/SQRT1_2)
//!      + Number constants (MAX_VALUE/MIN_VALUE/MAX_SAFE_INTEGER/
//!      MIN_SAFE_INTEGER/EPSILON/POSITIVE_INFINITY/
//!      NEGATIVE_INFINITY/NaN) + `.length` → `"number"`.
//!    - `.prototype` → `"object"` (V3-18 m2.c).
//!    - `.name` → `"string"`.
//!    - Else → `"function"`.
//! 5. **V3-18 m1.h.3** — `typeof undeclared` returns `"undefined"`
//!    without throwing. Short-circuit before `lower_expr` (which
//!    would error on the unresolved Ident). V3-18 m1.h.11
//!    carve-out: `NaN` / `Infinity` lower to ConstF64 so they
//!    typeof as `"number"`; skip the shortcut for them.
//! 6. **SSA-type fold** — lower the operand (side effects still
//!    fire), pick the string by SSA `Type`:
//!    - I64/F64/I32 → `"number"`; Bool → `"boolean"`;
//!      Str/Substr → `"string"`; Symbol → `"symbol"`;
//!      BigInt → `"bigint"`; Closure/FnSig → `"function"` (V3-18
//!      m1.h.7 — callable values per spec); Obj/Arr/RegExp/Date/
//!      Promise/Weak{Ref,Map,Set}/Map/Set/MapIter/ArrIter/Void/
//!      Ptr → `"object"`; Any → runtime `__torajs_any_typeof`
//!      (P0.2 — reads box tag + heap header type_tag for the
//!      spec-mandated literal).
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::TypeOf` match arm bottoms out here).

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, expr: ExprId) -> Operand {
    if let Some(check_mod::Type::Undefined) = ctx.expr_types.get(&expr) {
        return Operand::Value(ctx.intern_string_literal("undefined"));
    }
    if let Some(s) = try_ident_global_typeof(ctx, expr) {
        return Operand::Value(ctx.intern_string_literal(s));
    }
    if let Some(s) = try_member_typeof(ctx, expr) {
        return Operand::Value(ctx.intern_string_literal(s));
    }
    if let Some(s) = try_undeclared_ident_typeof(ctx, expr) {
        return Operand::Value(ctx.intern_string_literal(s));
    }
    lower_by_ssa_type(ctx, expr)
}

fn try_ident_global_typeof(ctx: &LowerCtx<'_>, expr: ExprId) -> Option<&'static str> {
    let Expr::Ident(name) = ctx.ast.get_expr(expr) else {
        return None;
    };
    match name.as_str() {
        "undefined" => Some("undefined"),
        "Math" | "JSON" | "Reflect" | "globalThis" | "console" => Some("object"),
        "Number" | "String" | "Boolean" | "Symbol" | "Date" | "Array" | "Object" | "RegExp"
        | "Error" | "Function" | "Promise" | "Map" | "Set" | "WeakMap" | "WeakSet" | "Proxy"
        | "BigInt" | "ArrayBuffer" | "DataView" | "TypeError" | "RangeError" | "SyntaxError"
        | "ReferenceError" | "parseInt" | "parseFloat" | "isNaN" | "isFinite" | "encodeURI"
        | "decodeURI" | "encodeURIComponent" | "decodeURIComponent" => Some("function"),
        n if n.starts_with("__class_") => Some("function"),
        _ => None,
    }
}

fn try_member_typeof(ctx: &LowerCtx<'_>, expr: ExprId) -> Option<&'static str> {
    // Layer 3: Object-prototype-method on any receiver → "function".
    if let Expr::Member {
        name: member_name, ..
    } = ctx.ast.get_expr(expr)
        && matches!(
            member_name.as_str(),
            "constructor"
                | "hasOwnProperty"
                | "propertyIsEnumerable"
                | "isPrototypeOf"
                | "valueOf"
                | "toString"
                | "toLocaleString"
        )
    {
        return Some("function");
    }
    // Layer 4: namespace member dispatch.
    let Expr::Member {
        obj,
        name: member_name,
    } = ctx.ast.get_expr(expr)
    else {
        return None;
    };
    let Expr::Ident(ns) = ctx.ast.get_expr(*obj) else {
        return None;
    };
    if !is_known_ns(ns) {
        return None;
    }
    Some(classify_ns_member(ns, member_name))
}

fn is_known_ns(ns: &str) -> bool {
    matches!(
        ns,
        "Math"
            | "JSON"
            | "Reflect"
            | "globalThis"
            | "console"
            | "Object"
            | "Array"
            | "String"
            | "Number"
            | "Boolean"
            | "Symbol"
            | "Date"
            | "RegExp"
            | "Error"
            | "BigInt"
            | "Promise"
            | "Map"
            | "Set"
    )
}

fn classify_ns_member(ns: &str, member_name: &str) -> &'static str {
    let is_symbol_well_known = ns == "Symbol"
        && matches!(
            member_name,
            "iterator"
                | "asyncIterator"
                | "toPrimitive"
                | "toStringTag"
                | "hasInstance"
                | "isConcatSpreadable"
                | "match"
                | "replace"
                | "search"
                | "split"
                | "species"
                | "unscopables"
        );
    let is_math_const = ns == "Math"
        && matches!(
            member_name,
            "PI" | "E" | "LN2" | "LN10" | "LOG2E" | "LOG10E" | "SQRT2" | "SQRT1_2"
        );
    let is_number_const = ns == "Number"
        && matches!(
            member_name,
            "MAX_VALUE"
                | "MIN_VALUE"
                | "MAX_SAFE_INTEGER"
                | "MIN_SAFE_INTEGER"
                | "EPSILON"
                | "POSITIVE_INFINITY"
                | "NEGATIVE_INFINITY"
                | "NaN"
        );
    if is_symbol_well_known {
        "symbol"
    } else if is_math_const || is_number_const || member_name == "length" {
        "number"
    } else if member_name == "prototype" {
        "object"
    } else if member_name == "name" {
        "string"
    } else {
        "function"
    }
}

fn try_undeclared_ident_typeof(ctx: &LowerCtx<'_>, expr: ExprId) -> Option<&'static str> {
    let Expr::Ident(name) = ctx.ast.get_expr(expr) else {
        return None;
    };
    if ctx.locals.get(name).is_some()
        || ctx.globals.contains_key(name)
        || ctx.fn_table.contains_key(name)
        || matches!(name.as_str(), "NaN" | "Infinity")
    {
        return None;
    }
    Some("undefined")
}

fn lower_by_ssa_type(ctx: &mut LowerCtx<'_>, expr: ExprId) -> Operand {
    let v = ctx.lower_expr(expr);
    let ty = ctx.operand_ty(&v);
    let s: &str = match ty {
        Type::I64 | Type::F64 | Type::I32 => "number",
        Type::Bool => "boolean",
        // RFC 20260707 chunk 3 — a nullable-str source (missed
        // exec/match capture slot) may hold the undefined sentinel
        // ("undefined") or NULL (JS null → "object"); everything
        // else keeps the zero-cost static fold.
        Type::Str if crate::ssa_lower_nullable_guard::is_nullable_str_source(ctx, expr) => {
            return emit_str_typeof_runtime(ctx, v);
        }
        // RFC 20260707 residual — a Substr slot may hold the
        // Substr-shaped undefined sentinel (string index OOB read),
        // so every Substr typeof takes the two-state runtime branch
        // (sentinel address → "undefined", else "string"). Substr
        // typeof is cold; Str keeps its infection-gated static fold.
        Type::Substr => {
            return emit_substr_typeof_runtime(ctx, v);
        }
        Type::Str => "string",
        Type::Symbol => "symbol",
        Type::BigInt => "bigint",
        Type::Closure(_) | Type::FnSig(_) => "function",
        Type::Obj(_)
        | Type::Arr(_)
        | Type::RegExp
        | Type::Date
        | Type::Promise
        | Type::WeakRef
        | Type::WeakMap
        | Type::WeakSet
        | Type::Map
        | Type::Set
        | Type::MapIter
        | Type::ArrIter => "object",
        Type::Void | Type::Ptr => "object",
        Type::Any => {
            let cur_block = ctx.cur_block;
            let r = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.any_typeof, vec![v]),
                Type::Str,
                None,
            );
            return Operand::Value(r);
        }
    };
    Operand::Value(ctx.intern_string_literal(s))
}

/// Three-state `typeof` for a Str slot that may hold a nullish repr
/// (RFC 20260707 chunk 3): NULL → "object" (JS null), the undefined
/// sentinel cell → "undefined", a real Str → "string". Branch chain
/// over interned literals, same alloca/merge shape as
/// `coerce_to_bool`'s Str arm.
fn emit_str_typeof_runtime(ctx: &mut LowerCtx<'_>, v: Operand) -> Operand {
    let result_slot = ctx.alloca_in_entry(Type::Str, Some("__typeof_r"));
    let null_blk = ctx.f.add_block();
    let chk_undef_blk = ctx.f.add_block();
    let undef_blk = ctx.f.add_block();
    let str_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: null_blk,
            else_blk: chk_undef_blk,
        },
    );
    ctx.cur_block = null_blk;
    let obj_lit = ctx.intern_string_literal("object");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(obj_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = chk_undef_blk;
    let sentinel = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_undef, vec![]),
        Type::Str,
        None,
    );
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v, Operand::Value(sentinel)),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: str_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let undef_lit = ctx.intern_string_literal("undefined");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(undef_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = str_blk;
    let str_lit = ctx.intern_string_literal("string");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(str_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(result_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(r)
}

/// Two-state `typeof` for a Substr slot (RFC 20260707 residual):
/// the Substr-shaped undefined sentinel → "undefined", any real
/// view → "string". Same alloca/merge shape as the Str three-state
/// chain minus the NULL arm (the index-get producers never answer
/// NULL — nullish inputs propagate the sentinel).
fn emit_substr_typeof_runtime(ctx: &mut LowerCtx<'_>, v: Operand) -> Operand {
    let result_slot = ctx.alloca_in_entry(Type::Str, Some("__typeof_r"));
    let undef_blk = ctx.f.add_block();
    let str_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let sentinel = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.substr_undef, vec![]),
        Type::Substr,
        None,
    );
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v, Operand::Value(sentinel)),
        Type::Bool,
        None,
    );
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: str_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let undef_lit = ctx.intern_string_literal("undefined");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(undef_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = str_blk;
    let str_lit = ctx.intern_string_literal("string");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(str_lit), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(result_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(r)
}
