//! W4 (ann-width RFC §5.4) — container element / field width machinery.
//!
//! Containers are reference values: a callee writing a fractional
//! number through `xs: number[]` mutates the caller's array, so the
//! element-width decision is made per *alias equivalence class*, not
//! per slot. This module carries the union-find the constraint walk
//! feeds, the congruence closure that keeps nested container points
//! consistent (find(a)==find(b) ⟹ Elem(a)≡Elem(b), the Nelson-Oppen
//! shape), and the canonicalization that rewrites the walk's symbolic
//! `Elem(slot)` / `Field(slot, name)` keys onto class representatives
//! before the poison fixpoint runs.
//!
//! Alias edges come in two strengths:
//! - unconditional — sites that are themselves container evidence:
//!   element writes, literal / transform-method plumbing, nominal
//!   class hookups. Applied straight into the union-find.
//! - guarded — plain value copies (`let ys = xs`, call args, returns,
//!   for-of bindings, ternary arms). Activated post-walk only when
//!   either endpoint shows container evidence somewhere in the
//!   module, so scalar copy chains (`let v = xs[0]; v = v / 2`) don't
//!   glue the copy's width back onto the source class.

use super::{Analysis, SlotKey};
use std::collections::{HashMap, HashSet};

/// Union-find over slot keys. Walk-time access goes through the
/// mutating path-compressing `find`; frozen post-analysis queries
/// walk the parent chain without mutation.
#[derive(Default)]
pub(crate) struct UnionFind {
    parent: HashMap<SlotKey, SlotKey>,
}

impl UnionFind {
    pub(super) fn find(&mut self, k: &SlotKey) -> SlotKey {
        let p = match self.parent.get(k) {
            None => return k.clone(),
            Some(p) => p.clone(),
        };
        let root = self.find(&p);
        if root != p {
            self.parent.insert(k.clone(), root.clone());
        }
        root
    }

    pub(super) fn union(&mut self, a: &SlotKey, b: &SlotKey) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        // Minimal-nesting representative: keeps canonical spellings
        // shallow and makes self-referential containers (`xs.push(xs)`
        // unions Elem(xs) with xs) terminate — the congruence rewrite
        // Elem(find(x)) can then never deepen a key.
        if depth(&rb) < depth(&ra) {
            self.parent.insert(ra, rb);
        } else {
            self.parent.insert(rb, ra);
        }
    }

    fn find_frozen(&self, k: &SlotKey) -> SlotKey {
        let mut cur = k;
        loop {
            match self.parent.get(cur) {
                Some(p) => cur = p,
                None => return cur.clone(),
            }
        }
    }

    fn keys(&self) -> impl Iterator<Item = &SlotKey> {
        self.parent.keys().chain(self.parent.values())
    }
}

fn depth(k: &SlotKey) -> usize {
    match k {
        SlotKey::Elem(x) | SlotKey::Field(x, _) => depth(x) + 1,
        _ => 0,
    }
}

/// Deep-canonical form of a key: inner container references rewritten
/// to their class representatives, then the whole key resolved.
/// Congruence closure registers the rewritten spellings, so the outer
/// find lands on the class the walk's symbolic spellings merged into.
pub(super) fn canon_key(uf: &mut UnionFind, k: &SlotKey) -> SlotKey {
    let rewritten = match k {
        SlotKey::Elem(x) => SlotKey::Elem(Box::new(canon_key(uf, x))),
        SlotKey::Field(x, n) => SlotKey::Field(Box::new(canon_key(uf, x)), n.clone()),
        _ => k.clone(),
    };
    uf.find(&rewritten)
}

fn canon_key_frozen(uf: &UnionFind, k: &SlotKey) -> SlotKey {
    let rewritten = match k {
        SlotKey::Elem(x) => SlotKey::Elem(Box::new(canon_key_frozen(uf, x))),
        SlotKey::Field(x, n) => SlotKey::Field(Box::new(canon_key_frozen(uf, x)), n.clone()),
        _ => k.clone(),
    };
    uf.find_frozen(&rewritten)
}

/// Frozen analysis result: the poison fixpoint over canonical alias
/// classes (scalar slots and container elem/field points in one
/// lattice), queried through the frozen union-find so any spelling of
/// a key resolves to its class representative.
pub(crate) struct WidthTable {
    canon: HashSet<SlotKey>,
    uf: UnionFind,
    container_poison: bool,
}

impl WidthTable {
    pub(super) fn new(canon: HashSet<SlotKey>, uf: UnionFind, container_poison: bool) -> Self {
        WidthTable {
            canon,
            uf,
            container_poison,
        }
    }

    pub(crate) fn slot_is_f64(&self, k: &SlotKey) -> bool {
        self.canon.contains(&canon_key_frozen(&self.uf, k))
    }

    pub(crate) fn elem_is_f64(&self, holder: &SlotKey) -> bool {
        self.container_poison || self.slot_is_f64(&SlotKey::Elem(Box::new(holder.clone())))
    }

    #[allow(dead_code)] // D3 wires the struct/class field face
    pub(crate) fn field_is_f64(&self, holder: &SlotKey, name: &str) -> bool {
        self.container_poison
            || self.slot_is_f64(&SlotKey::Field(Box::new(holder.clone()), name.to_string()))
    }
}

/// Nominal class hookups: every instance of `class C` shares one
/// struct layout at lowering, so all of them must answer field-width
/// queries from one class. `__new_C`'s ret carries every constructed
/// instance; each method's `__this` param carries every receiver.
pub(super) fn nominal_unions(a: &mut Analysis) {
    let mut fn_names: Vec<&String> = a.fn_params.keys().collect();
    fn_names.sort();
    let mut unions: Vec<(SlotKey, SlotKey)> = Vec::new();
    for f in fn_names {
        for c in &a.classes {
            if *f == format!("__new_{c}") {
                unions.push((SlotKey::Class(c.clone()), SlotKey::Ret(f.clone())));
            } else if f.starts_with(&format!("__cm_{c}__"))
                && a.fn_params[f].first().is_some_and(|p| p == "__this")
            {
                unions.push((
                    SlotKey::Class(c.clone()),
                    SlotKey::Param(f.clone(), "__this".to_string()),
                ));
            }
        }
    }
    for (x, y) in unions {
        a.mark_containerish(&x);
        a.mark_containerish(&y);
        a.uf.union(&x, &y);
    }
}

/// Guarded-union activation: an edge applies iff either endpoint has
/// container evidence; applying it spreads the evidence to the other
/// endpoint, so chains of copies activate transitively.
pub(super) fn activate_guarded(a: &mut Analysis) {
    let edges = std::mem::take(&mut a.guarded_unions);
    let mut pending: Vec<(SlotKey, SlotKey)> = edges;
    loop {
        let mut next: Vec<(SlotKey, SlotKey)> = Vec::new();
        let mut changed = false;
        for (x, y) in pending {
            if a.containerish.contains(&x) || a.containerish.contains(&y) {
                a.containerish.insert(x.clone());
                a.containerish.insert(y.clone());
                a.uf.union(&x, &y);
                changed = true;
            } else {
                next.push((x, y));
            }
        }
        if !changed {
            break;
        }
        pending = next;
    }
}

fn collect_subkeys(k: &SlotKey, out: &mut HashSet<SlotKey>) {
    if out.insert(k.clone()) {
        match k {
            SlotKey::Elem(x) | SlotKey::Field(x, _) => collect_subkeys(x, out),
            _ => {}
        }
    }
}

/// Congruence closure + key rewriting. After the walk, two symbolic
/// spellings `Elem(xs)` / `Elem(ys)` with find(xs)==find(ys) denote
/// the same element class; close over that congruence, then rewrite
/// every seed / edge key onto its representative so the fixpoint
/// propagates per alias class.
pub(super) fn canonicalize(a: &mut Analysis) {
    let mut keys: HashSet<SlotKey> = HashSet::new();
    for s in &a.seeds {
        collect_subkeys(s, &mut keys);
    }
    for (d, ts) in &a.edges {
        collect_subkeys(d, &mut keys);
        for (t, _) in ts {
            collect_subkeys(t, &mut keys);
        }
    }
    let uf_keys: Vec<SlotKey> = a.uf.keys().cloned().collect();
    for k in uf_keys {
        collect_subkeys(&k, &mut keys);
    }

    loop {
        let mut changed = false;
        for k in keys.clone() {
            let canon_inner = match &k {
                SlotKey::Elem(x) => SlotKey::Elem(Box::new(a.uf.find(x))),
                SlotKey::Field(x, n) => SlotKey::Field(Box::new(a.uf.find(x)), n.clone()),
                _ => continue,
            };
            keys.insert(canon_inner.clone());
            if a.uf.find(&k) != a.uf.find(&canon_inner) {
                a.uf.union(&k, &canon_inner);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let seeds = std::mem::take(&mut a.seeds);
    a.seeds = seeds.iter().map(|k| canon_key(&mut a.uf, k)).collect();
    let edges = std::mem::take(&mut a.edges);
    for (d, ts) in edges {
        let cd = canon_key(&mut a.uf, &d);
        let entry = a.edges.entry(cd).or_default();
        for (t, g) in ts {
            entry.push((canon_key(&mut a.uf, &t), g));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num_width::analyze;
    use crate::{lexer, parser};
    use std::collections::HashMap;

    fn table(src: &str) -> WidthTable {
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(&tokens).expect("parse");
        analyze(&ast, &HashMap::new())
    }

    fn table_classes(src: &str) -> WidthTable {
        let tokens = lexer::tokenize(src).expect("lex");
        let mut ast = parser::parse(&tokens).expect("parse");
        crate::ast::desugar_classes(&mut ast);
        analyze(&ast, &HashMap::new())
    }

    fn g(n: &str) -> SlotKey {
        SlotKey::Global(n.into())
    }

    #[test]
    fn s3a_index_assign_fract_poisons_elem() {
        let t = table("let a: number[] = [1, 2];\na[0] = 0.5;\nconsole.log(a[0]);");
        assert!(t.elem_is_f64(&g("a")));
    }

    #[test]
    fn int_writes_keep_elem_narrow() {
        let t = table(
            "let xs: number[] = [];\nlet i: number = 0;\nwhile (i < 10) { xs.push(i); i = i + 1; }\nxs[0] = 7;\nconsole.log(xs[0]);",
        );
        assert!(!t.elem_is_f64(&g("xs")));
    }

    #[test]
    fn push_fract_poisons_elem() {
        let t = table("let xs: number[] = [];\nxs.push(0.5);");
        assert!(t.elem_is_f64(&g("xs")));
    }

    #[test]
    fn s3b_literal_fract_init_poisons_elem() {
        let t = table("let b: number[] = [1.5, 2];\nconsole.log(b[0]);");
        assert!(t.elem_is_f64(&g("b")));
    }

    #[test]
    fn s3e_write_through_param_aliases_caller() {
        let t = table(
            "function set(q: number[]) { q[1] = 2.5; }\nlet e: number[] = [1, 2];\nset(e);\nconsole.log(e[1]);",
        );
        assert!(t.elem_is_f64(&g("e")));
        assert!(t.elem_is_f64(&SlotKey::Param("set".into(), "q".into())));
    }

    #[test]
    fn s3f_member_assign_fract_poisons_field() {
        let t = table("let o: { x: number } = { x: 1 };\no.x = 0.5;\nconsole.log(o.x);");
        assert!(t.field_is_f64(&g("o"), "x"));
    }

    #[test]
    fn int_field_stays_narrow() {
        let t = table("let o: { x: number } = { x: 1 };\no.x = 2;\nconsole.log(o.x);");
        assert!(!t.field_is_f64(&g("o"), "x"));
    }

    #[test]
    fn s3h_class_field_assign_poisons_nominal_class() {
        let t = table_classes(
            "class P { x: number; constructor() { this.x = 1; } }\nlet q = new P();\nq.x = 0.5;\nconsole.log(q.x);",
        );
        assert!(t.field_is_f64(&SlotKey::Class("P".into()), "x"));
        assert!(t.field_is_f64(&g("q"), "x"));
    }

    #[test]
    fn class_int_fields_stay_narrow() {
        let t = table_classes(
            "class P { x: number; constructor() { this.x = 1; } }\nlet q = new P();\nq.x = 3;\nconsole.log(q.x);",
        );
        assert!(!t.field_is_f64(&SlotKey::Class("P".into()), "x"));
    }

    #[test]
    fn class_method_this_write_poisons_field() {
        let t = table_classes(
            "class P { x: number; constructor() { this.x = 1; }\n  half() { this.x = 0.5; } }\nlet q = new P();\nq.half();\nconsole.log(q.x);",
        );
        assert!(t.field_is_f64(&SlotKey::Class("P".into()), "x"));
    }

    #[test]
    fn nested_array_congruence() {
        let t = table(
            "let grid: number[][] = [[1, 2]];\nlet row = grid[0];\nrow[0] = 0.5;\nconsole.log(grid[0][0]);",
        );
        assert!(t.elem_is_f64(&SlotKey::Elem(Box::new(g("grid")))));
        assert!(t.elem_is_f64(&g("row")));
    }

    #[test]
    fn scalar_elem_copy_does_not_poison_source() {
        let t = table("let xs: number[] = [4, 2];\nlet v = xs[0];\nv = v / 2;\nconsole.log(v);");
        assert!(!t.elem_is_f64(&g("xs")));
    }

    #[test]
    fn map_callback_ret_feeds_result_elems() {
        let t = table(
            "function addHalf(x: number): number { return x + 0.5; }\nfunction same(x: number): number { return x; }\nlet xs: number[] = [1, 2];\nlet ys = xs.map(addHalf);\nlet zs = xs.map(same);\nconsole.log(ys[0]);",
        );
        assert!(t.elem_is_f64(&g("ys")));
        assert!(!t.elem_is_f64(&g("zs")));
        assert!(!t.elem_is_f64(&g("xs")));
    }

    #[test]
    fn spread_from_fract_source_poisons_target() {
        let t =
            table("let xs: number[] = [0.5];\nlet ys: number[] = [...xs, 2];\nconsole.log(ys[0]);");
        assert!(t.elem_is_f64(&g("ys")));
    }

    #[test]
    fn self_referential_push_terminates() {
        let t = table("let xs = [1];\nxs.push(xs);\nconsole.log(xs[0]);");
        assert!(!t.elem_is_f64(&g("zzz_unrelated")));
    }

    #[test]
    fn ternary_arm_flows_merge() {
        let t = table(
            "let a: number[] = [1];\nlet b: number[] = [2];\nlet c = true ? a : b;\nc[0] = 0.5;\nconsole.log(a[0]);",
        );
        assert!(t.elem_is_f64(&g("a")));
        assert!(t.elem_is_f64(&g("b")));
    }

    #[test]
    fn ret_flow_aliases_binding() {
        let t = table(
            "function make(): number[] { let q: number[] = [1, 2]; return q; }\nlet m = make();\nm[0] = 0.5;\nconsole.log(m[0]);",
        );
        assert!(t.elem_is_f64(&g("m")));
        assert!(t.elem_is_f64(&SlotKey::Ret("make".into())));
    }

    #[test]
    fn scalar_w1_shapes_unchanged_by_container_pipeline() {
        // r3 shape — the guarded unions must stay inert for pure
        // scalar copy chains (no container evidence anywhere).
        let t = table(
            "function f(x: number): number {\n  let n: number = x;\n  while (n % 2 === 0) { n = n / 2; }\n  return n;\n}\nconsole.log(f(12));",
        );
        assert!(t.slot_is_f64(&SlotKey::Local("f".into(), "n".into())));
        assert!(!t.slot_is_f64(&SlotKey::Param("f".into(), "x".into())));
    }
}
