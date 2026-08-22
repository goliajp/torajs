//! The two side tables the `analyze` driver builds around its walk —
//! carved out of `mod.rs` (file-size hard limit: the fn sat on the
//! known-debt list at 225 lines and the file at 508). Verbatim moves:
//! the slot-name registry the walk phase resolves names against, and
//! the W4 objlit shape-join table the frozen verdict set feeds.

use std::collections::{HashMap, HashSet};

use super::{Analysis, SlotKey, container, let_names};
use crate::ast::{Ast, Stmt};

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
);

pub(super) fn collect_slot_registry(ast: &Ast) -> SlotRegistry {
    let mut fn_params: HashMap<String, Vec<String>> = HashMap::new();
    let mut toplevel_lets: HashSet<String> = HashSet::new();
    let mut by_name: HashMap<String, Vec<SlotKey>> = HashMap::new();
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
                for v in let_names::collect_let_names_fn(stmt) {
                    by_name
                        .entry(v.clone())
                        .or_default()
                        .push(SlotKey::Local(name.clone(), v));
                }
            }
            other => {
                let mut names = HashSet::new();
                let_names::collect_let_names(other, &mut names);
                for name in names {
                    toplevel_lets.insert(name.clone());
                    by_name
                        .entry(name.clone())
                        .or_default()
                        .push(SlotKey::Global(name.clone()));
                }
            }
        }
    }
    (fn_params, toplevel_lets, by_name)
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
) -> HashMap<Vec<String>, HashSet<String>> {
    let mut objlit_shape_f64: HashMap<Vec<String>, HashSet<String>> = HashMap::new();
    if !a.container_poison {
        for (i, e) in ast.exprs.iter().enumerate() {
            let crate::ast::Expr::ObjectLit { fields } = e else {
                continue;
            };
            if fields.is_empty() {
                continue;
            }
            let anon = SlotKey::Anon(i as u32);
            let floats: Vec<String> = fields
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
            let shape: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
            objlit_shape_f64.entry(shape).or_default().extend(floats);
        }
    }
    objlit_shape_f64
}
