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

mod container;
mod container_walk;
mod cycle;
mod mono;
mod walk;
mod width;

pub(crate) use container::WidthTable;
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
    /// W4 — nominal aggregation point for a class: every instance of
    /// `class C` shares one struct layout at lowering, so field-width
    /// decisions join over all instances. `__new_C`'s ret and every
    /// `__cm_C__*` method's `__this` param union into this key.
    Class(String),
    /// W4 — captured outer binding referenced from a lifted closure
    /// body, where the defining scope is no longer recoverable. All
    /// same-named slots union through this key (conservative merge —
    /// width-only cost, mirrors the scalar by_name broadcast).
    Captured(String),
    /// W4 — anonymous container origin (array / object literal, or a
    /// transform-method result), keyed by the originating ExprId.
    Anon(u32),
    /// W4 — element-width point of the container alias class whose
    /// representative is the inner key. `number[]` elems narrow to I64
    /// only when every write reaching any alias of the array is
    /// provably integral.
    Elem(Box<SlotKey>),
    /// W4 — field-width point of a struct / class / inline-object
    /// alias class: (container class representative, field name).
    Field(Box<SlotKey>, String),
}

/// A slot dependency plus whether the value path from that slot to
/// the consuming write runs through a growth op (Mul, or Add/Sub with
/// a non-constant increment). A growth edge inside an assignment-graph
/// cycle means the slot's value can grow without bound across
/// iterations — integral, yet not i64-safe (W5): past 2^53 the f64
/// semantics round where i64 keeps exact / wraps, so the slot must
/// stay F64 and let guarded demotion (1b-ii) pull it back.
pub(super) type Dep = (SlotKey, bool);

/// Width of an expression as seen by the analysis.
pub(super) enum W {
    /// Statically certain to be (or possibly be) a fractional /
    /// non-i64-exact f64 value.
    F64,
    /// Integral candidate whose final width depends on these slots.
    Num(Vec<Dep>),
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
    pub(super) edges: HashMap<SlotKey, Vec<Dep>>,
    /// W4 — container alias classes. Unconditional unions (element
    /// writes, literal/method plumbing, nominal class hookups) land
    /// here directly during the walk.
    pub(super) uf: container::UnionFind,
    /// W4 — guarded alias edges from plain value copies (`let ys =
    /// xs`, call args, returns). Activated post-walk only when either
    /// endpoint shows container evidence, so scalar copy chains
    /// (`let v = xs[0]; v = v / 2`) don't glue the copy's width back
    /// onto the source class.
    pub(super) guarded_unions: Vec<(SlotKey, SlotKey)>,
    /// W4 — slots with container evidence (write receivers, literal
    /// origins, class keys); the activation worklist grows it.
    pub(super) containerish: HashSet<SlotKey>,
    /// Class names post-desugar (`ast.class_parents` keys), sorted for
    /// deterministic union order.
    pub(super) classes: Vec<String>,
    /// W4 — an element write through a receiver the analysis cannot
    /// resolve to a container class. Never expected to fire (assign
    /// receivers are idents / members / indexes / calls); if it does,
    /// every elem/field query answers F64 — conservative, loud in
    /// STATS, never silent-wrong.
    pub(super) container_poison: bool,
}

/// Compute the set of slots that must take the F64 representation.
/// Call after monomorphization (the analyzed AST must be the one
/// lowering walks) with the same retarget map lowering uses.
pub(crate) fn f64_slots(ast: &Ast, retargets: &HashMap<ExprId, String>) -> HashSet<SlotKey> {
    analyze(ast, retargets).into_f64_set()
}

/// Full analysis result — the scalar F64 slot set plus the container
/// alias classes that answer elem/field width queries (W4).
pub(crate) fn analyze(ast: &Ast, retargets: &HashMap<ExprId, String>) -> WidthTable {
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

    let mut classes: Vec<String> = ast.class_parents.keys().cloned().collect();
    classes.sort();

    let mut a = Analysis {
        ast,
        retargets,
        fn_params,
        toplevel_lets,
        by_name,
        seeds: Vec::new(),
        edges: HashMap::new(),
        uf: container::UnionFind::default(),
        guarded_unions: Vec::new(),
        containerish: HashSet::new(),
        classes,
        container_poison: false,
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

    // W4 container pipeline: nominal class hookups, guarded-union
    // activation, congruence closure, then rewrite of every seed /
    // edge key onto its alias-class representative.
    container::nominal_unions(&mut a);
    container::activate_guarded(&mut a);
    let raw_keys = container::canonicalize(&mut a);

    // W5: an assignment-graph cycle through a growth edge is an
    // unbounded self-feeding loop — integral yet not i64-safe. Seed
    // every slot on such a cycle before the poison fixpoint.
    cycle::seed_growth_cycles(&a.edges, &mut a.seeds);

    // Fixpoint: poison flows forward along assignment edges until
    // stable. Monotone single-direction lattice — O(edges).
    let mut out: HashSet<SlotKey> = HashSet::new();
    let mut work: VecDeque<SlotKey> = a.seeds.into_iter().collect();
    while let Some(k) = work.pop_front() {
        if out.insert(k.clone())
            && let Some(dsts) = a.edges.get(&k)
        {
            for (d, _) in dsts {
                if !out.contains(d) {
                    work.push_back(d.clone());
                }
            }
        }
    }

    // Re-expand onto raw (pre-canonicalization) spellings so pre-W4
    // consumers querying uncanonicalized slot keys keep seeing every
    // poisoned slot.
    let expanded: Vec<SlotKey> = raw_keys
        .iter()
        .filter(|k| out.contains(&container::canon_key(&mut a.uf, k)))
        .cloned()
        .collect();
    out.extend(expanded);

    WidthTable::new(out, a.uf, a.container_poison)
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
    fn w3_modulo_variable_dividend_seeds_f64() {
        // W3 — `a % b` can mint -0 (negative dividend, zero
        // remainder); the %-fed slot chain must stay f64. The SSA
        // float_demote narrows the proven-non-negative hot loop back.
        let s = slots(
            "function gcd(a: number, b: number): number {\n  while (b !== 0) {\n    let t: number = b;\n    b = a % b;\n    a = t;\n  }\n  return a;\n}\nconsole.log(gcd(48, 18));",
        );
        assert!(s.contains(&param("gcd", "b")));
        assert!(s.contains(&param("gcd", "a")));
        assert!(s.contains(&local("gcd", "t")));
        assert!(s.contains(&ret("gcd")));
    }

    #[test]
    fn w3_modulo_nonneg_const_dividend_stays_narrow() {
        // a non-negative integer-literal dividend bounds the
        // remainder in [+0, |b|) — no -0, int path keeps.
        let s =
            slots("function wrap(k: number): number { return 100 % k; }\nconsole.log(wrap(7));");
        assert!(s.is_empty());
    }

    #[test]
    fn bitwise_firewall_blocks_poison() {
        let s = slots("function f(x: number): number { return (x / 2) | 0; }\nconsole.log(f(7));");
        assert!(!s.contains(&ret("f")));
    }

    #[test]
    fn w5_s7_growth_cycle_poisons_loop_cell() {
        let s = slots(
            "function grow(start: number, steps: number): number {\n  let n: number = start;\n  let i: number = 0;\n  while (i < steps) { n = n * 3 + 1; i = i + 1; }\n  return n;\n}\nconsole.log(grow(1, 40));",
        );
        assert!(s.contains(&local("grow", "n")));
        assert!(s.contains(&ret("grow")));
        // the small-const counter and the unlooped param stay narrow
        assert!(!s.contains(&local("grow", "i")));
        assert!(!s.contains(&param("grow", "steps")));
    }

    #[test]
    fn w5_cross_slot_growth_cycle_poisons_both() {
        let s = slots(
            "function f(k: number): number {\n  let n: number = 1;\n  let t: number = 0;\n  let i: number = 0;\n  while (i < k) { t = n * 3; n = t + 1; i = i + 1; }\n  return n;\n}\nconsole.log(f(10));",
        );
        assert!(s.contains(&local("f", "n")));
        assert!(s.contains(&local("f", "t")));
        assert!(!s.contains(&local("f", "i")));
    }

    #[test]
    fn w5_recursive_growth_cycle_poisons_ret() {
        let s = slots(
            "function fib(n: number): number {\n  if (n < 2) { return n; }\n  return fib(n - 1) + fib(n - 2);\n}\nconsole.log(fib(40));",
        );
        assert!(s.contains(&ret("fib")));
    }

    #[test]
    fn w5_accumulator_with_nonconst_step_poisons() {
        let s = slots(
            "function sum(k: number, c: number): number {\n  let acc: number = 0;\n  let i: number = 0;\n  while (i < k) { acc = acc + c; i = i + 1; }\n  return acc;\n}\nconsole.log(sum(10, 5));",
        );
        assert!(s.contains(&local("sum", "acc")));
        assert!(!s.contains(&local("sum", "i")));
    }

    #[test]
    fn w5_straightline_mul_no_cycle_stays_narrow() {
        let s = slots(
            "function area(w: number, h: number): number {\n  let a: number = w * h;\n  return a;\n}\nconsole.log(area(3, 4));",
        );
        assert!(s.is_empty());
    }
}
