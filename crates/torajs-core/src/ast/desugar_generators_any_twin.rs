//! The all-`any` twin of a generator whose parameters a value of
//! another kind can reach.
//!
//! `function* gen(s: string) { yield s.length }` desugars to a factory
//! `gen(s)` that stores `s` in a `string` field of `__Gen_gen`, and the
//! machine reads `this.s.length` at that field's layout. Reached
//! through `any` (`const g: any = gen; g(5)`) or called with an `any`
//! argument, the call must bind the value as passed (§10.2.1.3) and
//! read `s.length` as a [[Get]] on it (§13.3.2.1): `undefined` for
//! `5`. A plain function takes an all-`any` clone of its body for
//! that (`check_monomorph_any_widen`), but a generator's body is no
//! longer a body by then: it is a class whose field types the clone
//! cannot change. So the twin is cut here, from the source generator,
//! before the machine is built: `function* __genany_<name>` with
//! every non-rest parameter `any` and the yield type `any`. It
//! desugars like any generator, into its own `__Gen___genany_<name>`
//! whose param fields are `any`; the typed generator is untouched and
//! still serves every argument that fits. The widen lanes call the
//! twin instead of cloning the factory ([`generator_any_twin`]).
//!
//! Minted only for a generator with a parameter a scalar coercion
//! cannot serve (anything but a number / boolean / bigint / `any`
//! slot) whose factory may meet such an argument: its name is read as
//! a value (the callee of a direct call does not count), its
//! capturing expression's wrapper is, or the checker found a direct
//! call handing it an `any` argument and asked for the twin
//! ([`Ast::generator_twin_requests`]; the driver compiles again). A
//! generator nothing reaches that way carries no twin. A generator
//! expression that reads an enclosing local by
//! reference shares its wrapper's capture record with the twin: the
//! wrapper's twin calls it with the original's env. Left out: the
//! frame link (`desugar_generators_frame_link`) — a by-reference
//! wrapper inside another generator's body, and that generator —
//! which edits the nested factory and its record by name.

use super::clone_body::BodyCloner;
use std::collections::HashSet;

use super::{Ast, Expr, ExprId, Param, Stmt};

const PREFIX: &str = "__genany_";

/// The twin factory of generator factory `name`, when one was minted.
/// A generator factory without one is asked for one on `ast`: the
/// driver compiles the program again with the request seeded.
pub(crate) fn generator_any_twin(ast: &mut Ast, name: &str) -> Option<String> {
    let twin = format!("{PREFIX}{name}");
    if ast.generator_factory_classes.contains_key(&twin) {
        return Some(twin);
    }
    if ast.generator_factory_classes.contains_key(name) && !name.starts_with(PREFIX) {
        ast.generator_twin_requests.insert(name.to_string());
    }
    None
}

/// Is `name` a decl the twin of some generator desugared into: the
/// factory, its iterator class's ctor, methods and step type, or the
/// class's own factory?
pub(crate) fn is_generator_any_twin_decl(name: &str) -> bool {
    const GEN: &[u8] = b"_Gen_";
    name.starts_with(PREFIX)
        || (name.as_bytes().windows(GEN.len() + PREFIX.len()))
            .any(|w| w.starts_with(GEN) && w.ends_with(PREFIX.as_bytes()))
}

/// The twin factory of `name`, when one was minted (no request).
pub(crate) fn minted_generator_any_twin(ast: &Ast, name: &str) -> Option<String> {
    let twin = format!("{PREFIX}{name}");
    ast.generator_factory_classes
        .contains_key(&twin)
        .then_some(twin)
}

/// Mint the twin of every top-level generator that needs one. True
/// when one was minted.
pub(super) fn mint_generator_any_twins(ast: &mut Ast) -> bool {
    let by_eid = super::desugar_generators_frame_link::wrappers_by_eid(ast);
    let mut linked = Vec::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            body,
            is_generator: true,
            ..
        } = s
        {
            super::desugar_generators_frame_link::nested_wrappers(ast, body, &by_eid, &mut linked);
        }
    }
    let escaped = escaped_names(ast);
    let sources: Vec<usize> = ast
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                body,
                is_generator: true,
                ..
            } if type_params.is_empty()
                && !name.starts_with(PREFIX)
                && (ast.generator_twin_requests.contains(name)
                    || escaped.contains(name.as_str()) && params.iter().any(lossy_param))
                && !linked.iter().any(|(_, g)| g == name)
                && !holds_wrapper(ast, body, &by_eid) =>
            {
                Some(i)
            }
            _ => None,
        })
        .collect();
    drop(escaped);
    let minted = !sources.is_empty();
    for i in sources {
        mint_one(ast, i);
    }
    minted
}

/// Names read other than as the callee of a call, and the generator
/// each capturing expression's wrapper calls when the wrapper itself
/// is such a value. A generator expression bound by an unannotated
/// declaration whose name is only ever called stays a callee.
fn escaped_names(ast: &Ast) -> HashSet<&str> {
    let callees: HashSet<ExprId> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, .. } => Some(*callee),
            _ => None,
        })
        .collect();
    let values: Vec<(ExprId, &str)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Expr::Ident(n) if !callees.contains(&ExprId(i as u32)) => {
                Some((ExprId(i as u32), n.as_str()))
            }
            _ => None,
        })
        .collect();
    let read_as_value: HashSet<&str> = values.iter().map(|(_, n)| *n).collect();
    let mut called_binding: HashSet<ExprId> = HashSet::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::LetDecl {
            name,
            type_ann: None,
            init,
            ..
        } = s
            && !read_as_value.contains(name.as_str())
        {
            called_binding.insert(*init);
        }
    });
    let mut out: HashSet<&str> = values
        .iter()
        .filter(|(eid, _)| !called_binding.contains(eid))
        .map(|(_, n)| *n)
        .collect();
    for eid in ast.gen_fn_exprs.keys() {
        if callees.contains(eid) || called_binding.contains(eid) {
            continue;
        }
        if let Expr::ArrowFn { body, .. } = ast.get_expr(*eid)
            && let Some(Stmt::Return(Some(call))) = body.last()
            && let Expr::Call { callee, .. } = ast.get_expr(*call)
            && let Expr::Ident(n) = ast.get_expr(*callee)
        {
            out.insert(n.as_str());
        }
    }
    out
}

fn holds_wrapper(
    ast: &Ast,
    body: &[Stmt],
    by_eid: &std::collections::HashMap<crate::ast::ExprId, String>,
) -> bool {
    let mut ws = Vec::new();
    super::desugar_generators_frame_link::nested_wrappers(ast, body, by_eid, &mut ws);
    !ws.is_empty()
}

/// A slot whose run-time check can fail: a declared non-rest param
/// that is not a number / boolean / bigint / `any` scalar.
fn lossy_param(p: &Param) -> bool {
    !p.is_rest
        && p.type_ann.as_deref().is_some_and(|ann| {
            !matches!(
                ann.trim(),
                "any" | "unknown" | "number" | "boolean" | "bigint" | "i64" | "i32" | "f64"
            )
        })
}

fn mint_one(ast: &mut Ast, idx: usize) {
    let Stmt::FnDecl {
        name,
        params,
        body,
        span,
        ..
    } = ast.stmts[idx].clone()
    else {
        return;
    };
    let mut cloner = BodyCloner::new(ast);
    let cloned_body = cloner.clone_stmts(&body);
    let twin_params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: p.name.clone(),
            type_ann: if p.is_rest {
                p.type_ann.clone()
            } else {
                Some("any".to_string())
            },
            default: p.default.map(|d| cloner.clone_expr(d)),
            is_rest: p.is_rest,
        })
        .collect();
    super::clone_body_tables::migrate(&mut cloner);
    let twin = format!("{PREFIX}{name}");
    if let Some(&n) = ast.gen_param_destr_prefix.get(&name) {
        ast.gen_param_destr_prefix.insert(twin.clone(), n);
    }
    if let Some(info) = ast.gen_env_captures.get(&name).cloned() {
        ast.gen_env_captures.insert(twin.clone(), info);
    }
    if ast.async_generator_fns.contains(&name) {
        ast.async_generator_fns.insert(twin.clone());
    }
    ast.stmts.push(Stmt::FnDecl {
        name: twin,
        type_params: Vec::new(),
        params: twin_params,
        return_type: None,
        body: cloned_body,
        is_generator: true,
        span,
    });
}
