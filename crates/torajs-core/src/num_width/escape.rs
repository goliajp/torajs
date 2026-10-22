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

use std::collections::HashMap;

use super::alias::named_ref;
use super::{Analysis, Scope, SlotKeyId};
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
    /// box escape) and the box point itself (a typed array handed to
    /// the box is written values of any kind through it).
    /// A struct-shaped face (`{ v: any }`, an interface, a class, an
    /// alias of one) seeds each field point whose own face is a sink:
    /// a container stored into `o.v` reaches the any world through it.
    /// `holders` is false for a face the compiler spelled (an
    /// any-widened clone's `any[]` parameter, a rest parameter): its
    /// arrays are the program's typed ones re-viewed, not handed to a
    /// slot the program declared writable with any value.
    pub(super) fn seed_any_face(&mut self, key: SlotKeyId, ann: &str, holders: bool) {
        self.seed_any_face_on_path(key, ann, &mut Vec::new(), false, holders);
    }

    /// An any face on the slot itself seeds both tables; a face found
    /// by descending a struct annotation (`field`) goes to
    /// `escape_only_seeds`: it demotes the containers held in that
    /// field, but it is not a number slot of the program's and must
    /// not seed the F64 fixpoint (a generator's `value: any` step
    /// field would otherwise widen every yielded integer).
    fn push_any_seed(&mut self, k: SlotKeyId, field: bool) {
        if field {
            self.escape_only_seeds.push(k);
        } else {
            self.any_seeds.push(k);
        }
    }

    fn seed_any_face_on_path(
        &mut self,
        key: SlotKeyId,
        ann: &str,
        path: &mut Vec<String>,
        field: bool,
        holders: bool,
    ) {
        if holds_undefined_number(ann) {
            self.seeds.push(key);
            return;
        }
        if map_value_escapes(ann) {
            self.push_any_seed(key, field);
            return;
        }
        if let Some(fields) = crate::ast_collect_fn_closure_init::parse_inlobj_field_anns(ann) {
            let fields: Vec<(PropKey, String)> = fields
                .into_iter()
                .map(|(f, a)| (PropKey::from(f), a))
                .collect();
            self.seed_field_faces(key, &fields, path, holders);
            return;
        }
        // A union slot holds whichever member's value it is handed, so
        // every member's faces apply to it (`xs: (string | number)[] |
        // any[]` writes values of any kind through the `any[]` member).
        if let Some(members) = union_members(ann) {
            for m in members {
                self.seed_any_face_on_path(key, m, path, field, holders);
            }
            return;
        }
        // An array whose element face is a union (`(string | number)[]`)
        // is written values of more than one kind: a typed array handed
        // to it takes the other kinds through it, as through `any[]`.
        if let Some(depth) = union_elem_depth(ann) {
            if holders {
                let mut h = key;
                for _ in 1..depth {
                    h = self.keys.elem(h);
                }
                self.escape_only_seeds.push(h);
            }
            return;
        }
        let Some((base, depth)) = named_ref(ann) else {
            return;
        };
        let mut k = key;
        let mut holder = None;
        for _ in 0..depth {
            holder = Some(k);
            k = self.keys.elem(k);
        }
        if is_any_face(base) {
            self.push_any_seed(k, field);
            // the array whose elements the face spells `any` is written
            // values of any kind (`xs.push("s")` through `xs: any[]`),
            // so a typed array reaching that slot escapes as it would
            // through `any`; it is no number slot of the program's
            if let Some(h) = holder.filter(|_| holders) {
                self.escape_only_seeds.push(h);
            }
            return;
        }
        // A type already on the path has its fields seeded by the outer
        // visit (the cyclic ones join nominally through `Class`).
        if path.iter().any(|p| p == base) {
            return;
        }
        // A desugar-spelled class (a generator state class with its
        // hoisted locals as `any` fields) is a compiler slot, not a
        // face the program hands to the any world.
        if base.starts_with("__") {
            return;
        }
        let Some(fields) = self.struct_fields(base) else {
            return;
        };
        // Too deep to follow field by field: demote the whole struct
        // point instead; propagation then reaches every field under it.
        if path.len() >= STRUCT_NEST_CAP {
            self.push_any_seed(k, field);
            return;
        }
        path.push(base.to_string());
        match fields.as_slice() {
            [(f, target)] if f.lossy() == "__alias__" => {
                self.seed_any_face_on_path(k, target, path, field, holders)
            }
            _ => self.seed_field_faces(k, &fields, path, holders),
        }
        path.pop();
    }

    fn seed_field_faces(
        &mut self,
        key: SlotKeyId,
        fields: &[(PropKey, String)],
        path: &mut Vec<String>,
        holders: bool,
    ) {
        for (f, fann) in fields {
            let fk = self.keys.field(key, f);
            self.seed_any_face_on_path(fk, fann, path, true, holders);
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
            if n.starts_with("__") {
                continue;
            }
            if let Some(fields) = self.struct_decls.get(&n).cloned() {
                let mut path = vec![n.clone()];
                self.seed_field_faces(self.keys.class(&n), &fields, &mut path, true);
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
        let (params, fields, _) = self.generic_decls.get(&base[..open])?;
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
        // A cast reads the value; it writes nothing back into the
        // operand's own slot, so a scalar operand keeps its width (the
        // generator desugar yields through `v as any`, and the F64
        // seed widened every yielded integer). Escape only.
        if (any_target || struct_target)
            && let Some(k) = self.container_key_of(inner, scope)
        {
            if any_target {
                self.escape_only_seeds.push(k);
            } else {
                self.seed_any_face_on_path(k, ann, &mut Vec::new(), true, true);
            }
        }
        self.walk_expr(inner, scope);
    }
}

/// The annotation spellings `parse_type` folds to Any.
/// A `number | undefined` slot (`x?: number`, `o: { a?: number }`,
/// `let x: number;`): it materializes as a boxed `any`, and what it
/// hands an arithmetic or a slot downstream is §7.1.4
/// ToNumber(undefined), which is NaN. The slot's own repr is fixed by
/// the annotation, but every slot its value flows into must be able
/// to spell NaN — an I64 one truncated `x * 2` on an absent `x` to 0.
/// Seeding the slot F64 sends that through the fixpoint.
fn holds_undefined_number(ann: &str) -> bool {
    ann.strip_prefix("__nullable(")
        .and_then(|r| r.strip_suffix(')'))
        .is_some_and(|inner| inner.trim() == "number")
}

/// The members of a union face `__union(A|B)`, under an optional
/// `__nullable(..)` wrapper.
fn union_members(ann: &str) -> Option<Vec<&str>> {
    let inner = union_inner(crate::check_type_ann::strip_nullable(ann.trim()))?;
    Some(crate::check_type_ann::split_top_pipe(inner))
}

/// How many `[]` an array of a union element (`__union(A|B)[]`,
/// `__nullable(__union(A|B)[])[]`) is deep, or None for any other face.
fn union_elem_depth(ann: &str) -> Option<usize> {
    let mut t = ann.trim();
    let mut depth = 0;
    loop {
        if let Some(inner) = t
            .strip_prefix("__nullable(")
            .and_then(|r| r.strip_suffix(')'))
        {
            t = inner.trim();
        } else if let Some(inner) = t.strip_suffix("[]") {
            t = inner.trim_end();
            depth += 1;
        } else {
            break;
        }
    }
    (depth > 0 && union_inner(t).is_some()).then_some(depth)
}

/// The text inside `__union( .. )` when that marker's parenthesis is
/// the one closing `t`.
fn union_inner(t: &str) -> Option<&str> {
    let inner = t.strip_prefix("__union(")?.strip_suffix(')')?;
    let mut d: i32 = 0;
    for b in inner.bytes() {
        match b {
            b'(' => d += 1,
            b')' => {
                d -= 1;
                if d < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (d == 0).then_some(inner)
}

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

#[cfg(test)]
mod tests {
    use super::super::{SlotKey, analyze};
    use crate::{lexer, parser};
    use std::collections::HashMap;

    fn escaped(src: &str, names: &[&str]) -> Vec<bool> {
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        let t = analyze(
            &ast,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
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
        let t = analyze(
            &ast,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
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
    fn written_union_array_params_escape() {
        let src = "function f(xs: (string | number)[]) { xs.push(\"s\") }\nfunction g(xs: (string | number)[] | any[]) { xs.push(1) }\nfunction h(xs: (string | number)[] | undefined) { xs!.push(1) }\nfunction r(xs: (string | number)[]) { return xs.length }\nconst a: number[] = [1];\nconst b: string[] = [\"x\"];\nconst c: number[] = [2];\nconst d: number[] = [3];\nf(a); g(b); h(c); r(d);";
        assert_eq!(
            escaped(src, &["a", "b", "c", "d"]),
            [true, true, true, false]
        );
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

    #[test]
    fn value_of_an_optional_number_reaches_its_consumers_as_f64() {
        let src = "function t1(x?: number) { return x * 2 }\nfunction t2(o: { a?: number }) { return o.a * 2 }\nfunction t3(x: number) { return x * 2 }\nconsole.log(t1(), t2({}), t3(1));";
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        let t = analyze(
            &ast,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let ret = |f: &str| t.slot_is_f64(&SlotKey::Ret(f.into()));
        assert_eq!([ret("t1"), ret("t2"), ret("t3")], [true, true, false]);
    }
}
