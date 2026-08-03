//! `s.{match,matchAll}(re, ...trailing)` String-receiver
//! trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 263 — fifty-sixth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S286 — String.{match,matchAll}(re, ...trailing)
//! trailing-arg ignore per ES §22.1.3.{11,13}. Spec reads
//! only `re`; tora's regex helper takes only (Str, RegExp)
//! so trailing operands typecheck-and-drop here, ssa_lower
//! mirrors with lower-and-drop in the RegExp-branch
//! match/matchAll arms.
//!
//! Returns:
//! - `Some(Ok(Type::Array<Array<String>>))` for matchAll
//!   when receiver is String AND args[0] is RegExp AND
//!   args.len() >= 2
//! - `Some(Ok(Type::Nullable<Array<String>>))` for match under
//!   the same gates
//! - `Some(Ok(Type::Any))` for single-arg `match` with an
//!   Any-typed pattern — the §22.1.3.13 step-3 custom `@@match`
//!   shape (`ssa_lower_call_str_match_custom` is the SSA mirror)
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   {match, matchAll}, no args, non-String receiver, or
//!   args[0] not RegExp — cascade falls through to the
//!   copyWithin/fill / general method dispatch arms below)

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if !matches!(m_name.as_str(), "match" | "matchAll") || args.is_empty() {
        return None;
    }
    // §22.1.3.13 step 3 — a single-arg `s.match(x)` whose pattern
    // syntactically can carry a user `@@match` types `any` (the SSA
    // mirror is `ssa_lower_call_str_match_custom`). The syntactic
    // probe runs BEFORE any type_of so a non-matching shape falls
    // through with the cascade completely untouched.
    if args.len() == 1 {
        if m_name != "match" || !any_pattern_may_carry_matcher(ast, args[0]) {
            return None;
        }
        let src_ty = match checker.type_of(ast, *src_id) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(src_ty, Type::String) {
            return None;
        }
        let aty0 = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(aty0, Type::Any) {
            return None;
        }
        return Some(Ok(Type::Any));
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty0, Type::RegExp) {
        return None;
    }
    for &aid in &args[1..] {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(if m_name == "matchAll" {
        Type::Array(Box::new(Type::Array(Box::new(Type::String))))
    } else {
        // RC-4 F1a — s.match(re) is null on miss per spec
        // §22.1.3.13 (matchAll never is); see
        // check_type_of_member_regex for the decay contract.
        Type::Nullable(Box::new(Type::Array(Box::new(Type::String))))
    }))
}

/// Can an Any-typed pattern argument actually carry a user `@@match`?
/// Purely syntactic, shared verbatim by the checker gate above and
/// the SSA gate (`ssa_lower_call_str_regex_methods`) so the static
/// result type and the emitted branch agree.
///
/// `s.replace(x, r)` with an Any-typed `x` and store evidence — the
/// §22.1.3.19 step-3 `@@replace` shape (SSA mirror:
/// `ssa_lower_call_str_match_custom::lower_replace_any_pattern`).
/// The replacer gate (checker String or Function) matches the SSA
/// twin so only shapes both lanes can emit leave the member-table
/// route; the custom replacer's return is arbitrary → `any`.
pub(crate) fn try_match_replace(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if m_name != "replace"
        || !matches!(args.len(), 1 | 2)
        || !any_pattern_may_carry_matcher(ast, args[0])
    {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty0, Type::Any) {
        return None;
    }
    // The single-arg spelling has replaceValue = undefined; the
    // two-arg one gates on shapes both SSA lanes can emit.
    if let Some(&a1) = args.get(1) {
        let aty1 = match checker.type_of(ast, a1) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(aty1, Type::String | Type::Function(..)) {
            return None;
        }
    }
    Some(Ok(Type::Any))
}

/// An `any` binding can also be a hoist-widened primitive — `var
/// separator = ","` types Any because var-hoist splits the init off
/// and stamps a synthetic `: any` on the hoisted `let`, so neither
/// the annotation nor the (now-`Uninit`) init distinguishes a
/// dynobj from a string. Routing a primitive to the custom-probe
/// branch is behavior-correct (the probe misses, the coerce lane
/// answers) but would needlessly widen the result type from
/// `Nullable<Array<Str>>` to `any` and break typed downstream reads
/// (the test262 cstm-matcher-on-*-primitive regressions). So the
/// gate keys off DIRECT store evidence instead: an Ident pattern
/// joins only when the program somewhere computed-key-stores into
/// that name (`name[expr] = v`), hands it to `Object.defineProperty`
/// / `defineProperties` / `Reflect.defineProperty`, or (r290) wrote
/// a computed-key object-literal field anywhere — `{ [Symbol.split]:
/// fn }` plants a `@@sym` at literal position, and the LetDecl
/// name-to-init association lives in Stmt space, so the arm is
/// name-blind at the same coarseness as the index-assign arm (a
/// false positive costs one runtime probe on a behavior-identical
/// fallback). An inline literal pattern joins directly. Everything
/// else (store-free names, non-literal non-Ident patterns) keeps the
/// member-table route and today's coerce behavior.
pub(crate) fn any_pattern_may_carry_matcher(ast: &Ast, arg: ExprId) -> bool {
    // An inline literal usually rides an `as any` widen — peel it.
    let mut arg = arg;
    while let Expr::As { expr, .. } = ast.get_expr(arg) {
        arg = *expr;
    }
    if let Expr::ObjectLit { fields } = ast.get_expr(arg)
        && fields
            .iter()
            .any(|(_, v)| ast.objlit_computed_keys.contains_key(v))
    {
        return true;
    }
    let Expr::Ident(name) = ast.get_expr(arg) else {
        return false;
    };
    if !ast.objlit_computed_keys.is_empty() {
        return true;
    }
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } => {
                if let Expr::Index { obj, .. } = ast.get_expr(*target)
                    && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == name)
                {
                    return true;
                }
            }
            Expr::Call { callee, args } => {
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && matches!(m.as_str(), "defineProperty" | "defineProperties")
                    && matches!(ast.get_expr(*obj), Expr::Ident(ns) if ns == "Object" || ns == "Reflect")
                    && matches!(args.first().map(|a| ast.get_expr(*a)), Some(Expr::Ident(n)) if n == name)
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// `s.split(x, limit?)` with an Any-typed `x` and store evidence —
/// the §22.1.3.23 step-2 `@@split` shape (SSA mirror:
/// `ssa_lower_call_str_match_custom::lower_split_any_pattern`).
/// The splitter's return is arbitrary → `any`; the limit passes
/// through raw (step 2 precedes step 4's ToUint32, so no numeric
/// gate here — the fallback lane coerces it itself).
pub(crate) fn try_match_split(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if m_name != "split"
        || !matches!(args.len(), 1 | 2)
        || !any_pattern_may_carry_matcher(ast, args[0])
    {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty0, Type::Any) {
        return None;
    }
    if let Some(&a1) = args.get(1)
        && let Err(e) = checker.type_of(ast, a1)
    {
        return Some(Err(e));
    }
    Some(Ok(Type::Any))
}
