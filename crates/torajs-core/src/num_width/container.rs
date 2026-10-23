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

use super::slot_keys::{SlotKeyData, SlotKeyId, SlotKeys};
use super::{Analysis, SlotKey};
use crate::ssa::id_hash::{HashMap, HashSet};

/// Union-find over slot key ids, a parent array indexed by id (an id
/// past its end is its own root). Walk-time access goes through the
/// mutating path-compressing `find`; frozen post-analysis queries
/// walk the parent chain without mutation.
#[derive(Default)]
pub(crate) struct UnionFind {
    parent: Vec<u32>,
}

impl UnionFind {
    #[inline]
    fn parent_of(&self, k: SlotKeyId) -> SlotKeyId {
        match self.parent.get(k.index()) {
            Some(p) => SlotKeyId::from_index(*p as usize),
            None => k,
        }
    }

    pub(super) fn find(&mut self, k: SlotKeyId) -> SlotKeyId {
        let root = self.find_frozen(k);
        let mut cur = k;
        while cur != root {
            let next = self.parent_of(cur);
            self.parent[cur.index()] = root.index() as u32;
            cur = next;
        }
        root
    }

    pub(super) fn union(&mut self, keys: &SlotKeys, a: SlotKeyId, b: SlotKeyId) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        let need = ra.index().max(rb.index()) + 1;
        if self.parent.len() < need {
            let from = self.parent.len() as u32;
            self.parent.extend(from..need as u32);
        }
        // Minimal-nesting representative: keeps canonical spellings
        // shallow and makes self-referential containers (`xs.push(xs)`
        // unions Elem(xs) with xs) terminate — the congruence rewrite
        // Elem(find(x)) can then never deepen a key.
        if keys.depth(rb) < keys.depth(ra) {
            self.parent[ra.index()] = rb.index() as u32;
        } else {
            self.parent[rb.index()] = ra.index() as u32;
        }
    }

    pub(super) fn find_frozen(&self, k: SlotKeyId) -> SlotKeyId {
        let mut cur = k;
        loop {
            let p = self.parent_of(cur);
            if p == cur {
                return cur;
            }
            cur = p;
        }
    }

    /// Every key of a class with more than one member.
    pub(super) fn keys(&self) -> impl Iterator<Item = SlotKeyId> + '_ {
        (self.parent.iter().enumerate())
            .filter(|(i, p)| **p as usize != *i)
            .flat_map(|(i, p)| [SlotKeyId::from_index(i), SlotKeyId::from_index(*p as usize)])
    }
}

/// Deep-canonical form of a key: inner container references rewritten
/// to their class representatives, then the whole key resolved.
/// Congruence closure registers the rewritten spellings, so the outer
/// find lands on the class the walk's symbolic spellings merged into.
pub(super) fn canon_key(uf: &mut UnionFind, keys: &SlotKeys, k: SlotKeyId) -> SlotKeyId {
    let rewritten = match keys.data(k) {
        SlotKeyData::Elem(x) => {
            let cx = canon_key(uf, keys, x);
            keys.mk(SlotKeyData::Elem(cx))
        }
        SlotKeyData::Field(x, n) => {
            let cx = canon_key(uf, keys, x);
            keys.mk(SlotKeyData::Field(cx, n))
        }
        _ => k,
    };
    uf.find(rewritten)
}

/// [`canon_key`] without path compression. The rewritten spelling is
/// interned, so the answer can be filed in a set and met again.
pub(super) fn canon_key_frozen(uf: &UnionFind, keys: &SlotKeys, k: SlotKeyId) -> SlotKeyId {
    let rewritten = match keys.data(k) {
        SlotKeyData::Elem(x) => {
            let cx = canon_key_frozen(uf, keys, x);
            keys.mk(SlotKeyData::Elem(cx))
        }
        SlotKeyData::Field(x, n) => {
            let cx = canon_key_frozen(uf, keys, x);
            keys.mk(SlotKeyData::Field(cx, n))
        }
        _ => k,
    };
    uf.find_frozen(rewritten)
}

/// [`canon_key_frozen`] of a spelling lowering asks with, adding
/// nothing to the table: `None` when some rewritten part was never
/// interned. Interning a part interns every key under it, so such a
/// spelling is its own class, and no class the analysis filed.
pub(super) fn canon_spelling(uf: &UnionFind, keys: &SlotKeys, k: &SlotKey) -> Option<SlotKeyId> {
    let rewritten = match k {
        SlotKey::Elem(x) => keys.get(&SlotKeyData::Elem(canon_spelling(uf, keys, x)?))?,
        SlotKey::Field(x, n) => {
            let cx = canon_spelling(uf, keys, x)?;
            keys.get(&SlotKeyData::Field(cx, keys.find_atom(n)?))?
        }
        _ => keys.lookup(k)?,
    };
    Some(uf.find_frozen(rewritten))
}

/// Nominal class hookups: every instance of `class C` shares one
/// struct layout at lowering, so all of them must answer field-width
/// queries from one class. `__new_C`'s ret carries every constructed
/// instance; each method's `__this` param carries every receiver.
pub(super) fn nominal_unions(a: &mut Analysis) {
    let mut fn_names: Vec<&String> = a.fn_params.keys().collect();
    fn_names.sort();
    let mut unions: Vec<(SlotKeyId, SlotKeyId)> = Vec::new();
    for f in fn_names {
        for c in &a.classes {
            if *f == format!("__new_{c}") {
                unions.push((a.keys.class(c), a.keys.ret(f)));
            } else if f.starts_with(&format!("__cm_{c}__"))
                && a.fn_params[f].first().is_some_and(|p| p == "__this")
            {
                unions.push((a.keys.class(c), a.keys.param(f, "__this")));
            }
        }
    }
    for (x, y) in unions {
        a.mark_containerish(x);
        a.mark_containerish(y);
        a.uf.union(&a.keys, x, y);
    }
}

/// Generator step hookup: `for (const v of h(3))` binds `v` to the
/// ELEMENT of the generator object, and the parser's pre-built
/// `src[i]` element read spells that as `Elem(Ret(h))`. The value
/// actually delivered is the `value` field of what `h`'s desugared
/// class answers from `next()`, and nothing joined the two — so a
/// widened step value met a narrow `nums` at `nums.push(v)` and the
/// write refused loudly ("container width analysis missed this
/// write", 553-04).
///
/// `generator_factory_classes` is the factory-fn → `__Gen_<name>` map
/// `desugar_generators` leaves behind; the step methods are found the
/// same way `nominal_unions` finds a class's methods, by prefix over
/// the analyzed fn names, so both the direct and the any-lane copy
/// join.
pub(super) fn generator_step_unions(a: &mut Analysis) {
    if a.ast.generator_factory_classes.is_empty() {
        return;
    }
    let mut fn_names: Vec<String> = a.fn_params.keys().cloned().collect();
    fn_names.sort();
    let mut unions: Vec<(SlotKeyId, SlotKeyId)> = Vec::new();
    let factories: Vec<(String, String)> = a
        .ast
        .generator_factory_classes
        .iter()
        .map(|(f, c)| (f.clone(), c.clone()))
        .collect();
    for (factory, class) in factories {
        let fr = a.keys.ret(&factory);
        let elem = a.keys.elem(fr);
        for m in &fn_names {
            if *m == format!("__cm_{class}__next") || *m == format!("__cmany_{class}__next") {
                let mr = a.keys.ret(m);
                unions.push((elem, a.keys.field(mr, "value")));
            }
        }
    }
    for (x, y) in unions {
        a.mark_containerish(x);
        a.mark_containerish(y);
        a.uf.union(&a.keys, x, y);
    }
}

/// Iterator step hookup, the class-protocol counterpart of
/// [`generator_step_unions`]: `for (const v of it)` over an instance of
/// a class `C` whose `[Symbol.iterator]()` declares iterator class `I`
/// binds `v` through the parser's `src[i]` element read, which spells
/// `Elem(Class(C))`, while the loop loads `value` out of what `I`'s
/// `next()` answers (§7.4.8 IteratorValue). With nothing joining the
/// two, a fractional `value` met an integer-wide `out` at
/// `out.push(v)`. Both copies of `next` join, as above.
pub(super) fn iterator_step_unions(a: &mut Analysis) {
    use crate::ssa_lower_for_of_iter_protocol_plan::{iterator_method_classes, iterator_next_fn};
    let mut unions: Vec<(SlotKeyId, SlotKeyId)> = Vec::new();
    for st in &a.ast.stmts {
        let Some((src, iter)) = iterator_method_classes(a.ast, st) else {
            continue;
        };
        let elem = a.keys.elem(a.keys.class(src));
        let next = iterator_next_fn(iter);
        for m in [next.clone(), next.replacen("__cm_", "__cmany_", 1)] {
            if a.fn_params.contains_key(&m) {
                let mr = a.keys.ret(&m);
                unions.push((elem, a.keys.field(mr, "value")));
            }
        }
    }
    for (x, y) in unions {
        a.mark_containerish(x);
        a.mark_containerish(y);
        a.uf.union(&a.keys, x, y);
    }
}

/// Objlit-nominal constructor hookup: a method-bearing object literal
/// IS the single constructor of its synthetic `__ObjLit_<n>` type
/// (ast/objlit_nominal.rs), so its literal-origin Anon key joins the
/// Class key exactly the way `__new_C`'s ret joins `Class(C)` above.
/// The methods' `__this: __ObjLit_<n>` params join through the
/// annotation hookups (`alias_ann_union`) once the name is in the
/// nominal set; this glue is the missing constructor edge — without
/// it the F5 field→fn sig projections live on the Anon key only,
/// while the TypeDecl fill site (pass 0.5) queries the Class key.
pub(super) fn objlit_ctor_unions(a: &mut Analysis) {
    if a.ast.objlit_method_fields.is_empty() {
        return;
    }
    // fn name → objlit type, off the `__this` ann objlit_nominal pinned
    // — LOAD-BEARING: re-annotating a face drops this edge (r461).
    let mut fn_owner: HashMap<String, String> = HashMap::default();
    for stmt in &a.ast.stmts {
        if let crate::ast::Stmt::FnDecl { name, params, .. } = stmt
            && let Some(p) = params.iter().find(|p| p.name == "__this")
            && let Some(ann) = &p.type_ann
            && a.ast.objlit_method_fields.contains_key(ann)
        {
            fn_owner.insert(name.clone(), ann.clone());
        }
    }
    if fn_owner.is_empty() {
        return;
    }
    let mut unions: Vec<(SlotKeyId, SlotKeyId)> = Vec::new();
    for (i, e) in a.ast.exprs.iter().enumerate() {
        let crate::ast::Expr::ObjectLit { fields } = e else {
            continue;
        };
        // A stale arena copy (rewrite passes re-add composites) shares
        // the live literal's method ExprIds, so it resolves to the same
        // class — joining it is harmless.
        let owner = fields.iter().find_map(|(_, fe)| match a.ast.get_expr(*fe) {
            crate::ast::Expr::Closure { fn_name, .. } => fn_owner.get(fn_name),
            _ => None,
        });
        if let Some(ty) = owner {
            unions.push((a.keys.class(ty), a.keys.anon(i as u32)));
        }
    }
    for (x, y) in unions {
        a.mark_containerish(x);
        a.mark_containerish(y);
        a.uf.union(&a.keys, x, y);
    }
}

/// Guarded-union activation: a guarded edge applies iff either
/// endpoint has container evidence; a nested edge (element point,
/// candidate) applies iff the CANDIDATE side has its own evidence —
/// the element side is trivially containerish, and scalars flowing
/// into elements must contribute width only (one-way constraint),
/// never glue onto the element class. Applying an edge spreads
/// evidence, so chains activate transitively across both kinds. Both
/// lists stay, applied or not: `promise_taint` reads every pair.
pub(super) fn activate_guarded(a: &mut Analysis) {
    let mut applied_g = vec![false; a.guarded_unions.len()];
    let mut applied_n = vec![false; a.nested_unions.len()];
    loop {
        let mut changed = false;
        // Evidence is filed under whatever spelling its marking site
        // saw. A guarded edge can name the same slot along another
        // path — `Field(Param("__closure", "f"), "__ret")` on the edge
        // against `Field(Field(Global(via), "__p0"), "__ret")` for the
        // evidence the argument site left. Those two agree only
        // through congruence, which `canonicalize` runs after this
        // pass, so ask in deep-canonical form rather than literally.
        let evidence: HashSet<SlotKeyId> = (a.containerish.iter())
            .map(|k| canon_key_frozen(&a.uf, &a.keys, *k))
            .collect();
        for (&(x, y), applied) in a.guarded_unions.iter().zip(&mut applied_g) {
            if !*applied
                && (evidence.contains(&canon_key_frozen(&a.uf, &a.keys, x))
                    || evidence.contains(&canon_key_frozen(&a.uf, &a.keys, y)))
            {
                a.containerish.insert(x);
                a.containerish.insert(y);
                a.uf.union(&a.keys, x, y);
                (*applied, changed) = (true, true);
            }
        }
        for (&(ek, cand), applied) in a.nested_unions.iter().zip(&mut applied_n) {
            if !*applied && evidence.contains(&canon_key_frozen(&a.uf, &a.keys, cand)) {
                a.containerish.insert(ek);
                a.uf.union(&a.keys, ek, cand);
                (*applied, changed) = (true, true);
            }
        }
        if !changed {
            break;
        }
    }
}

fn collect_subkeys(keys: &SlotKeys, k: SlotKeyId, out: &mut HashSet<SlotKeyId>) {
    let mut k = k;
    while out.insert(k) {
        match keys.data(k) {
            SlotKeyData::Elem(x) | SlotKeyData::Field(x, _) => k = x,
            _ => break,
        }
    }
}

/// Congruence closure + key rewriting. After the walk, two symbolic
/// spellings `Elem(xs)` / `Elem(ys)` with find(xs)==find(ys) denote
/// the same element class; close over that congruence, then rewrite
/// every seed / edge key onto its representative so the fixpoint
/// propagates per alias class.
pub(super) fn canonicalize(a: &mut Analysis) {
    let mut keys: HashSet<SlotKeyId> = HashSet::default();
    for s in &a.seeds {
        collect_subkeys(&a.keys, *s, &mut keys);
    }
    for (d, ts) in &a.edges {
        collect_subkeys(&a.keys, *d, &mut keys);
        for (t, _) in ts {
            collect_subkeys(&a.keys, *t, &mut keys);
        }
    }
    let uf_keys: Vec<SlotKeyId> = a.uf.keys().collect();
    for k in uf_keys {
        collect_subkeys(&a.keys, k, &mut keys);
    }

    loop {
        let mut changed = false;
        let snapshot: Vec<SlotKeyId> = keys.iter().copied().collect();
        for k in snapshot {
            let canon_inner = match a.keys.data(k) {
                SlotKeyData::Elem(x) => {
                    let fx = a.uf.find(x);
                    a.keys.mk(SlotKeyData::Elem(fx))
                }
                SlotKeyData::Field(x, n) => {
                    let fx = a.uf.find(x);
                    a.keys.mk(SlotKeyData::Field(fx, n))
                }
                _ => continue,
            };
            keys.insert(canon_inner);
            if a.uf.find(k) != a.uf.find(canon_inner) {
                a.uf.union(&a.keys, k, canon_inner);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let seeds = std::mem::take(&mut a.seeds);
    a.seeds = (seeds.iter())
        .map(|k| canon_key(&mut a.uf, &a.keys, *k))
        .collect();
    let edges = std::mem::take(&mut a.edges);
    for (d, ts) in edges {
        let cd = canon_key(&mut a.uf, &a.keys, d);
        let mut out = Vec::with_capacity(ts.len());
        for (t, g) in ts {
            out.push((canon_key(&mut a.uf, &a.keys, t), g));
        }
        a.edges.entry(cd).or_default().extend(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num_width::WidthTable;
    use crate::num_width::analyze;
    use crate::{lexer, parser};
    use std::collections::HashMap;

    fn table(src: &str) -> WidthTable {
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        analyze(
            &ast,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
    }

    fn table_classes(src: &str) -> WidthTable {
        let tokens = lexer::tokenize(src).expect("lex");
        let mut ast = parser::parse(src, &tokens).expect("parse");
        crate::ast::desugar_classes(&mut ast);
        analyze(
            &ast,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
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
    fn d5_cyclic_alias_fract_write_poisons_nominal_field() {
        let t = table(
            "type Item = { v: number; next: Item | null };\nconst c: Item = { v: 1, next: null };\nc.v = 0.25;\nconsole.log(c.v);",
        );
        assert!(t.is_nominal_alias("Item"));
        assert!(t.field_is_f64(&SlotKey::Class("Item".into()), "v"));
    }

    #[test]
    fn d5_cyclic_alias_deep_write_collapses_depth() {
        // The Field(Field(slot, next), v) spelling must canonicalize
        // onto the same nominal point as the shallow write.
        let t = table(
            "type Item = { v: number; next: Item | null };\nconst c: Item = { v: 1, next: null };\nc.next = { v: 2, next: null };\nc.next.v = 0.5;\nconsole.log(c.v);",
        );
        assert!(t.field_is_f64(&SlotKey::Class("Item".into()), "v"));
    }

    #[test]
    fn d5_deep_write_without_intermediate_assign() {
        // The Field(Field(slot, next), v) seed must collapse even when
        // `next` was never assigned a tracked container value — the
        // TypeDecl field-rule union alone closes the depth.
        let t = table(
            "type Item = { v: number; next: Item | null };\nconst c: Item = { v: 1, next: null };\nc.next.v = 0.5;\nconsole.log(c.v);",
        );
        assert!(t.field_is_f64(&SlotKey::Class("Item".into()), "v"));
    }

    #[test]
    fn d5_cyclic_alias_int_writes_stay_narrow() {
        let t = table(
            "type Counter = { n: number; next: Counter | null };\nconst k: Counter = { n: 1, next: null };\nk.n = 7;\nconsole.log(k.n);",
        );
        assert!(t.is_nominal_alias("Counter"));
        assert!(!t.field_is_f64(&SlotKey::Class("Counter".into()), "n"));
    }

    #[test]
    fn d5_param_ann_joins_nominal() {
        let t = table(
            "type Item = { v: number; next: Item | null };\nfunction bump(it: Item) { it.v = 0.5; }\nconst c: Item = { v: 1, next: null };\nbump(c);\nconsole.log(c.v);",
        );
        assert!(t.field_is_f64(&SlotKey::Class("Item".into()), "v"));
    }

    #[test]
    fn d5_mutual_recursion_joins_each_alias() {
        let t = table(
            "type A = { x: number; b: B | null };\ntype B = { y: number; a: A | null };\nconst aa: A = { x: 1, b: null };\naa.x = 1.5;\nconsole.log(aa.x);",
        );
        assert!(t.is_nominal_alias("A") && t.is_nominal_alias("B"));
        assert!(t.field_is_f64(&SlotKey::Class("A".into()), "x"));
        assert!(!t.field_is_f64(&SlotKey::Class("B".into()), "y"));
    }

    #[test]
    fn d5_acyclic_alias_keeps_slot_granularity() {
        let t = table(
            "type P = { x: number };\nconst o1: P = { x: 1 };\nconst o2: P = { x: 2 };\no1.x = 0.5;\no2.x = 3;\nconsole.log(o1.x);",
        );
        assert!(!t.is_nominal_alias("P"));
        assert!(t.field_is_f64(&g("o1"), "x"));
        assert!(!t.field_is_f64(&g("o2"), "x"));
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
