//! Construction-site capture-annotation snapshots for lifted closures.
//!
//! `desugar_implicit_generics` infers a lifted closure's return ann by
//! statically sniffing its body; captured idents used to resolve
//! through the program-wide by-name `outer_binds` table, where the
//! LAST-SEEN annotation for a name wins. Two same-named bindings in
//! unrelated scopes (`function viaParam(x: number)` capturing `x` vs a
//! later `isErr(x: any)`) made the sniff read the WRONG scope's ann —
//! the closure's ret ann became `any`, its lowered ABI drifted from
//! the `() => number` fn-type annotation pinned at the binding site,
//! and the call site read garbage bits (the num-width Captured
//! broadcast poison, tasks/2026-07-18).
//!
//! This pass recovers the defining scope instead: walk every fn body
//! with an in-order binds map (params + let anns, fn-level precision)
//! and, at each `Expr::Closure { fn_name, captures }` construction
//! site, snapshot the anns its captures resolve to RIGHT THERE. The
//! sniff then consults the snapshot first and falls back to
//! `outer_binds` only for names the snapshot misses.
//!
//! Lifted closure bodies (`__env`-first FnDecls) themselves contain
//! construction sites of nested closures; their captures' anns come
//! from their OWN snapshot, so those bodies walk in dependency waves
//! (a body walks only once its snapshot exists). Bodies whose
//! construction site was never reached walk last with params-only
//! binds — their nested sites degrade to the old fallback, never
//! worse than pre-pass behavior.

use super::{AstExprsView, Expr, ExprId, Param, Stmt, binds_to_params, infer_expr_ann_with};
use std::collections::{HashMap, HashSet};

pub(crate) type Binds = HashMap<String, String>;

/// Construction-site annotation snapshots, keyed two ways:
/// - `closures`: closure fn_name → (capture name → ann at the site).
/// - `objlits`: method-carrying ObjectLit ExprId → the full binds map
///   at its site. `objlit_nominal` types the literal's DATA fields to
///   mint the `__ObjLit_n` layout; resolving a field-value ident
///   through the by-name fallback picked the wrong scope's ann just
///   like the closure ret sniff did (`{ px: x }` under `x: number`
///   laid the slot out as `any` when a later fn spelled `x: any` —
///   struct fill then wrote raw bits the any-lane reader crashed on).
pub(crate) struct SiteAnns {
    pub(crate) closures: HashMap<String, Binds>,
    pub(crate) objlits: HashMap<u32, Binds>,
}

pub(crate) fn collect_closure_capture_anns(
    stmts: &[Stmt],
    exprs: AstExprsView,
    fn_sigs: &HashMap<String, String>,
    objlit_method_exprs: &HashSet<ExprId>,
) -> SiteAnns {
    let mut ctx = Ctx {
        exprs,
        fn_sigs,
        objlit_method_exprs,
        snapshots: SiteAnns {
            closures: HashMap::new(),
            objlits: HashMap::new(),
        },
    };

    // Top-level walk: in-order binds over top-level lets; construction
    // sites in top-level expressions snapshot against the prefix seen
    // so far (a closure can only capture bindings declared before it).
    let mut top_binds: Binds = HashMap::new();
    for s in stmts {
        if !matches!(s, Stmt::FnDecl { .. }) {
            ctx.walk_stmt(s, &mut top_binds);
        }
    }

    // Wave 1 — every non-`__env`-first fn body (user fns, class
    // methods, generator shells): binds = final top lets + own params.
    let mut env_first: Vec<(&String, &Vec<Param>, &Vec<Stmt>)> = Vec::new();
    for s in stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = s
        else {
            continue;
        };
        if params.first().is_some_and(|p| p.name == "__env") {
            env_first.push((name, params, body));
            continue;
        }
        let mut binds = top_binds.clone();
        seed_param_anns(&mut binds, params);
        for b in body {
            ctx.walk_stmt(b, &mut binds);
        }
    }

    // Waves 2..n — `__env`-first bodies whose snapshot now exists seed
    // their captures' anns from it; iterate until no progress (each
    // wave can reveal the next nesting level's construction sites).
    let mut walked: Vec<bool> = vec![false; env_first.len()];
    loop {
        let mut progressed = false;
        for (i, (name, params, body)) in env_first.iter().enumerate() {
            if walked[i] || !ctx.snapshots.closures.contains_key(*name) {
                continue;
            }
            walked[i] = true;
            progressed = true;
            let mut binds = top_binds.clone();
            if let Some(snap) = ctx.snapshots.closures.get(*name) {
                binds.extend(snap.clone());
            }
            seed_param_anns(&mut binds, params);
            for b in *body {
                ctx.walk_stmt(b, &mut binds);
            }
        }
        if !progressed {
            break;
        }
    }
    // Unreached bodies (construction site in a shape the walkers skip)
    // still walk so their nested sites get params + top-level precision.
    for (i, (_, params, body)) in env_first.iter().enumerate() {
        if walked[i] {
            continue;
        }
        let mut binds = top_binds.clone();
        seed_param_anns(&mut binds, params);
        for b in *body {
            ctx.walk_stmt(b, &mut binds);
        }
    }

    ctx.snapshots
}

fn seed_param_anns(binds: &mut Binds, params: &[Param]) {
    for p in params {
        if p.name == "__env" {
            continue;
        }
        if let Some(ann) = &p.type_ann {
            binds.insert(p.name.clone(), ann.clone());
        }
    }
}

struct Ctx<'a> {
    exprs: AstExprsView<'a>,
    fn_sigs: &'a HashMap<String, String>,
    objlit_method_exprs: &'a HashSet<ExprId>,
    snapshots: SiteAnns,
}

impl<'a> Ctx<'a> {
    /// In-order statement walk. Fn-level binding precision: block-
    /// scoped lets stay visible after their block (same conservative
    /// granularity as the num-width Local key).
    fn walk_stmt(&mut self, s: &Stmt, binds: &mut Binds) {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                self.walk_expr(*init, binds);
                if let Some(ann) = type_ann {
                    binds.insert(name.clone(), ann.clone());
                } else {
                    let bs: Vec<Param> = binds_to_params(binds);
                    if let Some(ann) =
                        infer_expr_ann_with(self.exprs, *init, &bs, binds, self.fn_sigs)
                    {
                        binds.insert(name.clone(), ann);
                    }
                }
            }
            Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => self.walk_expr(*eid, binds),
            Stmt::YieldInto { value, .. } => self.walk_expr(*value, binds),
            Stmt::Return(Some(eid)) => self.walk_expr(*eid, binds),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond, binds);
                self.walk_stmt(then_branch, binds);
                if let Some(eb) = else_branch {
                    self.walk_stmt(eb, binds);
                }
            }
            Stmt::Labeled { body, .. } => {
                self.walk_stmt(body, binds);
            }
            Stmt::While { cond, body } => {
                self.walk_expr(*cond, binds);
                self.walk_stmt(body, binds);
            }
            Stmt::DoWhile { body, cond } => {
                self.walk_stmt(body, binds);
                self.walk_expr(*cond, binds);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(i) = init {
                    self.walk_stmt(i, binds);
                }
                if let Some(c) = cond {
                    self.walk_expr(*c, binds);
                }
                if let Some(st) = step {
                    self.walk_expr(*st, binds);
                }
                self.walk_stmt(body, binds);
            }
            Stmt::ForOf {
                var_name,
                elem_expr,
                body,
                ..
            } => {
                self.walk_expr(*elem_expr, binds);
                binds.remove(var_name);
                self.walk_stmt(body, binds);
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.walk_expr(*parent, binds);
                self.walk_expr(*sep, binds);
                binds.remove(var_name);
                self.walk_stmt(body, binds);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                self.walk_expr(*scrutinee, binds);
                for c in cases {
                    self.walk_expr(c.value, binds);
                    for cs in &c.body {
                        self.walk_stmt(cs, binds);
                    }
                }
                if let Some(d) = default {
                    for ds in d {
                        self.walk_stmt(ds, binds);
                    }
                }
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                for bs in body {
                    self.walk_stmt(bs, binds);
                }
                if let Some(cp) = catch_param {
                    binds.remove(cp);
                }
                for cs in catch_body {
                    self.walk_stmt(cs, binds);
                }
                if let Some(fb) = finally_body {
                    for fs in fb {
                        self.walk_stmt(fs, binds);
                    }
                }
            }
            Stmt::Block(v) | Stmt::Multi(v) => {
                for bs in v {
                    self.walk_stmt(bs, binds);
                }
            }
            Stmt::ExportDecl {
                inner,
                default_expr,
                ..
            } => {
                if let Some(i) = inner {
                    self.walk_stmt(i, binds);
                }
                if let Some(de) = default_expr {
                    self.walk_expr(*de, binds);
                }
            }
            _ => {}
        }
    }

    fn walk_expr(&mut self, eid: ExprId, binds: &Binds) {
        let exprs: AstExprsView<'a> = self.exprs;
        match &exprs[eid.0 as usize] {
            Expr::Closure { fn_name, captures } => {
                let snap: Binds = captures
                    .iter()
                    .filter_map(|c| binds.get(c).map(|a| (c.clone(), a.clone())))
                    .collect();
                // First construction site wins (one placeholder per
                // lifted arrow; guards a hypothetical duplicate).
                self.snapshots
                    .closures
                    .entry(fn_name.clone())
                    .or_insert(snap);
            }
            Expr::BinOp { left, right, .. }
            | Expr::Sequence { left, right }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.walk_expr(*left, binds);
                self.walk_expr(*right, binds);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Delete { expr }
            | Expr::Spread { expr }
            | Expr::As { expr, .. } => self.walk_expr(*expr, binds),
            Expr::InstanceOf { expr, rhs } => {
                self.walk_expr(*expr, binds);
                self.walk_expr(*rhs, binds);
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => self.walk_expr(*obj, binds),
            Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
                self.walk_expr(*obj, binds);
                self.walk_expr(*index, binds);
            }
            Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
                self.walk_expr(*callee, binds);
                for arg in args {
                    self.walk_expr(*arg, binds);
                }
            }
            Expr::Assign { target, value } => {
                self.walk_expr(*target, binds);
                self.walk_expr(*value, binds);
            }
            Expr::Array(elems) => {
                for e in elems {
                    self.walk_expr(*e, binds);
                }
            }
            Expr::ObjectLit { fields } => {
                // A method-carrying literal gets a full binds snapshot:
                // `objlit_nominal` sniffs its data-field anns to mint
                // the `__ObjLit_n` layout and must resolve field-value
                // idents in THIS scope, not the by-name fallback.
                if fields
                    .iter()
                    .any(|(_, e)| self.objlit_method_exprs.contains(e))
                {
                    self.snapshots
                        .objlits
                        .entry(eid.0)
                        .or_insert_with(|| binds.clone());
                }
                for (_, e) in fields {
                    self.walk_expr(*e, binds);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond, binds);
                self.walk_expr(*then_branch, binds);
                self.walk_expr(*else_branch, binds);
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    self.walk_expr(*a, binds);
                }
            }
            Expr::PostIncr { target, .. } => self.walk_expr(*target, binds),
            // Pre-lift arrows don't survive to this pass; if one does,
            // its body is its own scope — skip (construction sites
            // inside will take the outer_binds fallback).
            _ => {}
        }
    }
}
