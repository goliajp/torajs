//! The two side tables the `analyze` driver builds around its walk —
//! carved out of `mod.rs` (file-size hard limit: the fn sat on the
//! known-debt list at 225 lines and the file at 508). Verbatim moves:
//! the slot-name registry the walk phase resolves names against, and
//! the any-face / poison seeding the fixpoint starts from.
//!
//! Rotation 507 — the registry also carries the const-int table
//! ([`record_const_int`]): the value behind an immutable binding whose
//! initializer is an integer literal, so a `const step = 3` counter
//! reads as the same small-step counter a literal `3` does.

use crate::ast::GenericStructDecls;
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
            out.insert(
                name.clone(),
                ast.generic_struct_decl(name, type_params, fields),
            );
        }
    }
    out
}

/// The two seed families that join the F64 fixpoint once the alias
/// classes are frozen: the any-face slots and, under an untrackable
/// index-assign receiver, every container point.
pub(super) fn seed_any_and_poison(a: &mut Analysis<'_>) {
    // RFC 20260708-spread-call chunk 2a — any-face slots seed the
    // F64 fixpoint. A number slot fed from the any world (an `any[]`
    // elem read into a number param, an `any` param flowing into a
    // number local) receives ToNumber(undefined) = NaN and
    // fractional f64s at runtime; an I64 repr would FpToSi-truncate
    // them (NaN → 0, silent). Conservative direction — repr cost
    // only, never correctness.
    let frozen_any: Vec<SlotKey> = a
        .any_seeds
        .iter()
        .map(|k| container::canon_key_frozen(&a.uf, k))
        .collect();
    a.seeds.extend(frozen_any);
    // An untrackable index-assign receiver poisons every container
    // query (container_walk.rs) — but that query-side short-circuit
    // reaches only the Elem/Field spellings. A SCALAR fed from a
    // container point (`let y = c.x`) resolves through `canon`, which
    // the bool never touches: the field slot lowers F64 while the
    // local stays I64, and the store between them bit-puns (the r293
    // Symbol-key SIGABRT — an Fpr value reaching
    // materialize_operand_gpr). Seed every container point in the
    // edge graph so the fixpoint carries the poison to its scalar
    // dependents, keeping both sides of the lattice consistent.
    if a.container_poison {
        let poisoned: Vec<SlotKey> = a
            .edges
            .keys()
            .filter(|k| matches!(k, SlotKey::Elem(_) | SlotKey::Field(..)))
            .cloned()
            .collect();
        a.seeds.extend(poisoned);
    }
    seed_accessor_fields(a);
}

/// A declared field an accessor definer can reach answers through its
/// getter (or a redefined `value`), and that number is any number: the
/// slot's integer lane would truncate `0.5` to `0` on the way back in.
fn seed_accessor_fields(a: &mut Analysis<'_>) {
    let names = &a.ast.accessor_field_names;
    if !names.reaches_any() {
        return;
    }
    let mut owners: Vec<&String> = a.classes.iter().chain(&a.nominal_aliases).collect();
    owners.sort();
    owners.dedup();
    let reached: Vec<SlotKey> = owners
        .into_iter()
        .filter_map(|c| Some((c, a.struct_decls.get(c)?)))
        .flat_map(|(c, fields)| {
            (fields.iter())
                .filter(|(f, _)| names.may(&f.lossy()))
                .map(|(f, _)| SlotKey::Field(Box::new(SlotKey::Class(c.clone())), f.clone()))
        })
        .map(|k| container::canon_key_frozen(&a.uf, &k))
        .collect();
    a.seeds.extend(reached);
}
