//! W-ESC — typed-container any-escape faces (RFC
//! 20260706-typed-arr-any-escape, B1).
//!
//! A typed `Array<T>` that flows into an `any`-annotated slot
//! (`const u: any = t` / any param / any return / `any[]` element)
//! crosses into the runtime `any` world, where JS semantics allow
//! element-kind transitions (`u[0] = "s"` on a `number[]` alias) that
//! the typed tier's raw-slot loads cannot observe. The alloc site
//! itself must therefore re-intern with an Any element (NaN-box
//! slots) — decided at module level over the same container alias
//! classes W4 uses, so caller / callee / global faces never drift
//! (K.3 lesson).
//!
//! Seeds are the any-annotated slot keys themselves — the annotation
//! IS the escape sink. A container reaching such a slot shares its
//! alias class through the existing guarded-alias / container-channel
//! unions; a class no container ever reaches is simply never asked
//! about at the widen site. Conservative direction: an escaped class
//! demotes every array in it — repr cost only, never correctness
//! (same bias as the `by_name` broadcast).

use std::collections::{HashMap, HashSet, VecDeque};

use super::alias::named_ref;
use super::container::{UnionFind, canon_key_frozen};
use super::{Analysis, Scope, SlotKey};
use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};

/// Non-generic `type` / `interface` / class bodies by name — the
/// field faces an annotation naming one of them carries.
pub(super) type StructDecls = HashMap<String, Vec<(PropKey, String)>>;

/// How deep a struct annotation's field faces are followed. A named
/// type already on the path stops the descent too, so this only caps
/// long chains of distinct nested types.
const STRUCT_NEST_CAP: usize = 6;

pub(super) fn collect_struct_decls(ast: &Ast) -> StructDecls {
    let mut out = HashMap::new();
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
            && type_params.is_empty()
        {
            out.insert(name.clone(), fields.clone());
        }
    }
    out
}

impl<'a> Analysis<'a> {
    /// Record `key` as an any-escape sink when its annotation face
    /// spells a `Type::Any` source (`any` / `unknown` / `object` —
    /// the same set `parse_type` folds to Any). Depth-wrapped so
    /// `box: any[]` seeds the element point (values stored INTO the
    /// box escape; the box itself is already Arr<Any> by annotation).
    /// A struct-shaped face (`{ v: any }`, an interface, a class, an
    /// alias of one) seeds each field point whose own face is a sink:
    /// a container stored into `o.v` reaches the any world through it.
    pub(super) fn seed_any_face(&mut self, key: &SlotKey, ann: &str) {
        self.seed_any_face_on_path(key, ann, &mut Vec::new());
    }

    fn seed_any_face_on_path(&mut self, key: &SlotKey, ann: &str, path: &mut Vec<String>) {
        if map_value_escapes(ann) {
            self.any_seeds.push(key.clone());
            return;
        }
        if let Some(fields) = crate::ast_collect_fn_closure_init::parse_inlobj_field_anns(ann) {
            let fields: Vec<(PropKey, String)> = fields
                .into_iter()
                .map(|(f, a)| (PropKey::from(f), a))
                .collect();
            self.seed_field_faces(key, &fields, path);
            return;
        }
        let Some((base, depth)) = named_ref(ann) else {
            return;
        };
        let mut k = key.clone();
        for _ in 0..depth {
            k = SlotKey::Elem(Box::new(k));
        }
        if is_any_face(base) {
            self.any_seeds.push(k);
            return;
        }
        // A type already on the path has its fields seeded by the outer
        // visit (the cyclic ones join nominally through `Class`).
        if path.iter().any(|p| p == base) {
            return;
        }
        let Some(fields) = self.struct_fields(base) else {
            return;
        };
        // Too deep to follow field by field: demote the whole struct
        // point instead; propagation then reaches every field under it.
        if path.len() >= STRUCT_NEST_CAP {
            self.any_seeds.push(k);
            return;
        }
        path.push(base.to_string());
        match fields.as_slice() {
            [(f, target)] if f.lossy() == "__alias__" => {
                self.seed_any_face_on_path(&k, target, path)
            }
            _ => self.seed_field_faces(&k, &fields, path),
        }
        path.pop();
    }

    fn seed_field_faces(
        &mut self,
        key: &SlotKey,
        fields: &[(PropKey, String)],
        path: &mut Vec<String>,
    ) {
        for (f, fann) in fields {
            let fk = SlotKey::Field(Box::new(key.clone()), f.clone());
            self.seed_any_face_on_path(&fk, fann, path);
        }
    }

    /// Class and nominal-alias field faces, seeded once on the type's
    /// own aggregation point: every instance joins `Class(C)`, so an
    /// unannotated `new C()` whose `v: any` field receives a
    /// container is covered without any slot naming `C`.
    pub(super) fn seed_nominal_field_faces(&mut self) {
        let mut names: Vec<String> = self.classes.clone();
        names.extend(self.nominal_aliases.iter().cloned());
        names.sort();
        names.dedup();
        for n in names {
            if let Some(fields) = self.struct_decls.get(&n).cloned() {
                let mut path = vec![n.clone()];
                self.seed_field_faces(&SlotKey::Class(n), &fields, &mut path);
            }
        }
    }

    /// Fields of a non-generic declaration, or of a generic one
    /// instantiated by `base` (`Box<number>`) with its arguments
    /// substituted.
    fn struct_fields(&self, base: &str) -> Option<Vec<(PropKey, String)>> {
        if let Some(f) = self.struct_decls.get(base) {
            return Some(f.clone());
        }
        let open = base.find('<')?;
        let (params, fields) = self.generic_decls.get(&base[..open])?;
        let args = split_inst_args(base[open + 1..].strip_suffix('>')?);
        if args.len() != params.len() {
            return None;
        }
        let subst: Vec<(String, String)> = params
            .iter()
            .cloned()
            .zip(args.iter().map(|a| a.to_string()))
            .collect();
        Some(
            fields
                .iter()
                .map(|(f, a)| (f.clone(), crate::ssa_lower::substitute_in_ann(a, &subst)))
                .collect(),
        )
    }

    /// `e as T` re-views `e`'s own value as `T`, so `T`'s any faces
    /// apply to `e`'s container class the way an annotated slot's do —
    /// with one difference: an any-typed array target (`xs as any[]`)
    /// re-views `xs` itself, not its elements. Only the outermost
    /// target counts: `xs as unknown as number[]` hands the value back
    /// to the typed world, and the inner casts are walked through.
    pub(super) fn walk_cast(&mut self, operand: ExprId, ann: &str, scope: &Scope) {
        let mut inner = operand;
        while let Expr::As { expr, .. } = self.ast.get_expr(inner) {
            inner = *expr;
        }
        let any_target = named_ref(ann).is_some_and(|(b, _)| is_any_face(b));
        let struct_target = !any_target
            && (crate::ast_collect_fn_closure_init::parse_inlobj_field_anns(ann).is_some()
                || named_ref(ann).is_some_and(|(b, _)| self.struct_fields(b).is_some()));
        if (any_target || struct_target)
            && let Some(k) = self.container_key_of(inner, scope)
        {
            if any_target {
                self.any_seeds.push(k);
            } else {
                self.seed_any_face(&k, ann);
            }
        }
        self.walk_expr(inner, scope);
    }
}

/// The annotation spellings `parse_type` folds to Any.
fn is_any_face(base: &str) -> bool {
    matches!(base, "any" | "unknown" | "object")
}

/// A generic instantiation's argument list (`number|Pair<a|b>`), split
/// at depth-0 `|` — the instantiation key spells its argument
/// separator that way.
pub(super) fn split_inst_args(inner: &str) -> Vec<&str> {
    let mut args: Vec<&str> = Vec::new();
    let mut d: i32 = 0;
    let mut last = 0usize;
    for (i, &b) in inner.as_bytes().iter().enumerate() {
        match b {
            b'<' | b'(' => d += 1,
            b'>' | b')' => d -= 1,
            b'|' if d == 0 => {
                args.push(&inner[last..i]);
                last = i + 1;
            }
            _ => {}
        }
    }
    if !inner.is_empty() {
        args.push(&inner[last..]);
    }
    args
}

/// A `Map` annotation whose value face is not `number` (`Map<K, any>`,
/// a union, a type parameter): a `Map<K, number>` handed to it may be
/// written any value there, so the typed value lane
/// (`ssa_lower_map_num_lane`) must not claim the Map.
fn map_value_escapes(ann: &str) -> bool {
    let t = crate::check_type_ann::strip_nullable(ann.trim());
    let Some(args) = t.strip_prefix("Map<").and_then(|r| r.strip_suffix('>')) else {
        return false;
    };
    match crate::check_type_ann::split_top_pipe(args).as_slice() {
        [_, v] => v.trim() != "number",
        _ => true,
    }
}

/// Post-canonicalize propagation: map every seed onto its frozen
/// class representative, then push the escape down the Elem chain —
/// a demoted outer array's inner arrays must demote too (the any
/// side reaches them through the outer's NaN-box slots). Depth-
/// capped: real TS nesting is shallow and an unrepresented Elem key
/// canonicalizes to itself, so the chain would otherwise walk
/// forever.
pub(super) fn propagate(seeds: &[SlotKey], uf: &UnionFind) -> HashSet<SlotKey> {
    const ELEM_DEPTH_CAP: usize = 8;
    let mut out: HashSet<SlotKey> = HashSet::new();
    let mut work: VecDeque<(SlotKey, usize)> = seeds
        .iter()
        .map(|k| (canon_key_frozen(uf, k), 0usize))
        .collect();
    // Field points the walk knows about: an escaped object hands the
    // any side every container it holds (`(o as any).xs.push("s")`
    // writes into `o.xs`'s block), so those classes escape too.
    let projections: HashSet<&SlotKey> = uf
        .keys()
        .filter(|k| matches!(k, SlotKey::Field(..)))
        .collect();
    loop {
        while let Some((k, d)) = work.pop_front() {
            if out.insert(k.clone()) && d < ELEM_DEPTH_CAP {
                let ek = canon_key_frozen(uf, &SlotKey::Elem(Box::new(k)));
                if !out.contains(&ek) {
                    work.push_back((ek, d + 1));
                }
            }
        }
        for p in &projections {
            let SlotKey::Field(base, _) = p else {
                continue;
            };
            let ck = canon_key_frozen(uf, p);
            if !out.contains(&ck) && out.contains(&canon_key_frozen(uf, base)) {
                work.push_back((ck, 0));
            }
        }
        if work.is_empty() {
            break;
        }
    }
    // TORAJS_NUM_WIDTH_STATS=1 — one line per escaped representative.
    if std::env::var_os("TORAJS_NUM_WIDTH_STATS").is_some() {
        let mut lines: Vec<String> = out.iter().map(|k| format!("{k:?}")).collect();
        lines.sort();
        for l in &lines {
            eprintln!("[num_width] any-escape class: {l}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{SlotKey, analyze};
    use crate::{lexer, parser};
    use std::collections::HashMap;

    fn escaped(src: &str, names: &[&str]) -> Vec<bool> {
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        let t = analyze(&ast, &HashMap::new(), &HashMap::new(), &HashMap::new());
        names
            .iter()
            .map(|n| t.slot_escapes_any(&SlotKey::Global((*n).into())))
            .collect()
    }

    #[test]
    fn cast_operand_escapes_and_bystander_does_not() {
        let src = "const c: number[] = [1];\nconst d: number[] = [2];\n(c as any).push(\"s\");\nd.push(3);";
        assert_eq!(escaped(src, &["c", "d"]), [true, false]);
    }

    #[test]
    fn double_cast_and_unknown_cast_escape() {
        let src = "const c: number[] = [1];\nconst d: number[] = [2];\n(c as unknown as any).push(1);\nconsole.log(d as unknown);";
        assert_eq!(escaped(src, &["c", "d"]), [true, true]);
    }

    #[test]
    fn cast_back_to_typed_does_not_escape() {
        let src = "const xs: number[] = [1];\nconst ys = xs as unknown as number[];\nys.push(2);\nconst zs: number[] = [3];\n(zs as any as number[]).push(4);";
        assert_eq!(escaped(src, &["xs", "zs"]), [false, false]);
    }

    #[test]
    fn struct_target_cast_escapes_its_any_fields() {
        let src = "interface B { v: any; w: number[] }\nconst o = { v: [1], w: [2] };\n(o as B).v.push(3);\nconst p = { u: [4] };\n(p as { u: any }).u.push(5);";
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        let t = analyze(&ast, &HashMap::new(), &HashMap::new(), &HashMap::new());
        let field = |o: &str, f: &str| {
            t.slot_escapes_any(&SlotKey::Field(
                Box::new(SlotKey::Global(o.into())),
                f.into(),
            ))
        };
        assert_eq!(
            [field("o", "v"), field("o", "w"), field("p", "u")],
            [true, false, true]
        );
    }

    #[test]
    fn any_field_past_the_nesting_cap_still_escapes() {
        let mut src = String::new();
        for i in 1..=8 {
            src.push_str(&format!("type T{i} = {{ a: T{} }};\n", i + 1));
        }
        src.push_str("type T9 = { v: any };\nconst c: number[] = [1];\n");
        src.push_str(
            "const t: T1 = { a: { a: { a: { a: { a: { a: { a: { a: { v: c } } } } } } } } };",
        );
        assert_eq!(escaped(&src, &["c"]), [true]);
    }

    #[test]
    fn generic_struct_any_field_escapes() {
        let src = "interface Box<T> { v: any; t: T }\nconst c: number[] = [1];\nconst b: Box<number> = { v: c, t: 1 };";
        assert_eq!(escaped(src, &["c"]), [true]);
    }

    #[test]
    fn typed_cast_does_not_escape() {
        let src = "const c: number[] = [1];\nconst e = c as number[];\ne.push(2);";
        assert_eq!(escaped(src, &["c"]), [false]);
    }

    #[test]
    fn any_field_of_named_and_inline_types_escapes() {
        let src = "interface B { v: any; n: number[] }\nconst c: number[] = [1];\nconst m: number[] = [2];\nconst b: B = { v: c, n: m };\nconst d: number[] = [3];\nconst o: { w: any } = { w: d };";
        assert_eq!(escaped(src, &["c", "m", "d"]), [true, false, true]);
    }

    #[test]
    fn container_held_by_escaped_object_escapes() {
        let src = "const xs: number[] = [1];\nconst ys: number[] = [2];\nconst o = { xs: xs };\nconst p = { ys: ys };\nconst a: any = o;\nconsole.log(p.ys);";
        assert_eq!(escaped(src, &["xs", "ys"]), [true, false]);
    }
}
