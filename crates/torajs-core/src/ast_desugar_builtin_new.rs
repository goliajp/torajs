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
//!    ssa_lower paths cover both spellings). The Error family
//!    (`Error(...)` / `TypeError(...)` / ... — ES §20.5.1.1, same
//!    called-as-function = construct rule) rides the same rewrite;
//!    AggregateError / SuppressedError joined in rotation 234 (their
//!    classes are injectable now — build_error_data_subclass).
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

fn rewrite_array_of(ast: &mut Ast) {
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
}

fn rewrite_array_call(ast: &mut Ast) {
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
                type_args: vec![],
            };
        }
    }
    // `Error(...)` / `TypeError(...)` / ... without `new` → the
    // construct form (ES §20.5.1.1: the Error constructor performs
    // the same steps when called as a function). Same rewrite shape
    // as `Array(...)` above; AggregateError / SuppressedError ride it
    // too since rotation 234 (injectable via build_error_data_subclass).
}

/// `RegExp(pattern, flags)` without `new` → the construct form (ES
/// §22.2.4.1: the RegExp constructor performs the same steps when
/// called as a function). Same rewrite shape as `Array(...)` above.
/// The §22.2.3.1 same-pattern short-circuit — answering the argument
/// itself when it is already a RegExp with undefined flags — is a
/// recorded divergence: the rewrite always constructs a fresh cell.
fn rewrite_regexp_call(ast: &mut Ast) {
    let n_exprs = ast.exprs.len();
    for i in 0..n_exprs {
        let regexp_call_args = match &ast.exprs[i] {
            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &ast.exprs[callee.0 as usize]
                    && name == "RegExp"
                {
                    Some(args.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(args) = regexp_call_args {
            ast.exprs[i] = Expr::New {
                class_name: "RegExp".into(),
                args,
                type_args: vec![],
            };
        }
    }
}

fn rewrite_error_call(ast: &mut Ast) {
    let n_exprs = ast.exprs.len();
    for i in 0..n_exprs {
        let error_call: Option<(String, Vec<crate::ast::ExprId>)> = match &ast.exprs[i] {
            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = &ast.exprs[callee.0 as usize]
                    && matches!(
                        name.as_str(),
                        "Error"
                            | "TypeError"
                            | "RangeError"
                            | "SyntaxError"
                            | "ReferenceError"
                            | "EvalError"
                            | "URIError"
                            // rotation 234 — the data-carrying pair
                            // construct when called as functions too
                            // (§20.5.7.1.1 / §20.5.8.1.1 step 1); they
                            // joined the injectable family with
                            // build_error_data_subclass.
                            | "AggregateError"
                            | "SuppressedError"
                    )
                {
                    Some((name.clone(), args.clone()))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some((class_name, args)) = error_call {
            ast.exprs[i] = Expr::New {
                class_name,
                args,
                type_args: vec![],
            };
        }
    }
}

fn rewrite_array_args(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let array_args = match &ast.exprs[i] {
            Expr::New {
                class_name, args, ..
            } if class_name == "Array" => {
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
}

fn rewrite_zero_arg_object(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let object_args: Option<Vec<crate::ast::ExprId>> = match &ast.exprs[i] {
            Expr::New {
                class_name, args, ..
            } if class_name == "Object" => Some(args.clone()),
            _ => None,
        };
        let Some(args) = object_args else {
            continue;
        };
        if args.is_empty() {
            ast.exprs[i] = Expr::ObjectLit { fields: Vec::new() };
        } else {
            // §20.1.1.1 — Object's [[Construct]] with an ordinary
            // NewTarget IS its [[Call]] (nullish → fresh object,
            // else ToObject); rewrite to the call form the kernel
            // already serves (r292 — S15.2.2.1_A2 family).
            let callee = ast.add_expr(Expr::Ident("Object".to_string()));
            ast.exprs[i] = Expr::Call { callee, args };
        }
    }
}

fn rewrite_regexp_new(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let regex_plan: Option<(String, String)> = match &ast.exprs[i] {
            Expr::New {
                class_name, args, ..
            } if class_name == "RegExp" => match args.len() {
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
            // Only rewrite when the pattern parses cleanly. A
            // malformed constant-string `new RegExp("[", "u")` /
            // `new RegExp("\\u{ZZZ}", "u")` must stay as `Expr::New`
            // so `ssa_lower_new::lower_regexp` handles it — that
            // path calls `__torajs_regex_compile_or_throw` +
            // `emit_throw_check_owned` to raise a catchable
            // `SyntaxError` per ES §22.2.3.1. The literal-lowering
            // `Expr::Regex` arm intentionally skips the throw check
            // (its call is hoisted to `BlockId(0)` for LICM and needs
            // an entry-block-safe throw-check shape — L3b), so
            // rewriting a malformed constant would silently return a
            // never-match stub.
            // Strict flag parse: `None` = duplicate or unknown flag
            // letter (§22.2.3.1 Early Error). Combined with the
            // explicit `u`+`v` conflict check, the two cover every
            // pattern-independent SyntaxError face; failing any of
            // them keeps the node as `Expr::New` so
            // `lower_regexp` routes through
            // `__torajs_regex_compile_or_throw` for a catchable throw.
            let flag_bits_opt = torajs_regex::flags::parse_flags(flags.as_bytes());
            let parse_ok = match flag_bits_opt {
                Some(flag_bits)
                    if flag_bits & torajs_regex::parser::RE_FLAG_U == 0
                        || flag_bits & torajs_regex::parser::RE_FLAG_V == 0 =>
                {
                    let mut parser =
                        torajs_regex::parser::Parser::new(pattern.as_bytes(), flag_bits);
                    let root_opt = parser.parse();
                    root_opt.is_some() && !parser.err()
                }
                _ => false,
            };
            if parse_ok {
                ast.exprs[i] = Expr::Regex { pattern, flags };
            }
        }
    }
    // RFC 20260716 刀 2 — Number / String / Boolean all get the
    // wrapper substrate. 0-arg forms are `new String() / new Number()
    // / new Boolean()` per spec §22.1.1.1 / §21.1.1.1 / §20.3.1.1
    // — the constructor produces the corresponding Wrapper object
    // whose `[[StringData]] / [[NumberData]] / [[BooleanData]]` is
    // the spec default value ("" / +0 / false). Pre-2026-07-16 the
    // desugar collapsed 0-arg to a primitive literal — that was
    // observably wrong: `typeof new String()` is `"object"` (not
    // `"string"`), `Boolean(new String())` is `true` (not `false`),
    // and the wrapper receiver reaches its own [[Get]] / prototype
    // chain. Rewriting to a 1-arg `new String("") / new Number(0)
    // / new Boolean(false)` routes through the existing
    // `lower_string_wrapper` / `lower_number_wrapper` /
    // `lower_boolean_wrapper` SSA arms; the corresponding checker
    // fns' 0-arg guards (`check_type_of_new::check_*_wrapper`)
    // become unreachable — desugar owns the invariant.
    //
    // test262 impact: unblocks the "cannot assign to a property of
    // this any value" cluster's coercion cases like case 60
    // (`enumerable: new String()` — expected ToBoolean → true).
}

fn rewrite_zero_arg_wrapper_new(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let (is_number_zero, is_string_zero, is_boolean_zero) = match &ast.exprs[i] {
            Expr::New {
                class_name, args, ..
            } if args.is_empty() => (
                class_name == "Number",
                class_name == "String",
                class_name == "Boolean",
            ),
            _ => (false, false, false),
        };
        if is_number_zero {
            let default_arg = ast.add_expr(Expr::Number(0.0));
            if let Expr::New { args, .. } = &mut ast.exprs[i] {
                args.push(default_arg);
            }
        } else if is_string_zero {
            let default_arg = ast.add_expr(Expr::String(String::new()));
            if let Expr::New { args, .. } = &mut ast.exprs[i] {
                args.push(default_arg);
            }
        } else if is_boolean_zero {
            let default_arg = ast.add_expr(Expr::Bool(false));
            if let Expr::New { args, .. } = &mut ast.exprs[i] {
                args.push(default_arg);
            }
        }
    }
}

fn rewrite_date_new(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let plan = match &ast.exprs[i] {
            Expr::New {
                class_name, args, ..
            } if class_name == "Date" => match args.len() {
                0 => Some(("__torajs_date_now".to_string(), false, args.clone())),
                1 => {
                    // Literal fast lanes keep their direct kernels;
                    // anything else is §21.4.2.1 step 4 over a runtime
                    // value (Date copy / no-hint ToPrimitive / a
                    // String primitive PARSES) — the anyvalue kernel.
                    let factory = match &ast.exprs[args[0].0 as usize] {
                        Expr::String(_) => "__torajs_date_from_iso",
                        Expr::Number(_) => "__torajs_date_from_ms",
                        _ => "__torajs_date_from_value",
                    };
                    Some((factory.to_string(), false, args.clone()))
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
                // Rotation 373 (L3b 373-02) — §21.4.2.1 step 5: every
                // supplied component runs ToNumber, in argument order.
                // A non-Number-literal argument wraps in the
                // `Number(x)` coercion call the m1.h.8 machinery
                // already owns (`new Date(1859, '10', 24)` reads
                // month 10, not NaN); a Number literal skips the wrap
                // — the components kernel takes f64 directly.
                for a in args.iter_mut() {
                    if !matches!(ast.exprs[a.0 as usize], Expr::Number(_)) {
                        let callee = ast.add_expr(Expr::Ident("Number".to_string()));
                        *a = ast.add_expr(Expr::Call {
                            callee,
                            args: vec![*a],
                        });
                    }
                }
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

mod fn_ctor;
mod promise;

pub(crate) fn run(ast: &mut Ast) {
    rewrite_array_of(ast);
    rewrite_array_call(ast);
    rewrite_regexp_call(ast);
    rewrite_error_call(ast);
    rewrite_array_args(ast);
    rewrite_zero_arg_object(ast);
    rewrite_regexp_new(ast);
    rewrite_zero_arg_wrapper_new(ast);
    rewrite_date_new(ast);
    promise::rewrite_promise_new(ast);
    fn_ctor::rewrite_function_zero_arg(ast);
}
