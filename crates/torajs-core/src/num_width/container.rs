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
use crate::ast::ExprId;
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

pub(super) fn canon_key_frozen(uf: &UnionFind, k: &SlotKey) -> SlotKey {
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
    /// W-ESC — frozen class reps whose containers flow into `any`-
    /// annotated slots (escape.rs); the widen site re-interns their
    /// Arr elems as Type::Any.
    any_escaped: HashSet<SlotKey>,
    uf: UnionFind,
    container_poison: bool,
    /// D5 — cyclic plain-alias names (see `alias.rs`); the TypeDecl
    /// fill site takes nominal widths for these, like classes.
    nominal_aliases: HashSet<String>,
    /// RFC 20260725-fallthrough-return knives 1-2 — functions with a
    /// path that runs off the end of the body, which answers
    /// `undefined` there. The call site asks here to know a result
    /// read off one of them may hold that answer's sentinel.
    fallthrough_fns: HashSet<String>,
    /// The mirror of `fallthrough_fns`: `(fn name, param name)` pairs
    /// some call site hands the `undefined` sentinel. The parameter's
    /// own body asks here, because the value arrives from a caller
    /// lowered separately and nothing else records it.
    undef_sentinel_params: HashSet<(String, String)>,
    /// W4 shape-join (rotation 371) — per ordered-field-name shape,
    /// the fields whose width floats on ANY same-shaped literal.
    /// Layout slot width is family-wide (same-shaped literals share
    /// a layout through the coercible first-match); operation width
    /// stays per-binding through the keys above.
    objlit_shape_f64: HashMap<Vec<String>, HashSet<String>>,
    /// The index reads the walk proved in-bounds — `xs[i]` under an
    /// enclosing, untainted `i < xs.length` guard (see
    /// [`crate::ssa_lower_bounds_proven`]). It rides here rather than
    /// beside the table because the element-width verdicts above are
    /// only sound under this very proof: the reason a `number`
    /// element widens to F64 is that an unproven read owes
    /// `undefined`, which an I64 slot cannot spell. A consumer that
    /// can reach the widths can therefore always reach the proof they
    /// were taken under, and cannot substitute another.
    index_read_proven: HashSet<ExprId>,
}

impl WidthTable {
    #[allow(clippy::too_many_arguments)] // frozen-analysis assembly, one call site
    pub(super) fn new(
        canon: HashSet<SlotKey>,
        any_escaped: HashSet<SlotKey>,
        uf: UnionFind,
        container_poison: bool,
        nominal_aliases: HashSet<String>,
        fallthrough_fns: HashSet<String>,
        undef_sentinel_params: HashSet<(String, String)>,
        objlit_shape_f64: HashMap<Vec<String>, HashSet<String>>,
        index_read_proven: HashSet<ExprId>,
    ) -> Self {
        WidthTable {
            canon,
            any_escaped,
            uf,
            container_poison,
            nominal_aliases,
            fallthrough_fns,
            undef_sentinel_params,
            objlit_shape_f64,
            index_read_proven,
        }
    }

    /// See the field's doc — the guard-dominated bounds proof the
    /// element widths were decided under.
    pub(crate) fn index_read_proven(&self, eid: ExprId) -> bool {
        self.index_read_proven.contains(&eid)
    }

    /// W4 shape-join query — true when `name` floats on any literal
    /// sharing this ordered field-name shape (see the field's doc).
    pub(crate) fn objlit_shape_field_is_f64(&self, shape: &[String], name: &str) -> bool {
        self.objlit_shape_f64
            .get(shape)
            .is_some_and(|s| s.contains(name))
    }

    /// True when `param` of `fn_name` may be handed the `undefined`
    /// sentinel by one of its call sites, so the consumers inside that
    /// body have to check for it the way they check a binding that was
    /// initialized from the same shape.
    pub(crate) fn param_takes_undef_sentinel(&self, fn_name: &str, param: &str) -> bool {
        self.undef_sentinel_params
            .contains(&(fn_name.to_string(), param.to_string()))
    }

    pub(crate) fn is_nominal_alias(&self, name: &str) -> bool {
        self.nominal_aliases.contains(name)
    }

    /// RFC 20260725-fallthrough-return knife 1 — true when calling
    /// `name` can answer `undefined` by running off the end of its
    /// body (ES §10.2.1.4 step 11) rather than through a `return`.
    pub(crate) fn returns_undef_on_fallthrough(&self, name: &str) -> bool {
        self.fallthrough_fns.contains(name)
    }

    pub(crate) fn slot_is_f64(&self, k: &SlotKey) -> bool {
        self.canon.contains(&canon_key_frozen(&self.uf, k))
    }

    /// W-ESC — true when the container held in slot `k` flows into
    /// the `any` world (its frozen class rep carries an escape face).
    pub(crate) fn slot_escapes_any(&self, k: &SlotKey) -> bool {
        !self.any_escaped.is_empty() && self.any_escaped.contains(&canon_key_frozen(&self.uf, k))
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
    let mut unions: Vec<(SlotKey, SlotKey)> = Vec::new();
    let factories: Vec<(String, String)> = a
        .ast
        .generator_factory_classes
        .iter()
        .map(|(f, c)| (f.clone(), c.clone()))
        .collect();
    for (factory, class) in factories {
        let elem = SlotKey::Elem(Box::new(SlotKey::Ret(factory)));
        for m in &fn_names {
            if *m == format!("__cm_{class}__next") || *m == format!("__cmany_{class}__next") {
                unions.push((
                    elem.clone(),
                    SlotKey::Field(Box::new(SlotKey::Ret(m.clone())), "value".to_string()),
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
    let mut fn_owner: HashMap<String, String> = HashMap::new();
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
    let mut unions: Vec<(SlotKey, SlotKey)> = Vec::new();
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
            unions.push((SlotKey::Class(ty.clone()), SlotKey::Anon(i as u32)));
        }
    }
    for (x, y) in unions {
        a.mark_containerish(&x);
        a.mark_containerish(&y);
        a.uf.union(&x, &y);
    }
}

/// Guarded-union activation: a guarded edge applies iff either
/// endpoint has container evidence; a nested edge (element point,
/// candidate) applies iff the CANDIDATE side has its own evidence —
/// the element side is trivially containerish, and scalars flowing
/// into elements must contribute width only (one-way constraint),
/// never glue onto the element class. Applying an edge spreads
/// evidence, so chains activate transitively across both kinds.
pub(super) fn activate_guarded(a: &mut Analysis) {
    let mut guarded: Vec<(SlotKey, SlotKey)> = std::mem::take(&mut a.guarded_unions);
    let mut nested: Vec<(SlotKey, SlotKey)> = std::mem::take(&mut a.nested_unions);
    loop {
        let mut changed = false;
        let mut next_g: Vec<(SlotKey, SlotKey)> = Vec::new();
        for (x, y) in guarded {
            if a.containerish.contains(&x) || a.containerish.contains(&y) {
                a.containerish.insert(x.clone());
                a.containerish.insert(y.clone());
                a.uf.union(&x, &y);
                changed = true;
            } else {
                next_g.push((x, y));
            }
        }
        guarded = next_g;
        let mut next_n: Vec<(SlotKey, SlotKey)> = Vec::new();
        for (ek, cand) in nested {
            if a.containerish.contains(&cand) {
                a.containerish.insert(ek.clone());
                a.uf.union(&ek, &cand);
                changed = true;
            } else {
                next_n.push((ek, cand));
            }
        }
        nested = next_n;
        if !changed {
            break;
        }
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
        let ast = parser::parse(src, &tokens).expect("parse");
        analyze(&ast, &HashMap::new(), &HashMap::new(), &HashMap::new())
    }

    fn table_classes(src: &str) -> WidthTable {
        let tokens = lexer::tokenize(src).expect("lex");
        let mut ast = parser::parse(src, &tokens).expect("parse");
        crate::ast::desugar_classes(&mut ast);
        analyze(&ast, &HashMap::new(), &HashMap::new(), &HashMap::new())
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
