//! The two side tables the `analyze` driver builds around its walk —
//! carved out of `mod.rs` (file-size hard limit: the fn sat on the
//! known-debt list at 225 lines and the file at 508). Verbatim moves:
//! the slot-name registry the walk phase resolves names against, and
//! the W4 objlit shape-join table the frozen verdict set feeds.
//!
//! Rotation 507 — the registry also carries the const-int table
//! ([`record_const_int`]): the value behind an immutable binding whose
//! initializer is an integer literal, so a `const step = 3` counter
//! reads as the same small-step counter a literal `3` does.

use crate::ast::{GenericStructDecls, PropKey};
use std::collections::{HashMap, HashSet};

use super::{Analysis, SlotKey, container, let_names};
use crate::ast::{Ast, Expr, Stmt, UnaryOp};

/// Pre-walk name registry: every fn's param names, every top-level
/// let (Global-keyed, including ones nested in top-level blocks /
/// loops — lowering's `num_width_local_key` keys every main-fn let
/// as Global, so the analysis must resolve the same names or their
/// widths silently read as NotNum, the mandelbrot
/// `cr = i/100-1.5` FpToSi truncation), and the by-name index the
/// alias resolution consults. Shadowing collapses same-named
/// bindings into one key — joins only cost width, never correctness.
pub(super) type SlotRegistry = (
    HashMap<String, Vec<String>>,
    HashSet<String>,
    HashMap<String, Vec<SlotKey>>,
    HashMap<SlotKey, f64>,
);

/// The integer literal an immutable binding was initialized with,
/// keyed by the slot it resolves to. Rotation 507 — a step written
/// `const step = 3` is the same counter as one written `3`, and the
/// W5 carve-out has to see it that way or the accumulator pays a
/// versioned loop and a per-iteration guard the literal form does not
/// (506-06). `mutable: false` is the whole guarantee: the checker
/// rejects every assignment to such a binding, so the value the
/// declaration shows is the value every read sees.
fn record_const_int(ast: &Ast, decl: Option<&Stmt>, key: SlotKey, out: &mut HashMap<SlotKey, f64>) {
    let Some(Stmt::LetDecl {
        mutable: false,
        init,
        is_var: false,
        ..
    }) = decl
    else {
        return;
    };
    if let Some(v) = int_literal_value(ast, *init) {
        out.insert(key, v);
    }
}

/// The value of an integer-literal initializer (`3`, `-3`), or
/// `None` for anything else.
fn int_literal_value(ast: &Ast, eid: crate::ast::ExprId) -> Option<f64> {
    match ast.get_expr(eid) {
        Expr::Number(n) if n.fract() == 0.0 => Some(*n),
        Expr::Unary {
            op: UnaryOp::Neg,
            expr,
        } => match ast.get_expr(*expr) {
            Expr::Number(n) if n.fract() == 0.0 => Some(-*n),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn collect_slot_registry(ast: &Ast) -> SlotRegistry {
    let mut fn_params: HashMap<String, Vec<String>> = HashMap::new();
    let mut toplevel_lets: HashSet<String> = HashSet::new();
    let mut by_name: HashMap<String, Vec<SlotKey>> = HashMap::new();
    let mut const_ints: HashMap<SlotKey, f64> = HashMap::new();
    for stmt in &ast.stmts {
        match stmt {
            Stmt::FnDecl { name, params, .. } => {
                fn_params.insert(
                    name.clone(),
                    params.iter().map(|p| p.name.clone()).collect(),
                );
                for p in params {
                    by_name
                        .entry(p.name.clone())
                        .or_default()
                        .push(SlotKey::Param(name.clone(), p.name.clone()));
                }
                if let Stmt::FnDecl { body, .. } = stmt {
                    for b in body {
                        let_names::walk_bindings(b, &mut |v, decl| {
                            by_name
                                .entry(v.to_string())
                                .or_default()
                                .push(SlotKey::Local(name.clone(), v.to_string()));
                            record_const_int(
                                ast,
                                decl,
                                SlotKey::Local(name.clone(), v.to_string()),
                                &mut const_ints,
                            );
                        });
                    }
                }
            }
            other => {
                let_names::walk_bindings(other, &mut |name, decl| {
                    toplevel_lets.insert(name.to_string());
                    by_name
                        .entry(name.to_string())
                        .or_default()
                        .push(SlotKey::Global(name.to_string()));
                    record_const_int(
                        ast,
                        decl,
                        SlotKey::Global(name.to_string()),
                        &mut const_ints,
                    );
                });
            }
        }
    }
    (fn_params, toplevel_lets, by_name, const_ints)
}

/// Rotation 664 — `name → lifted closure` for a callback argument
/// spelled through an immutable binding (`const show = (v: number) =>
/// …; p.then(show)` / `xs.forEach(show)`). The callback wirings resolve
/// only a function declaration's name or a closure literal, so a value
/// arriving through the promise or the element slot never reached the
/// closure's parameter: the lifted function kept an integer face for
/// `v` while the runtime handed it f64 bits (`1.5` printed `1`), and a
/// fractional return landed in an integer result slot.
///
/// Keyed by name, so only a name bound exactly once in the whole
/// program — one slot of any kind (`by_name`), no function of that
/// name — resolves; anything else keeps today's unresolved answer.
pub(super) fn collect_bound_closures(
    ast: &Ast,
    fn_params: &HashMap<String, Vec<String>>,
    by_name: &HashMap<String, Vec<SlotKey>>,
) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let mut visit = |name: &str, decl: Option<&Stmt>| {
        let Some(Stmt::LetDecl {
            mutable: false,
            is_var: false,
            init,
            ..
        }) = decl
        else {
            return;
        };
        if let crate::ast::Expr::Closure { fn_name, .. } = ast.get_expr(*init)
            && by_name.get(name).is_some_and(|slots| slots.len() == 1)
            && !fn_params.contains_key(name)
        {
            out.insert(name.to_string(), fn_name.clone());
        }
    };
    for stmt in &ast.stmts {
        match stmt {
            Stmt::FnDecl { body, .. } => {
                for b in body {
                    let_names::walk_bindings(b, &mut visit);
                }
            }
            other => let_names::walk_bindings(other, &mut visit),
        }
    }
    out
}

/// W4 shape-join (rotation 371) — same-shaped anonymous literals
/// share one struct layout at lowering (resolve_objlit_layout's
/// first-match scan admits F64-vs-I64 as coercible), so the LAYOUT
/// slot width must join across the family even though each binding
/// keeps its own OPERATION width (the d5 slot-granularity contract).
/// Collect, per ordered field-name shape, the fields any family
/// member's verdict floats; `apply_w4_widen` consults this beside
/// the literal's own key, so the FIRST registrant already claims the
/// joined width and a later `{x: 1.5}` / `{x: NaN}` stops truncating
/// through the coercible-match FpToSi (it read back 0 as a set-like
/// `size`).
pub(super) fn collect_objlit_shape_f64(
    ast: &Ast,
    a: &Analysis<'_>,
    canon_out: &HashSet<SlotKey>,
) -> HashMap<Vec<PropKey>, HashSet<PropKey>> {
    let mut objlit_shape_f64: HashMap<Vec<PropKey>, HashSet<PropKey>> = HashMap::new();
    if !a.container_poison {
        for (i, e) in ast.exprs.iter().enumerate() {
            let crate::ast::Expr::ObjectLit { fields } = e else {
                continue;
            };
            if fields.is_empty() {
                continue;
            }
            let anon = SlotKey::Anon(i as u32);
            let floats: Vec<PropKey> = fields
                .iter()
                .filter(|(fname, _)| {
                    let fk = SlotKey::Field(Box::new(anon.clone()), fname.clone());
                    canon_out.contains(&container::canon_key_frozen(&a.uf, &fk))
                })
                .map(|(fname, _)| fname.clone())
                .collect();
            if floats.is_empty() {
                continue;
            }
            let shape: Vec<PropKey> = fields.iter().map(|(n, _)| n.clone()).collect();
            objlit_shape_f64.entry(shape).or_default().extend(floats);
        }
    }
    objlit_shape_f64
}

/// Every generic `type` declaration by name, as `(type params, fields)`
/// — the shapes the alias hookups instantiate per use site.
pub(super) fn collect_generic_decls(ast: &Ast) -> GenericStructDecls {
    let mut out = HashMap::new();
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
            && !type_params.is_empty()
        {
            out.insert(name.clone(), (type_params.clone(), fields.clone()));
        }
    }
    out
}
