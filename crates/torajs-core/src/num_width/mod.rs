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
mod any_arr_writes;
mod bound_closures;
mod bounds_lower;
mod bounds_walk;
mod captures;
mod container;
mod container_assign;
mod container_callbacks;
mod container_lookup;
mod container_methods;
mod container_namespace;
mod container_promise;
mod container_result_key;
mod container_walk;
mod cycle;
mod escape;
mod escape_propagate;
mod fallthrough;
mod fnsig;
mod hidden_any;
mod hidden_any_calls;
mod hidden_any_containers;
mod hidden_any_scope;
mod hidden_any_seeds;
mod hidden_any_sources;
pub(crate) mod hof_receivers;
mod idle_bodies;
mod json_seed;
mod lattice;
mod let_names;
mod magnitude;
mod method_value;
mod mono;
mod nonzero_neg;
mod pipeline;
mod promise_entry;
mod promise_taint;
mod sentinel_hof;
mod sentinel_member_reads;
mod sentinel_tables;
mod slot_abi;
mod slot_key;
mod variadic_short;
mod walk;
mod walk_call;
mod width;
mod width_table;

pub(crate) use container_promise::awaited_promise_inner_of;
pub(crate) use fnsig::{fn_type_canon, split_fn_type};
pub(crate) use hidden_any::HiddenAny;
pub(crate) use magnitude::EXACT_INT_MAX;
pub(crate) use mono::{NumWidth, compute_typevar_widths};
pub(crate) use slot_key::SlotKey;
pub(crate) use width_table::WidthTable;

use crate::ast::{Ast, ExprId, GenericStructDecls, Stmt};
use fallthrough::{alias_fallthrough_closures, seed_and_walk_fn};
use lattice::{Dep, W, dump_f64_classes, fixpoint, join, literal_is_f64};
use sentinel_tables::close_sentinel_tables;
use std::collections::{HashMap, HashSet};

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
    /// The members of each expression the checker types a union, as
    /// narrowed where it is read (`check::union_narrow::UnionFacts`).
    pub(super) union_types: &'a HashMap<ExprId, crate::check::Type>,
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
    pub(super) bound_closures: bound_closures::BoundClosures,
    /// Reads of a function declaration's name a binding answers instead.
    pub(super) shadowing_reads: HashSet<ExprId>,
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
    /// Every slot key per name, for a synthetic body's unresolved names (`captures.rs`).
    pub(super) by_name: HashMap<String, Vec<SlotKey>>,
    pub(super) captures: captures::CaptureMap,
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
    /// W-ESC — escape sinks that are not number slots of the
    /// program's: the any faces found inside struct annotations (a
    /// field declared `any`) and the operands of casts to the any
    /// world. They demote the containers reaching them and never seed
    /// the F64 fixpoint (`escape.rs` `push_any_seed`, `walk_cast`).
    pub(super) escape_only_seeds: Vec<SlotKey>,
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
    pub(super) generic_decls: GenericStructDecls,
    /// The non-generic ones, for struct-shaped any-escape faces.
    pub(super) struct_decls: escape::StructDecls,
    /// W4 — an element write through a receiver the analysis cannot
    /// resolve to a container class. Never expected to fire (assign
    /// receivers are idents / members / indexes / calls); if it does,
    /// every elem/field query answers F64 — conservative, loud in
    /// STATS, never silent-wrong.
    pub(super) container_poison: bool,
    /// The keys of the values the walk keyed while their type held
    /// `any`: sources of promises it does not follow (`promise_entry`).
    pub(super) untracked_values: crate::ssa::id_hash::HashSet<SlotKey>,
    /// The number faces noted under a promise's value point (`promise_entry`).
    pub(super) number_faces: crate::ssa::id_hash::HashSet<SlotKey>,
    /// Does the program read a number out of a promise? Gates the two
    /// above (`promise_entry`).
    pub(super) promises: bool,
    /// The `(i, xs)` guard pairs standing at the walk's current point
    /// — pushed by a loop whose condition is `i < xs.length`, evicted
    /// by a statement that taints them, popped at the loop's end. See
    /// [`crate::ssa_lower_bounds_proven`] for what taints one.
    pub(super) bounds_stack: Vec<(String, String, bool)>,
    /// Guard conditions whose counter is also provably non-negative
    /// ([`bounds_lower`]) — the half the element seed needs and the
    /// bounds-check elision does not.
    pub(super) lower_settled: HashSet<ExprId>,
    /// Where that stack held while walking an index read: the reads
    /// proven in-bounds. Handed to the frozen table, because the
    /// element widths below are only sound under this proof (the
    /// field's doc on `WidthTable` says why).
    pub(super) proven_reads: HashSet<ExprId>,
    /// The subset of those whose pair settles both ends: the read is
    /// inside `[0, length)` and never answers `undefined`.
    pub(super) settled_reads: HashSet<ExprId>,
    /// The `s.charCodeAt(i)` calls walked under a pair that settles
    /// both ends (`bounds_walk::char_code_in_bounds`): their width
    /// is an integer, and lowering drops their NaN arm.
    pub(super) char_code_proven: HashSet<ExprId>,
    /// Member reads that carry the sentinel with no container key to
    /// carry it for them (`sentinel_member_reads`). Filled after the
    /// sentinel tables close and before the walk, because `width.rs`
    /// reads it per expression and the getter half costs an arena
    /// scan to answer.
    pub(super) sentinel_member_reads: HashSet<ExprId>,
    /// Magnitude bounds of the writes and `+ - *` the walk saw.
    mag: magnitude::MagRecord,
    /// Operands of the `-x` that cannot answer -0 (`nonzero_neg`).
    pub(super) nonzero_negs: HashSet<ExprId>,
    /// Each callback-taking iteration with the array it walks and that
    /// array's key (`hof_receivers`).
    pub(super) hof_receivers: Vec<(ExprId, ExprId, SlotKey)>,
    /// Each call that may leave a parameter unfilled: the callee's key,
    /// that position, whether later arguments are unknown (`variadic_short`).
    pub(super) variadic_calls: Vec<(SlotKey, usize, bool)>,
    /// Each function value stored into a slot, with the parameters
    /// that slot's argument positions reach (`fn_value_flow`).
    pub(super) fn_flows: Vec<(SlotKey, String, Vec<String>)>,
    /// The expressions that never run (`idle_bodies`), settled at the
    /// first `any` cast that asks.
    idle: std::cell::OnceCell<idle_bodies::IdleBodies>,
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
    union_types: &HashMap<ExprId, crate::check::Type>,
) -> WidthTable {
    let (fn_params, toplevel_lets, by_name, const_ints) =
        analyze_tables::collect_slot_registry(ast);
    let bodies = captures::bodies(ast);
    let captures = captures::collect_captures(ast, &bodies, &toplevel_lets);
    let (bound_closures, shadowing_reads) =
        bound_closures::collect(ast, &bodies, &toplevel_lets, &captures);
    let nonzero_negs = nonzero_neg::collect(ast, &bodies, &toplevel_lets, expr_types);

    let mut classes: Vec<String> = ast.class_parents.keys().cloned().collect();
    classes.sort();
    let nominal_aliases = alias::nominal_alias_names(ast);
    let generic_decls = analyze_tables::collect_generic_decls(ast);

    let mut a = Analysis {
        ast,
        expr_types,
        union_types,
        retargets,
        demoted,
        fn_params,
        bound_closures,
        shadowing_reads,
        toplevel_lets,
        const_ints,
        by_name,
        captures,
        seeds: Vec::new(),
        edges: HashMap::new(),
        c_seeds: Vec::new(),
        c_edges: HashMap::new(),
        any_seeds: Vec::new(),
        escape_only_seeds: Vec::new(),
        uf: container::UnionFind::default(),
        guarded_unions: Vec::new(),
        nested_unions: Vec::new(),
        containerish: HashSet::new(),
        classes,
        nominal_aliases,
        generic_decls,
        struct_decls: escape::collect_struct_decls(ast),
        container_poison: false,
        untracked_values: Default::default(),
        number_faces: Default::default(),
        promises: promise_entry::reads_number_promises(ast, expr_types),
        bounds_stack: Vec::new(),
        lower_settled: bounds_lower::settled_guards(ast),
        proven_reads: HashSet::new(),
        settled_reads: HashSet::new(),
        char_code_proven: HashSet::new(),
        sentinel_member_reads: HashSet::new(),
        mag: Default::default(),
        nonzero_negs,
        hof_receivers: Vec::new(),
        variadic_calls: Vec::new(),
        fn_flows: Vec::new(),
        idle: std::cell::OnceCell::new(),
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
    // call site tainted is one of the ways it can. The two answers
    // feed each other, so they are closed together first.
    let mut tables = close_sentinel_tables(&a, &ast.stmts);
    // Those tables answer by name; the widths the walk is about to
    // assign also need the two shapes that have no name (see
    // `sentinel_member_reads`), and they need them before the walk
    // starts, not after.
    a.sentinel_member_reads = sentinel_member_reads::freeze(&tables);
    let mut fallthrough_fns: HashSet<String> = tables.fns.clone();
    a.seed_nominal_field_faces();
    for stmt in &ast.stmts {
        if let Stmt::FnDecl { .. } = stmt {
            seed_and_walk_fn(&mut a, stmt, &tables, &mut fallthrough_fns);
        } else {
            a.walk_stmt(stmt, &top_scope);
        }
    }

    alias_fallthrough_closures(ast, &mut fallthrough_fns);

    let promise_taint = pipeline::join_classes(&mut a);
    cycle::seed_growth_cycles(&a.edges, &mut a.seeds);
    let arith_in_window = magnitude::settle(&mut a);
    analyze_tables::seed_any_and_poison(&mut a);
    variadic_short::seed(&mut a, &mut tables.params);
    let canon_out = fixpoint(std::mem::take(&mut a.seeds), &a.edges);
    dump_f64_classes(&canon_out);

    // W-ESC — escape faces on frozen class reps, Elem chains included
    let escapes = hof_receivers::settle(&a, &mut tables.params);

    let mut table = WidthTable::new(
        canon_out,
        escapes,
        a.uf,
        a.container_poison,
        a.nominal_aliases,
        fallthrough_fns,
        tables.params,
        tables.elems,
        tables.fields,
        a.proven_reads,
        a.settled_reads,
        a.char_code_proven,
        arith_in_window,
        a.nonzero_negs,
        a.by_name,
        a.captures,
        a.bound_closures,
    );
    table.promise_taint = promise_taint;
    table
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
        let ck = SlotKey::Class("__fn(number|number)->(number)".into());
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
        let ck = SlotKey::Class("__fn(number|number)->(number)".into());
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
    fn s9_square_in_a_slot_is_f64() {
        // `x * x` mints no -0, but an integer `x` past 2^26.5 squares
        // past 2^53, where Number multiplication rounds: a slot taking
        // the product holds the f64 one; the factor stays narrow.
        let s = slots(
            "function sq(x: number): number {\n  let a: number = x * x;\n  return a;\n}\nconsole.log(sq(7));",
        );
        assert!(!s.contains(&param("sq", "x")));
        assert!(s.contains(&local("sq", "a")));
        assert!(s.contains(&ret("sq")));
    }

    #[test]
    fn window_sum_of_large_param_is_f64() {
        // `z + z + z` over a value near 2^53 leaves the exact window
        let big = slots(
            "function g(z: number): number {\n  let t: number = z + z + z;\n  return t;\n}\nconsole.log(g(9007199254740991));",
        );
        assert!(big.contains(&local("g", "t")));
        assert!(!big.contains(&param("g", "z")));
        let small =
            slots("function g(z: number): number {\n  return z + z + z;\n}\nconsole.log(g(7));");
        assert!(!small.contains(&ret("g")));
    }

    #[test]
    fn window_counter_bound_counts_its_start() {
        let src = |start: &str| {
            format!(
                "function f(n: number): number {{\n  let s: number = {start};\n  let i: number = 0;\n  while (i < n) {{ s += 2; i++; }}\n  return s;\n}}\nconsole.log(f(5));"
            )
        };
        let big = slots(&src("9007199254740000"));
        assert!(big.contains(&local("f", "s")));
        assert!(!big.contains(&local("f", "i")));
        let small = slots(&src("0"));
        assert!(!small.contains(&local("f", "s")));
    }

    #[test]
    fn window_const_product_of_large_literal_is_f64() {
        let s = slots(
            "function m(x: number): number {\n  let p: number = x * 3;\n  let q: number = x;\n  return p + q;\n}\nconsole.log(m(9007199254740991));",
        );
        assert!(s.contains(&local("m", "p")));
        assert!(!s.contains(&local("m", "q")));
    }

    #[test]
    fn window_loop_test_bounds_its_counter() {
        let src = |bound: &str| {
            format!(
                "function f(n: number): number {{\n  let p: number = 0;\n  for (let i = 0; i < {bound}; i++) {{ p = i * 10000000; }}\n  return p;\n}}\nconsole.log(f(5), [9].map(f));"
            )
        };
        assert!(!slots(&src("1000")).contains(&local("f", "p")));
        // `f` is also a callback, so `n` may be past 2^32 and `i * 10^7`
        // past 2^53
        assert!(slots(&src("n")).contains(&local("f", "p")));
        // a negative step is not held by an upper bound
        let down = slots(
            "function g(): number {\n  let p: number = 0;\n  let i: number = 0;\n  while (i < 1000) { i = i + -1; p = i * 10000000; }\n  return p;\n}\nconsole.log(g());",
        );
        assert!(down.contains(&local("g", "p")));
    }

    #[test]
    fn window_direct_callee_param_takes_its_arguments_bound() {
        let src = |extra: &str| {
            format!(
                "function h(k: number): number {{\n  let p: number = k * 2654435761;\n  return p;\n}}\nconsole.log(h(7), h(11){extra});"
            )
        };
        assert!(!slots(&src("")).contains(&local("h", "p")));
        assert!(slots(&src(", [3].map(h)")).contains(&local("h", "p")));
    }
}
