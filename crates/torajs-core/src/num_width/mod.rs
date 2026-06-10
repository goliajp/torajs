//! W1 (ann-width RFC) — module-level number-slot width inference.
//!
//! `: number` is semantically f64 (TS spec); I64 is a lowering-side
//! representation choice that is only sound when every value reaching
//! the slot is provably integral. This module is the single ground
//! truth for that decision, replacing the per-site heuristics that
//! used to disagree with each other (param widen looked only at body
//! assignments, let-init widen only at the initializer, call-site
//! args at nothing — see rfcs/20260611-ann-width-unification).
//!
//! Direction: every slot starts as an I64 narrow candidate; any
//! statically-possible f64 reaching value poisons it to F64, and
//! poison propagates through the assignment graph (slot-to-slot
//! copies, call args into params, returns into ret slots, call
//! results into bindings) to a fixpoint. A miss in the F64-seed
//! enumeration is silent-wrong (f64 bits in an i64 slot), so the
//! seed set mirrors the union of every shape the old heuristics
//! recognized, plus `-0` literals which they all missed.
//!
//! Consumers gate on the annotation themselves: only `: number` (or
//! un-annotated) slots consult this set — explicit `: i64` keeps the
//! user's narrow choice, explicit `: f64` never needs it.
//!
//! `mono.rs` carries the sibling per-call-site width hint the generic
//! monomorphizer uses BEFORE this analysis can run (the mono pass
//! creates the very FnDecls the fixpoint walks).

mod mono;
mod walk;
mod width;

pub(crate) use mono::{NumWidth, compute_typevar_widths};

use crate::ast::{Ast, ExprId, Stmt};
use std::collections::{HashMap, HashSet, VecDeque};

/// Identity of a number-typed storage slot, module-wide.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SlotKey {
    /// Top-level `let` / `const` binding (whether it later promotes to
    /// a data global or localizes into main — both consumers key here).
    Global(String),
    /// Fn-local `let` binding: (fn name, var name). Block-scoped
    /// same-name lets within one fn share a key — merging their
    /// poison is conservative (F64-ward), never wrong.
    Local(String, String),
    /// Fn parameter: (fn name, param name).
    Param(String, String),
    /// Fn return slot.
    Ret(String),
}

/// Width of an expression as seen by the analysis.
pub(super) enum W {
    /// Statically certain to be (or possibly be) a fractional /
    /// non-i64-exact f64 value.
    F64,
    /// Integral candidate whose final width depends on these slots.
    Num(Vec<SlotKey>),
    /// Not a number value (or a shape the analysis does not track —
    /// member/index reads keep their annotation-derived width, the
    /// container-width face is W4 scope).
    NotNum,
}

pub(super) fn join(a: W, b: W) -> W {
    match (a, b) {
        (W::F64, _) | (_, W::F64) => W::F64,
        (W::Num(mut d1), W::Num(d2)) => {
            d1.extend(d2);
            W::Num(d1)
        }
        (W::Num(d), W::NotNum) | (W::NotNum, W::Num(d)) => W::Num(d),
        (W::NotNum, W::NotNum) => W::NotNum,
    }
}

/// A number literal that cannot live in an i64 slot: genuinely
/// fractional, past i64 range (`n as i64` saturates), or `-0` (the
/// sign bit is meaningful f64 state that i64 zero erases — the miss
/// that aborted repro R5).
pub(super) fn literal_is_f64(n: f64) -> bool {
    n.fract() != 0.0 || n.abs() >= 9.223372036854776e18 || (n == 0.0 && n.is_sign_negative())
}

pub(super) struct Scope<'a> {
    /// Enclosing fn name; `""` = module top level (locals resolve to
    /// `Global` keys there).
    pub(super) fn_name: &'a str,
    pub(super) params: HashSet<String>,
    pub(super) locals: HashSet<String>,
}

pub(super) struct Analysis<'a> {
    pub(super) ast: &'a Ast,
    /// Per-call-site monomorphization retargets — a generic call's
    /// callee ident still spells the generic name in the AST; the
    /// edges must land on the mono instance lowering actually calls.
    pub(super) retargets: &'a HashMap<ExprId, String>,
    /// name → ordered param names, for call-arg → Param edges.
    pub(super) fn_params: HashMap<String, Vec<String>>,
    /// Top-level let/const names, for ident resolution at top level
    /// and inside named fns (named fns see top-level bindings via the
    /// data-global path).
    pub(super) toplevel_lets: HashSet<String>,
    /// Every slot key in the module sharing a given name — broadcast
    /// target for writes from closure bodies, where the defining
    /// scope of a captured name is no longer recoverable post-lift.
    /// Conservative (may poison an unrelated same-named slot), which
    /// only costs width, never correctness.
    pub(super) by_name: HashMap<String, Vec<SlotKey>>,
    pub(super) seeds: Vec<SlotKey>,
    pub(super) edges: HashMap<SlotKey, Vec<SlotKey>>,
}

/// Compute the set of slots that must take the F64 representation.
/// Call after monomorphization (the analyzed AST must be the one
/// lowering walks) with the same retarget map lowering uses.
pub(crate) fn f64_slots(ast: &Ast, retargets: &HashMap<ExprId, String>) -> HashSet<SlotKey> {
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
                for v in walk::collect_let_names_fn(stmt) {
                    by_name
                        .entry(v.clone())
                        .or_default()
                        .push(SlotKey::Local(name.clone(), v));
                }
            }
            Stmt::LetDecl { name, .. } => {
                toplevel_lets.insert(name.clone());
                by_name
                    .entry(name.clone())
                    .or_default()
                    .push(SlotKey::Global(name.clone()));
            }
            _ => {}
        }
    }

    let mut a = Analysis {
        ast,
        retargets,
        fn_params,
        toplevel_lets,
        by_name,
        seeds: Vec::new(),
        edges: HashMap::new(),
    };

    // Top-level statements walk under the "" scope; fn bodies under
    // their own. Synthetic fns (`__closure_*` / `__cm_*` …) still walk
    // — their bodies can write captured outer slots — but their own
    // params / rets never enter the consumer guard (sites skip `__`).
    let top_scope = Scope {
        fn_name: "",
        params: HashSet::new(),
        locals: HashSet::new(),
    };
    for stmt in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = stmt
        {
            let scope = Scope {
                fn_name: name,
                params: params.iter().map(|p| p.name.clone()).collect(),
                locals: {
                    let mut s = HashSet::new();
                    for b in body {
                        walk::collect_let_names(b, &mut s);
                    }
                    s
                },
            };
            for b in body {
                a.walk_stmt(b, &scope);
            }
        } else {
            a.walk_stmt(stmt, &top_scope);
        }
    }

    // Fixpoint: poison flows forward along assignment edges until
    // stable. Monotone single-direction lattice — O(edges).
    let mut out: HashSet<SlotKey> = HashSet::new();
    let mut work: VecDeque<SlotKey> = a.seeds.into_iter().collect();
    while let Some(k) = work.pop_front() {
        if out.insert(k.clone())
            && let Some(dsts) = a.edges.get(&k)
        {
            for d in dsts {
                if !out.contains(d) {
                    work.push_back(d.clone());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    fn slots(src: &str) -> HashSet<SlotKey> {
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(&tokens).expect("parse");
        f64_slots(&ast, &HashMap::new())
    }

    fn local(f: &str, v: &str) -> SlotKey {
        SlotKey::Local(f.into(), v.into())
    }
    fn param(f: &str, p: &str) -> SlotKey {
        SlotKey::Param(f.into(), p.into())
    }
    fn ret(f: &str) -> SlotKey {
        SlotKey::Ret(f.into())
    }

    #[test]
    fn r1_fract_return_poisons_ret() {
        let s = slots("function f(): number { return 0.5; }\nconsole.log(f());");
        assert!(s.contains(&ret("f")));
    }

    #[test]
    fn r2_later_f64_assign_poisons_let_and_ret() {
        let s = slots(
            "function f(): number {\n  let acc: number = 0;\n  acc = acc + 0.5;\n  return acc;\n}\nconsole.log(f());",
        );
        assert!(s.contains(&local("f", "acc")));
        assert!(s.contains(&ret("f")));
    }

    #[test]
    fn r3_div_poisons_loop_cell_param_stays_int() {
        let s = slots(
            "function f(x: number): number {\n  let n: number = x;\n  while (n % 2 === 0) { n = n / 2; }\n  return n;\n}\nconsole.log(f(12));",
        );
        assert!(s.contains(&local("f", "n")));
        assert!(s.contains(&ret("f")));
        assert!(!s.contains(&param("f", "x")));
    }

    #[test]
    fn s1_callsite_fract_arg_poisons_param() {
        let s = slots("function g(x: number): number { return x + 1; }\nconsole.log(g(0.5));");
        assert!(s.contains(&param("g", "x")));
        assert!(s.contains(&ret("g")));
    }

    #[test]
    fn s2_callsite_slot_arg_propagates() {
        let s = slots(
            "function g(x: number): number { return x; }\nlet v: number = 2.5;\nconsole.log(g(v));",
        );
        assert!(s.contains(&SlotKey::Global("v".into())));
        assert!(s.contains(&param("g", "x")));
        assert!(s.contains(&ret("g")));
    }

    #[test]
    fn s5_ret_then_div_poisons_binding_not_callee() {
        let s = slots(
            "function h(): number { return 7; }\nlet q: number = h();\nq = q / 4;\nconsole.log(q);",
        );
        assert!(s.contains(&SlotKey::Global("q".into())));
        assert!(!s.contains(&ret("h")));
    }

    #[test]
    fn neg_zero_literal_is_f64_seed() {
        let s = slots(
            "function signOf(z: number): number { return 1 / z; }\nconst mz: number = -0;\nconsole.log(signOf(mz));",
        );
        assert!(s.contains(&SlotKey::Global("mz".into())));
        assert!(s.contains(&param("signOf", "z")));
    }

    #[test]
    fn int_domain_stays_narrow() {
        let s = slots(
            "function popcount(x: number): number {\n  let n: number = x;\n  let count: number = 0;\n  while (n !== 0) { n = n & (n - 1); count = count + 1; }\n  return count;\n}\nconsole.log(popcount(9999999));",
        );
        assert!(s.is_empty());
    }

    #[test]
    fn srem_int_modulo_stays_narrow() {
        let s = slots(
            "function gcd(a: number, b: number): number {\n  while (b !== 0) {\n    let t: number = b;\n    b = a % b;\n    a = t;\n  }\n  return a;\n}\nconsole.log(gcd(48, 18));",
        );
        assert!(s.is_empty());
    }

    #[test]
    fn bitwise_firewall_blocks_poison() {
        let s = slots("function f(x: number): number { return (x / 2) | 0; }\nconsole.log(f(7));");
        assert!(!s.contains(&ret("f")));
    }
}
