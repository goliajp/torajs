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

mod alias;
mod analyze_tables;
mod bounds_walk;
mod container;
mod container_lookup;
mod container_methods;
mod container_result_key;
mod container_walk;
mod cycle;
mod escape;
mod fallthrough;
mod fnsig;
mod json_seed;
mod let_names;
mod mono;
mod slot_abi;
mod walk;
mod width;

pub(crate) use container::WidthTable;
pub(crate) use fnsig::{fn_type_canon, split_fn_type};
pub(crate) use mono::{NumWidth, compute_typevar_widths};

use crate::ast::{Ast, ExprId, Stmt};
use fallthrough::{alias_fallthrough_closures, collect_undef_sentinel_params, seed_and_walk_fn};
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
    /// RFC 20260726-array-elem-width — the pending-throw value slot.
    ///
    /// There is one of it in the runtime, and it holds raw 8 bytes that
    /// a `throw` writes and a `catch` binding reads back, so both ends
    /// have to agree on how to read those bits — the same reason a
    /// promise's `value` point exists. Nothing joined them before, so
    /// `throw xs[i]` on a widened array wrote f64 bits that
    /// `catch (e: number)` read as an integer.
    ///
    /// Being a single point means every numeric throw site in a program
    /// shares one class. That is the conservative direction: a class
    /// merges F64-ward, and a JS number IS an f64, so the worst case is
    /// a slot that stays wider than it had to.
    Thrown,
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
    /// The checker's verdict per expression. Read only where the
    /// width a value arrives with depends on how lowering will
    /// materialize it rather than on where it came from — see the
    /// `Expr::As` arm in `width.rs`.
    pub(super) expr_types: &'a HashMap<ExprId, crate::check::Type>,
    /// Per-call-site monomorphization retargets — a generic call's
    /// callee ident still spells the generic name in the AST; the
    /// edges must land on the mono instance lowering actually calls.
    pub(super) retargets: &'a HashMap<ExprId, String>,
    /// Demoted speculative class-method rewrites (check.rs proved the
    /// receiver is a builtin container; ssa_lower already restored
    /// the member-call shape at these ExprIds). The `get` arm's
    /// `any_class_owns_method` gate is bypassed for them — receiver
    /// identity is typed evidence, not a name guess.
    pub(super) demoted: &'a HashMap<ExprId, ExprId>,
    /// name → ordered param names, for call-arg → Param edges.
    pub(super) fn_params: HashMap<String, Vec<String>>,
    /// Top-level let/const names, for ident resolution at top level
    /// and inside named fns (named fns see top-level bindings via the
    /// data-global path).
    pub(super) toplevel_lets: HashSet<String>,
    /// Rotation 507 — the integer literal behind an immutable
    /// binding, keyed by the slot it resolves to. `width.rs`'s
    /// counter carve-out reads it so `const step = 3; t += step`
    /// stays the small-step counter the literal form is (506-06);
    /// the magnitude rule applies to both spellings alike.
    pub(super) const_ints: HashMap<SlotKey, f64>,
    /// Every slot key in the module sharing a given name — broadcast
    /// target for writes from closure bodies, where the defining
    /// scope of a captured name is no longer recoverable post-lift.
    /// Conservative (may poison an unrelated same-named slot), which
    /// only costs width, never correctness.
    pub(super) by_name: HashMap<String, Vec<SlotKey>>,
    pub(super) seeds: Vec<SlotKey>,
    pub(super) edges: HashMap<SlotKey, Vec<Dep>>,
    /// W4 — container-channel constraints (element / field writes,
    /// callback wiring through transform methods). Kept out of the
    /// W1 seeds/edges so the scalar fixpoint pre-W4 consumers read
    /// stays bit-identical; merged in only for the canonical fixpoint
    /// the D2/D3 lowering wiring will consume.
    pub(super) c_seeds: Vec<SlotKey>,
    /// W-ESC — any-annotated slot keys collected during the walk
    /// (escape sinks; see escape.rs).
    pub(super) any_seeds: Vec<SlotKey>,
    pub(super) c_edges: HashMap<SlotKey, Vec<Dep>>,
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
    /// F2-fix — nested-container alias edges: (element point,
    /// candidate). `ys.push(xs)` makes xs alias ys's element class,
    /// and a map-family callback's param aliases the receiver's
    /// elements — but ONLY when the flowing value is itself a
    /// container. Activation requires evidence on the CANDIDATE side
    /// (the element side is trivially containerish, so the guarded
    /// either-endpoint rule would always fire and glue scalars into
    /// the element class — `xs.push(k)` + `xs.push(k*10)` closed a
    /// growth cycle through Param(k) and floated int arrays).
    pub(super) nested_unions: Vec<(SlotKey, SlotKey)>,
    /// W4 — slots with container evidence (write receivers, literal
    /// origins, class keys); the activation worklist grows it.
    pub(super) containerish: HashSet<SlotKey>,
    /// Class names post-desugar (`ast.class_parents` keys), sorted for
    /// deterministic union order.
    pub(super) classes: Vec<String>,
    /// D5 — plain alias names on a cycle in the named-type reference
    /// graph. Their field widths join at type granularity through
    /// `SlotKey::Class` (annotation-driven hookups in `alias.rs`);
    /// lowering's TypeDecl fill site queries the same set to take
    /// nominal widths for them. Generic instantiation keys
    /// (`Rec<number>`) register here lazily on first annotation
    /// sighting (`alias.rs::register_inst_key`) — their single
    /// memo-reserved layout is shared by every consuming slot, so
    /// widths must join nominally or they'd be lowering-order
    /// sensitive.
    pub(super) nominal_aliases: HashSet<String>,
    /// Generic TypeDecl bodies by name (`Rec` → (["T"], fields)),
    /// for the lazy instantiation-key registration above.
    pub(super) generic_decls: HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    /// W4 — an element write through a receiver the analysis cannot
    /// resolve to a container class. Never expected to fire (assign
    /// receivers are idents / members / indexes / calls); if it does,
    /// every elem/field query answers F64 — conservative, loud in
    /// STATS, never silent-wrong.
    pub(super) container_poison: bool,
    /// The `(i, xs)` guard pairs standing at the walk's current point
    /// — pushed by a loop whose condition is `i < xs.length`, evicted
    /// by a statement that taints them, popped at the loop's end. See
    /// [`crate::ssa_lower_bounds_proven`] for what taints one.
    pub(super) bounds_stack: Vec<(String, String)>,
    /// Where that stack held while walking an index read: the reads
    /// proven in-bounds. Handed to the frozen table, because the
    /// element widths below are only sound under this proof (the
    /// field's doc on `WidthTable` says why).
    pub(super) proven_reads: HashSet<ExprId>,
}

/// Every generic `type` declaration by name, as `(type params, fields)`
/// — the shapes the alias hookups instantiate per use site.
fn collect_generic_decls(ast: &Ast) -> HashMap<String, (Vec<String>, Vec<(String, String)>)> {
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

/// Full analysis result — the F64 slot classes plus the container
/// alias classes that answer elem/field width queries (W4). Call
/// after monomorphization (the analyzed AST must be the one lowering
/// walks) with the same retarget map lowering uses.
pub(crate) fn analyze(
    ast: &Ast,
    retargets: &HashMap<ExprId, String>,
    demoted: &HashMap<ExprId, ExprId>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
) -> WidthTable {
    // Pre-walk slot-name registry (doc on the collector — sibling
    // `analyze_tables.rs`).
    let (fn_params, toplevel_lets, by_name, const_ints) =
        analyze_tables::collect_slot_registry(ast);

    let mut classes: Vec<String> = ast.class_parents.keys().cloned().collect();
    classes.sort();
    let nominal_aliases = alias::nominal_alias_names(ast);
    let generic_decls = collect_generic_decls(ast);

    let mut a = Analysis {
        ast,
        expr_types,
        retargets,
        demoted,
        fn_params,
        toplevel_lets,
        const_ints,
        by_name,
        seeds: Vec::new(),
        edges: HashMap::new(),
        c_seeds: Vec::new(),
        c_edges: HashMap::new(),
        any_seeds: Vec::new(),
        uf: container::UnionFind::default(),
        guarded_unions: Vec::new(),
        nested_unions: Vec::new(),
        containerish: HashSet::new(),
        classes,
        nominal_aliases,
        generic_decls,
        container_poison: false,
        bounds_stack: Vec::new(),
        proven_reads: HashSet::new(),
    };

    // Top-level statements walk under the "" scope; fn bodies under
    // their own. Synthetic fns (`__closure_*` / `__cm_*` …) walk like
    // user fns, and since §5.6 F2 their Param / Ret keys feed the
    // lowering consumer sites too — a synthetic fn whose body returns
    // an f64-possible value gets a genuinely-F64 ABI instead of the
    // old `: number`-pinned I64 (the FpToSi truncation face).
    let top_scope = Scope {
        fn_name: "",
        params: HashSet::new(),
        locals: HashSet::new(),
    };
    // Ahead of the walk: the fall-through table below asks whether a
    // body hands the sentinel back, and returning a parameter that a
    // call site tainted is one of the ways it can.
    let undef_sentinel_params = collect_undef_sentinel_params(&a);
    let mut fallthrough_fns: HashSet<String> = HashSet::new();
    for stmt in &ast.stmts {
        if let Stmt::FnDecl { .. } = stmt {
            seed_and_walk_fn(&mut a, stmt, &undef_sentinel_params, &mut fallthrough_fns);
        } else {
            a.walk_stmt(stmt, &top_scope);
        }
    }

    alias_fallthrough_closures(ast, &mut fallthrough_fns);

    // W4 container pipeline: nominal class hookups, guarded-union
    // activation, container-channel merge, congruence closure, then
    // rewrite of every seed / edge key onto its alias-class
    // representative. The fixpoint runs per alias class; queries
    // canonicalize through the frozen union-find.
    container::nominal_unions(&mut a);
    container::objlit_ctor_unions(&mut a);
    slot_abi::dispatch_unions(&mut a);
    a.alias_nominal_unions();
    a.fnsig_nominal_unions();
    container::activate_guarded(&mut a);
    a.seeds.append(&mut a.c_seeds);
    for (d, ts) in std::mem::take(&mut a.c_edges) {
        a.edges.entry(d).or_default().extend(ts);
    }
    container::canonicalize(&mut a);
    cycle::seed_growth_cycles(&a.edges, &mut a.seeds);
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
    let canon_out = fixpoint(std::mem::take(&mut a.seeds), &a.edges);

    // TORAJS_NUM_WIDTH_STATS=1 — dump the canonical F64 class set
    // (one line per poisoned representative), the same diagnostic
    // shape as the SSA-pass *_STATS switches.
    if std::env::var_os("TORAJS_NUM_WIDTH_STATS").is_some() {
        let mut lines: Vec<String> = canon_out.iter().map(|k| format!("{k:?}")).collect();
        lines.sort();
        for l in &lines {
            eprintln!("[num_width] f64 class: {l}");
        }
    }

    // W-ESC — escape faces resolve to frozen class reps (+ Elem-chain
    // contagion for nested containers).
    let any_escaped = escape::propagate(&a.any_seeds, &a.uf);
    if std::env::var_os("TORAJS_NUM_WIDTH_STATS").is_some() {
        let mut lines: Vec<String> = any_escaped.iter().map(|k| format!("{k:?}")).collect();
        lines.sort();
        for l in &lines {
            eprintln!("[num_width] any-escape class: {l}");
        }
    }
    // W4 shape-join (rotation 371; doc on the collector — sibling
    // `analyze_tables.rs`).
    let objlit_shape_f64 = analyze_tables::collect_objlit_shape_f64(ast, &a, &canon_out);

    WidthTable::new(
        canon_out,
        any_escaped,
        a.uf,
        a.container_poison,
        a.nominal_aliases,
        fallthrough_fns,
        undef_sentinel_params,
        objlit_shape_f64,
        a.proven_reads,
    )
}

/// Poison flows forward along assignment edges until stable.
/// Monotone single-direction lattice — O(edges).
fn fixpoint(seeds: Vec<SlotKey>, edges: &HashMap<SlotKey, Vec<Dep>>) -> HashSet<SlotKey> {
    let mut out: HashSet<SlotKey> = HashSet::new();
    let mut work: VecDeque<SlotKey> = seeds.into_iter().collect();
    while let Some(k) = work.pop_front() {
        if out.insert(k.clone())
            && let Some(dsts) = edges.get(&k)
        {
            for (d, _) in dsts {
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

    struct Slots(WidthTable);
    impl Slots {
        fn contains(&self, k: &SlotKey) -> bool {
            self.0.slot_is_f64(k)
        }
    }

    fn slots(src: &str) -> Slots {
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        Slots(analyze(
            &ast,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ))
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
    fn s1_toplevel_for_body_const_fract_poisons_param() {
        // mandelbrot repro — a for-body const is a main-fn binding
        // (Global key); its fract width must reach the callee param.
        let s = slots(
            "function g(x: number): number { return x; }\nfor (let i = 0; i < 3; i = i + 1) {\n  const cr = i / 100 - 1.5;\n  console.log(g(cr));\n}",
        );
        assert!(s.contains(&SlotKey::Global("cr".into())));
        assert!(s.contains(&param("g", "x")));
    }

    #[test]
    fn toplevel_block_let_fract_resolves_global() {
        let s = slots("{\n  let t = 0.25;\n  t = t + 0.5;\n  console.log(t);\n}");
        assert!(s.contains(&SlotKey::Global("t".into())));
    }

    #[test]
    fn toplevel_block_int_let_stays_narrow() {
        let s = slots(
            "function g(x: number): number { return x; }\nfor (let j = 0; j < 2; j = j + 1) {\n  const dbl = j * 2;\n  console.log(g(dbl));\n}",
        );
        assert!(!s.contains(&SlotKey::Global("dbl".into())));
        assert!(!s.contains(&param("g", "x")));
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
        assert!(!s.contains(&param("popcount", "x")));
        assert!(!s.contains(&local("popcount", "n")));
        assert!(!s.contains(&local("popcount", "count")));
        assert!(!s.contains(&ret("popcount")));
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
        // both faces provable: a non-negative integer-literal
        // dividend bounds the remainder in [+0, |b|) (no -0) and a
        // non-zero literal divisor rules out the runtime-0 NaN —
        // int path keeps.
        let s = slots(
            "function wrap(k: number): number { return k + 100 % 7; }\nconsole.log(wrap(7));",
        );
        assert!(!s.contains(&param("wrap", "k")));
        assert!(!s.contains(&ret("wrap")));
    }

    #[test]
    fn json_parse_seeds_number_faces_f64() {
        // ②.7 — JSON text is runtime data: every number-domain face
        // reachable from the parse target seeds F64 (the typed
        // cursor parser otherwise truncates AND deranges the cursor).
        let s = slots(
            "type P = { x: number, tags: string[], inner: Q };\ntype Q = { y: number };\nlet xs: number[] = JSON.parse(\"[1.5]\");\nlet p: P = JSON.parse(\"{}\");\nconsole.log(xs[0], p.x);",
        );
        let g = |n: &str| SlotKey::Global(n.into());
        assert!(s.0.elem_is_f64(&g("xs")));
        assert!(s.0.field_is_f64(&g("p"), "x"));
        // nested named-type field face seeds through recursion.
        let inner = SlotKey::Field(Box::new(g("p")), "inner".into());
        assert!(s.0.field_is_f64(&inner, "y"));
        // non-number faces stay out of the width domain.
        assert!(!s.0.field_is_f64(&g("p"), "tags"));
    }

    #[test]
    fn w3_modulo_variable_divisor_floats() {
        // srem runtime-0 (§5.3 follow-up close): `100 % k` with
        // k == 0 is NaN per spec, but the int path's sdiv-by-zero
        // hands the dividend back (silent 100). The result floats
        // and frem mints the NaN.
        let s =
            slots("function wrap(k: number): number { return 100 % k; }\nconsole.log(wrap(7));");
        assert!(s.contains(&ret("wrap")));
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
    fn f1_fn_type_ann_residents_join_ret() {
        // T1 shape — `half` (f64 ret) and `add` (int ret) both flow
        // through the same fn-type annotation; the signature's __ret
        // projection joins over all residents, floating add's Ret too
        // (one interned signature must agree with every resident).
        let s = slots(
            "function add(x: number, y: number): number { return x + y; }\nfunction half(x: number, y: number): number { return x / y; }\nfunction pickOp(op: boolean): (x: number, y: number) => number {\n  if (op) { return add; }\n  return half;\n}\nconsole.log(pickOp(true)(3, 4));\nconsole.log(pickOp(false)(3, 4));",
        );
        let ck = SlotKey::Class("__fn(number|number)->number".into());
        assert!(s.0.field_is_f64(&ck, "__ret"));
        assert!(s.contains(&ret("add")));
        assert!(s.contains(&ret("half")));
        assert!(!s.0.field_is_f64(&ck, "__p0"));
    }

    #[test]
    fn f1_all_int_residents_keep_narrow_sig() {
        let s = slots(
            "function add(x: number, y: number): number { return x + y; }\nfunction sub(x: number, y: number): number { return x - y; }\nfunction pick(op: boolean): (x: number, y: number) => number {\n  if (op) { return add; }\n  return sub;\n}\nconsole.log(pick(true)(3, 4));",
        );
        let ck = SlotKey::Class("__fn(number|number)->number".into());
        assert!(!s.0.field_is_f64(&ck, "__ret"));
        assert!(!s.contains(&ret("add")));
    }

    #[test]
    fn f1_unannotated_carrier_projects_ret() {
        // `let f = half` has no annotation — the flow union alone
        // must answer the indirect read through f.
        let s = slots(
            "function half(x: number, y: number): number { return x / y; }\nlet f = half;\nconsole.log(f(3, 4));",
        );
        let fk = SlotKey::Field(Box::new(SlotKey::Global("f".into())), "__ret".into());
        assert!(s.contains(&fk));
    }

    #[test]
    fn f1_indirect_arg_poisons_resident_param() {
        // A fract arg through an fn-value call must reach the
        // resident fn's param (the indirect mirror of S1).
        let s = slots(
            "function g(x: number): number { return x + 1; }\nlet f = g;\nconsole.log(f(0.5));",
        );
        assert!(s.contains(&param("g", "x")));
    }

    #[test]
    fn f1_indirect_call_ret_feeds_binding() {
        let s = slots(
            "function half(x: number, y: number): number { return x / y; }\nfunction pickOp(op: boolean): (x: number, y: number) => number {\n  return half;\n}\nlet r: number = pickOp(true)(8, 2);\nconsole.log(r);",
        );
        assert!(s.contains(&SlotKey::Global("r".into())));
    }

    #[test]
    fn w5_straightline_mul_no_cycle_stays_narrow() {
        // positive-literal cofactor: no -0 risk (S9), and a
        // straight-line mul outside any growth cycle stays narrow.
        let s = slots(
            "function area(w: number): number {\n  let a: number = w * 3;\n  return a;\n}\nconsole.log(area(4));",
        );
        assert!(!s.contains(&param("area", "w")));
        assert!(!s.contains(&local("area", "a")));
        assert!(!s.contains(&ret("area")));
    }

    #[test]
    fn s9_var_var_mul_widens_result_not_operands() {
        // S9 — runtime zero × runtime negative mints -0, so a
        // var×var product is f64; the factors themselves stay
        // narrow (width flows operand → result only).
        let s = slots(
            "function area(w: number, h: number): number {\n  let a: number = w * h;\n  return a;\n}\nconsole.log(area(3, 4));",
        );
        assert!(!s.contains(&param("area", "w")));
        assert!(!s.contains(&param("area", "h")));
        assert!(s.contains(&local("area", "a")));
        assert!(s.contains(&ret("area")));
    }

    #[test]
    fn s9_square_keeps_int_face() {
        // square carve — `x * x` can never be negative×zero, so -0
        // is unmintable and the product stays narrow.
        let s = slots(
            "function sq(x: number): number {\n  let a: number = x * x;\n  return a;\n}\nconsole.log(sq(7));",
        );
        assert!(!s.contains(&param("sq", "x")));
        assert!(!s.contains(&local("sq", "a")));
        assert!(!s.contains(&ret("sq")));
    }
}
