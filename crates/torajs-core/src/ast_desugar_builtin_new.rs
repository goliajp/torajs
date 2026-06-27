//! `desugar_builtin_new` extracted from [`crate::ast`] (chunk 139).
//!
//! Pre-extract this was a 240 LOC `pub fn` inline in ast.rs (over
//! the 200-line god-fn hard limit per `torajs-file-size-debt`).
//! Body verbatim moved here; ast.rs keeps a 1-line wrapper
//! preserving the `ast::desugar_builtin_new` public surface for
//! `torajs-cli` callers (main / lsp / repl / cmd_build) source-
//! compatibility.
//!
//! Six rewrites:
//!
//! 1. **`Array.of(a, b, c)`** → `[a, b, c]` (array literal). Same
//!    ExprId reused — downstream sees plain `Expr::Array`.
//! 2. **`Array(...)`** without `new` → `new Array(...)` (ES
//!    §23.1.1.1 — same internal `Construct` slot; S136 lets P0.10 /
//!    ssa_lower paths cover both spellings).
//! 3. **`new Array(...)` P0.10 MVP** — 0 args → `[]`; ≥2 args →
//!    `[a, b, ...]`; 1-arg numeric stays as `Expr::New` (ssa_lower
//!    routes to `__torajs_arr_alloc_any_filled(n)`).
//! 4. **`new Object()`** 0-arg → `{}` (ES §20.1.1.1).
//! 5. **`new RegExp(pattern?, flags?)`** with constant-string args →
//!    `Expr::Regex { pattern, flags }` literal (ES §22.2.3.1).
//!    Dynamic-arg shapes still error at lower time (pattern must be
//!    statically known for C-side embedding).
//! 6. **`new Number(x)` / `new String(x)` / `new Boolean(x)`** MVP
//!    unwrap (V3-18 m1.h.10) — no wrapper-object substrate yet, so
//!    short-circuit directly to the primitive 0-arg case or the
//!    callable `Number(x)` form.
//! 7. **`new Date(...)`** arity dispatch (Phase 2.0b.2) — 0/1/2..7
//!    args → `__torajs_date_now` / `_from_iso` / `_from_ms` /
//!    `_from_components` with JS-spec component padding (day=1,
//!    hour=min=sec=ms=0); ≥8 args → panic.

use crate::ast::{Ast, Expr};

pub(crate) fn run(ast: &mut Ast) {
    let n_exprs = ast.exprs.len();
    for i in 0..n_exprs {
        let array_of_args = match &ast.exprs[i] {
            Expr::Call { callee, args } => {
                let callee_id = *callee;
                if let Expr::Member { obj, name } = &ast.exprs[callee_id.0 as usize]
                    && name == "of"
                    && let Expr::Ident(ns) = &ast.exprs[obj.0 as usize]
                    && ns == "Array"
                {
                    Some(args.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(args) = array_of_args {
            ast.exprs[i] = Expr::Array(args);
        }
    }
    let n_exprs = ast.exprs.len();
    for i in 0..n_exprs {
        let array_call_args = match &ast.exprs[i] {
            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &ast.exprs[callee.0 as usize]
                    && name == "Array"
                {
                    Some(args.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(args) = array_call_args {
            ast.exprs[i] = Expr::New {
                class_name: "Array".into(),
                args,
            };
        }
    }
    let n = ast.exprs.len();
    for i in 0..n {
        let array_args = match &ast.exprs[i] {
            Expr::New { class_name, args } if class_name == "Array" => {
                if args.is_empty() || args.len() >= 2 {
                    Some(args.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(args) = array_args {
            ast.exprs[i] = Expr::Array(args);
        }
    }
    let n = ast.exprs.len();
    for i in 0..n {
        let zero_arg_object = matches!(
            &ast.exprs[i],
            Expr::New { class_name, args }
                if class_name == "Object" && args.is_empty()
        );
        if zero_arg_object {
            ast.exprs[i] = Expr::ObjectLit { fields: Vec::new() };
        }
    }
    let n = ast.exprs.len();
    for i in 0..n {
        let regex_plan: Option<(String, String)> = match &ast.exprs[i] {
            Expr::New { class_name, args } if class_name == "RegExp" => match args.len() {
                0 => Some(("(?:)".to_string(), String::new())),
                1 => {
                    if let Expr::String(s) = &ast.exprs[args[0].0 as usize] {
                        Some((s.clone(), String::new()))
                    } else {
                        None
                    }
                }
                2 => {
                    let pat = &ast.exprs[args[0].0 as usize];
                    let flags = &ast.exprs[args[1].0 as usize];
                    if let (Expr::String(p), Expr::String(f)) = (pat, flags) {
                        Some((p.clone(), f.clone()))
                    } else {
                        None
                    }
                }
                _ => None,
            },
            _ => None,
        };
        if let Some((pattern, flags)) = regex_plan {
            ast.exprs[i] = Expr::Regex { pattern, flags };
        }
    }
    let n = ast.exprs.len();
    for i in 0..n {
        let unwrap_args = match &ast.exprs[i] {
            Expr::New { class_name, args }
                if matches!(class_name.as_str(), "Number" | "String" | "Boolean") =>
            {
                Some(args.clone())
            }
            _ => None,
        };
        if let Some(args) = unwrap_args {
            let class_name_str = match &ast.exprs[i] {
                Expr::New { class_name, .. } => class_name.clone(),
                _ => unreachable!(),
            };
            ast.exprs[i] = if args.is_empty() {
                match class_name_str.as_str() {
                    "Number" => Expr::Number(0.0),
                    "String" => Expr::String(String::new()),
                    "Boolean" => Expr::Bool(false),
                    _ => unreachable!(),
                }
            } else {
                let callee = ast.add_expr(Expr::Ident(class_name_str));
                Expr::Call { callee, args }
            };
        }
    }
    let n = ast.exprs.len();
    for i in 0..n {
        let plan = match &ast.exprs[i] {
            Expr::New { class_name, args } if class_name == "Date" => match args.len() {
                0 => Some(("__torajs_date_now".to_string(), false, args.clone())),
                1 => {
                    let is_str = matches!(ast.exprs[args[0].0 as usize], Expr::String(_));
                    if is_str {
                        Some(("__torajs_date_from_iso".to_string(), false, args.clone()))
                    } else {
                        Some(("__torajs_date_from_ms".to_string(), false, args.clone()))
                    }
                }
                n_args if (2..=7).contains(&n_args) => Some((
                    "__torajs_date_from_components".to_string(),
                    true,
                    args.clone(),
                )),
                n_args => panic!(
                    "v0.2 #2 Phase 2.0b.2: `new Date(...)` with {n_args} args not yet supported"
                ),
            },
            _ => None,
        };
        if let Some((factory, pad_components, mut args)) = plan {
            if pad_components {
                while args.len() < 7 {
                    let val = match args.len() {
                        2 => 1.0,
                        _ => 0.0,
                    };
                    args.push(ast.add_expr(Expr::Number(val)));
                }
            }
            let callee = ast.add_expr(Expr::Ident(factory));
            ast.exprs[i] = Expr::Call { callee, args };
        }
    }
}
