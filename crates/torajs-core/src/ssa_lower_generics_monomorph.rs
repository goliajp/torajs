//! Generics monomorphization machine parts (M3). The monomorphizer
//! itself moved into the check pipeline (`check_monomorph.rs`, RFC
//! 20260713-mono-check-specializations) so each specialization body
//! is type-checked after substitution — this file keeps the shared
//! parts both sides use:
//!
//!   - `collect_generics` / `Generics` — index of generic FnDecls.
//!   - `compute_arg_anns` — width/closure-shape-aware ann key per
//!     call site.
//!   - `substitute_in_ann` — bare-word type-param substitution inside
//!     annotation strings. Also used by:
//!       * `num_width::alias` for width-aware alias resolution
//!       * `ssa_lower_parse_type` for FnAnn subst
//!   - `substitute_in_stmt` — recursive ann substitution over a body.
//!   - `name_safe` — mono-name encoding of an ann string.
//!
//! Sibling `ssa_lower_generics_tvdefault.rs` still rewrites the
//! `__tvdefault__<T>` marker Idents (planted by class-factory
//! default-init) with the concrete default expression for the
//! substituted type.

use std::collections::HashMap;

use crate::ast::{Ast, Param, Stmt};
use crate::check::{self as check_mod, type_to_ann};

/// Encode an annotation string into a name-safe form for use inside a
/// monomorphized fn name. `number` → `number`; `number[]` → `number_arr`;
/// `__fn(number)->number` → `fn_number_to_number`. Distinct user types
/// produce distinct strings so the cache key `(name, type_args)` resolves
/// to a unique mono fn.
pub(crate) fn name_safe(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => c,
            _ => '_',
        })
        .collect()
}

/// Replace bare-word occurrences of each `from` token with `to` inside an
/// annotation string. Word boundary = anything that isn't an alphanumeric
/// or `_`. Used by `monomorphize_generics` to rewrite a generic FnDecl's
/// type annotations into a concrete specialization (e.g. `T` → `number`,
/// `T[]` → `number[]`, `__fn(T)->T` → `__fn(number)->number`).
pub(crate) fn substitute_in_ann(ann: &str, subst: &[(String, String)]) -> String {
    let mut out = String::with_capacity(ann.len());
    let bytes = ann.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        let is_word_start = c.is_ascii_alphabetic() || c == b'_';
        if !is_word_start {
            out.push(c as char);
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() {
            let cc = bytes[i];
            if cc.is_ascii_alphanumeric() || cc == b'_' {
                i += 1;
            } else {
                break;
            }
        }
        let word = &ann[start..i];
        if let Some((_, replacement)) = subst.iter().find(|(from, _)| from == word) {
            out.push_str(replacement);
        } else {
            out.push_str(word);
        }
    }
    out
}

/// Substitute every type-param name in a `Stmt`'s body recursively.
/// Currently only `Stmt::LetDecl` and the immediate FnDecl signature
/// carry annotation strings; we walk into nested Block / If / While / For
/// bodies. `subst` is the (param → concrete-ann) list applied to every
/// `type_ann` Some(...) string encountered.
pub(crate) fn substitute_in_stmt(stmt: &mut Stmt, subst: &[(String, String)]) {
    match stmt {
        Stmt::LetDecl { type_ann, .. } => {
            if let Some(ann) = type_ann {
                *ann = substitute_in_ann(ann, subst);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            substitute_in_stmt(then_branch, subst);
            if let Some(eb) = else_branch {
                substitute_in_stmt(eb, subst);
            }
        }
        Stmt::While { body, .. } => substitute_in_stmt(body, subst),
        Stmt::Labeled { body, .. } => substitute_in_stmt(body, subst),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                substitute_in_stmt(i, subst);
            }
            substitute_in_stmt(body, subst);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                substitute_in_stmt(s, subst);
            }
        }
        Stmt::FnDecl {
            params,
            return_type,
            body,
            ..
        } => {
            for p in params {
                if let Some(ann) = &mut p.type_ann {
                    *ann = substitute_in_ann(ann, subst);
                }
            }
            if let Some(rt) = return_type {
                *rt = substitute_in_ann(rt, subst);
            }
            for s in body {
                substitute_in_stmt(s, subst);
            }
        }
        // Expr / Return / Break / Continue / TypeDecl carry no annotation
        // strings worth substituting in the M3-minimal surface.
        _ => {}
    }
}

/// Width- and closure-shape-aware annotation strings for one generic
/// call site's inferred type args. Shared by `check_monomorph`'s
/// top-level seed loop and its inner-call seeding (the records the
/// checker produces while checking each specialization body).
/// `param_env` carries the enclosing specialization's substituted
/// param anns for param-passthrough args (empty at top level).
///
/// Width: a type-arg that resolved to `Type::Number` picks "f64" when
/// any arg position naming that type-param statically lowers to f64
/// (Math.* call, decimal literal, etc.); otherwise "number" → I64.
/// This lets one generic fn serve both `check<T=Number>(1, 2)` (I64
/// mono) and `check<T=Number>(Math.abs(-1), 1)` (F64 mono) cleanly.
///
/// Closure shape: `check::Type::Function` carries no closure-vs-bare-
/// fn distinction, so `type_to_ann` always answers `__fn(` — but a
/// closure ARG instantiating that type-param needs a `__cls(` slot
/// (env-block ptr, env-first CallIndirect). Without the flip the mono
/// body's call arm treats the env ptr as a bare fn ptr and jumps into
/// it (SIGBUS). The 153-pass `__fn(`→`__cls(` infection can't see
/// these anns — monomorphization runs after typecheck.
pub(crate) fn compute_arg_anns(
    ast: &Ast,
    eid: crate::ast::ExprId,
    callee_name: &str,
    type_args: &[check_mod::Type],
    generics: &HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)>,
    param_env: &HashMap<String, String>,
) -> Vec<String> {
    let widths: Vec<crate::num_width::NumWidth> = crate::num_width::compute_typevar_widths(
        ast,
        eid,
        callee_name,
        type_args,
        generics,
        param_env,
    );
    let cls_shapes: Vec<crate::ssa_lower_generics_mono_shapes::ClsShape> =
        crate::ssa_lower_generics_mono_shapes::compute_typevar_closure_shapes(
            ast,
            eid,
            callee_name,
            type_args,
            generics,
            param_env,
        );
    type_args
        .iter()
        .zip(widths.iter())
        .zip(cls_shapes.iter())
        .map(|((ty, w), shape)| {
            use crate::ssa_lower_generics_mono_shapes::ClsShape;
            if matches!(ty, check_mod::Type::Number) && matches!(w, crate::num_width::NumWidth::F64)
            {
                "f64".into()
            } else {
                let ann = type_to_ann(ty);
                match shape {
                    ClsShape::Closure if ann.starts_with("__fn(") => {
                        format!("__cls({}", &ann["__fn(".len()..])
                    }
                    _ => ann,
                }
            }
        })
        .collect()
}

/// Alias for the generic-FnDecl index: name -> (type_params, params,
/// return_type, body).
pub(crate) type Generics = HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)>;

/// Index the AST's generic FnDecls by name. Cloned out so callers can
/// mutate the AST freely while holding the index.
pub(crate) fn collect_generics(ast: &Ast) -> Generics {
    ast.stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                is_generator: _,
                span: _,
            } if !type_params.is_empty() => Some((
                name.clone(),
                (
                    type_params.clone(),
                    params.clone(),
                    return_type.clone(),
                    body.clone(),
                ),
            )),
            _ => None,
        })
        .collect()
}
