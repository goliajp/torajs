//! Pre-monomorphization width hints. The generic monomorphizer runs
//! BEFORE the module-wide fixpoint can exist (it creates the concrete
//! FnDecls the fixpoint walks), so its per-call-site `T = Number`
//! width selection keeps this self-contained static classifier. The
//! mono instances it emits are then covered by the full analysis like
//! any other fn.

use crate::ast::{Ast, BinOp as AstBinOp, Expr, ExprId, Param, Stmt, UnaryOp as AstUnaryOp};
use std::collections::HashMap;

/// Compile-time numeric width hint for a generic type-arg position.
/// `Number` at the typecheck layer collapses i64 and f64 into one type;
/// at SSA the two are distinct shapes. The monomorphizer reads this
/// hint to decide whether `T = Number` should specialize as `i64`
/// (default) or `f64` (e.g. when an arg is `Math.abs(...)` whose
/// intrinsic returns f64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumWidth {
    /// No information — fall back to the default ("number" → I64) so
    /// integer-heavy generics keep their i64 specialization.
    Unknown,
    /// One or more arg expressions at this type-arg position lower to
    /// f64 (Math.* calls, `/` results, decimal literals, etc.).
    F64,
}

/// Walk an expression and return F64 if it statically lowers to f64.
/// Conservative: returns Unknown for any shape we can't classify
/// purely from the AST (Idents, member accesses on user objects, etc.)
/// so we don't accidentally widen integer-shaped generics.
pub(crate) fn infer_arg_width(ast: &Ast, eid: ExprId) -> NumWidth {
    match ast.get_expr(eid) {
        // Genuinely fractional, OR magnitude past i64 range (e.g. `1e21`)
        // — both must promote to f64 since `n as i64` would saturate.
        Expr::Number(n) if n.fract() != 0.0 || n.abs() >= 9.223372036854776e18 => NumWidth::F64,
        Expr::BinOp {
            op: AstBinOp::Div, ..
        } => NumWidth::F64,
        Expr::Unary {
            op: AstUnaryOp::Neg,
            expr,
        } => infer_arg_width(ast, *expr),
        Expr::Call { callee, .. } => {
            // Math.* methods all return f64 (libm-shaped intrinsics).
            // String.fromCharCode and Number.parseInt return non-Number
            // types so we don't need width inference for them — only
            // the Number-returning subset matters here.
            if let Expr::Member { obj, name } = ast.get_expr(*callee)
                && let Expr::Ident(ns) = ast.get_expr(*obj)
                && ns == "Math"
            {
                let _ = name;
                return NumWidth::F64;
            }
            NumWidth::Unknown
        }
        _ => NumWidth::Unknown,
    }
}

/// For each entry in a generic fn's `type_params`, walk the call site's
/// argument expressions at the positions whose param annotation names
/// that type-param. Return F64 if any of those args statically lowers to
/// f64; otherwise Unknown (defaults to i64 mono). Only consults the AST
/// — no SSA-side info — so the result is callable from
/// `monomorphize_generics` before any SSA pass runs.
#[allow(clippy::type_complexity)]
pub(crate) fn compute_typevar_widths(
    ast: &Ast,
    call_eid: ExprId,
    callee_name: &str,
    type_args: &[crate::check::Type],
    generics: &HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)>,
) -> Vec<NumWidth> {
    let arg_eids: Vec<ExprId> = match ast.get_expr(call_eid) {
        Expr::Call { args, .. } => args.clone(),
        _ => Vec::new(),
    };
    let Some((tp_names, gen_params, _, _)) = generics.get(callee_name) else {
        return vec![NumWidth::Unknown; type_args.len()];
    };
    tp_names
        .iter()
        .enumerate()
        .map(|(_tp_idx, tp_name)| {
            // Only interesting if this type-arg resolved to Number.
            // Any other type collapses to Unknown — we only widen Number
            // generics to f64.
            let mut acc = NumWidth::Unknown;
            for (pi, p) in gen_params.iter().enumerate() {
                let Some(ann) = &p.type_ann else { continue };
                // Lightweight match — bare `T` is the common case (Type
                // checker substitutes nested forms like `T[]` to a
                // concrete element type during instantiation, so the
                // monomorphizer rarely sees compound TypeVar anns at
                // call sites worth inspecting). If the param ann is
                // exactly the type-param name, the corresponding arg
                // contributes to this T's width.
                if ann == tp_name
                    && let Some(arg_eid) = arg_eids.get(pi)
                {
                    let w = infer_arg_width(ast, *arg_eid);
                    if matches!(w, NumWidth::F64) {
                        acc = NumWidth::F64;
                        break;
                    }
                }
            }
            acc
        })
        .collect()
}
