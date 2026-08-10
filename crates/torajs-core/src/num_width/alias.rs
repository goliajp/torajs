//! D5 (ann-width RFC §5.4) — cyclic type-alias nominal hookups.
//!
//! A self-referential struct type can only be spelled through a named
//! alias (`type Item = { v: number; next: Item | null }`), so a write
//! like `c.next.v = 0.5` produces `Field(Field(slot, next), v)` keys
//! of unbounded depth — no slot-granular key set can cover them. The
//! field-width decision for a cyclic alias must therefore join at
//! *type* granularity, exactly like classes: every `class C` instance
//! shares one layout, and `nominal_unions` (container.rs) glues all
//! of them onto `SlotKey::Class(C)`. Aliases have no constructor to
//! hook, so the gluing points are the annotation spellings instead:
//! let / param / ret annotations naming a cyclic alias, and named-type
//! fields whose annotation names one (the latter collapses the
//! recursion depth — `Field(Class(Item), next)` ∪ `Class(Item)` makes
//! every `c.next.…next.v` spelling canonicalize onto one point).
//!
//! `SlotKey::Class` is reused for aliases: classes desugar into the
//! same TypeDecl namespace (no name collision is possible) and the
//! semantics — nominal aggregation point of a named type — are
//! identical, so a separate variant would just split one mechanism
//! in two.
//!
//! Nominalized aliases: *cyclic* ones (their single reserved layout
//! is shared by every recursion depth) and generator *step* aliases
//! (`__step_<gen>`, §5.6 F3 — single synthesized producer, so the
//! nominal join loses no precision and the state machine's value
//! slot negotiates with every yielded expression). All other plain
//! aliases keep the D3 per-consuming-slot precision (widened twins
//! allow differently-classed slots of one spelling to carry
//! different widths).

use super::{Analysis, SlotKey};
use crate::ast::{Ast, Stmt};
use std::collections::{HashMap, HashSet};

/// Plain alias names whose field widths join at *type* granularity
/// (nominal, like classes): cyclic aliases (their single reserved
/// layout is shared by every recursion depth) plus generator step
/// aliases (`__step_<gen>` — synthesized with a single producer, the
/// state machine's yield returns, so nominal granularity loses no
/// precision; §5.6 F3). All other plain aliases keep the D3
/// per-consuming-slot precision.
pub(crate) fn nominal_alias_names(ast: &Ast) -> HashSet<String> {
    let mut out = cyclic_alias_names(ast);
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name, type_params, ..
        } = stmt
            && type_params.is_empty()
            && name.starts_with("__step_")
        {
            out.insert(name.clone());
        }
    }
    // A method-bearing object literal's synthetic `__ObjLit_<n>`
    // TypeDecl (ast/objlit_nominal.rs) has a single producer — the
    // literal itself — so the nominal join loses no precision, same
    // rationale as `__step_`. Without the join the TypeDecl parse
    // keeps static widths while the literal's layout takes the method
    // fns' analyzed widths, splitting one logical type into twin sids:
    // `is_objlit_method_slot` then misses (receiver never pushed), and
    // a sibling `this.m()` reads the ret off the wrong register lane.
    out.extend(ast.objlit_method_fields.keys().cloned());
    out
}

/// Plain (non-class, non-generic, struct-shaped) alias names that sit
/// on a cycle in the named-type reference graph. Classes participate
/// as graph nodes — a `type A = { n: Node | null }` ↔ `class Node {
/// a: A | null }` mutual cycle makes `A` cyclic — but only plain
/// aliases land in the result; classes already have their nominal
/// path. Mentions are token-scanned, so exotic spellings (inline
/// nests, generics) over-approximate toward cyclic — width-only cost.
fn cyclic_alias_names(ast: &Ast) -> HashSet<String> {
    let mut mentions: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut aliases: HashSet<&str> = HashSet::new();
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
        {
            if !type_params.is_empty() || (fields.len() == 1 && fields[0].0 == "__alias__") {
                continue;
            }
            if !ast.class_parents.contains_key(name) {
                aliases.insert(name);
            }
            mentions.entry(name).or_default();
            for (_, ann) in fields {
                for tok in ident_tokens(ann) {
                    mentions.entry(name).or_default().insert(tok);
                }
            }
        }
    }
    let mut cyclic = HashSet::new();
    for &a in &aliases {
        if reaches(a, a, &mentions, &mut HashSet::new()) {
            cyclic.insert(a.to_string());
        }
    }
    cyclic
}

fn reaches<'x>(
    from: &'x str,
    target: &str,
    mentions: &HashMap<&'x str, HashSet<&'x str>>,
    seen: &mut HashSet<&'x str>,
) -> bool {
    let Some(nexts) = mentions.get(from) else {
        return false;
    };
    for &n in nexts {
        if n == target {
            return true;
        }
        if mentions.contains_key(n) && seen.insert(n) && reaches(n, target, mentions, seen) {
            return true;
        }
    }
    false
}

/// Maximal identifier runs in an annotation spelling.
fn ident_tokens(ann: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = ann.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < bytes.len() {
                let d = bytes[i] as char;
                if d.is_ascii_alphanumeric() || d == '_' || d == '$' {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(&ann[start..i]);
        } else {
            i += 1;
        }
    }
    out
}

/// `(name, array depth)` when an annotation spelling is a plain named
/// reference: optionally null-unioned — the parser normalizes
/// `T | null` to a `__nullable(T)` wrapper — and optionally
/// `[]`-suffixed per array layer (inside or outside the wrapper).
/// Raw `| null` / `| undefined` arms are also peeled in case an
/// un-normalized spelling reaches here. Anything richer (inline
/// objects, `__fn(..)`, multi-arm unions) returns None — the union is
/// skipped and the layout keeps its parse width, so a fract write
/// stays the loud assign-mismatch panic, never silent-wrong.
pub(super) fn named_ref(ann: &str) -> Option<(&str, usize)> {
    let mut base: Option<&str> = None;
    for arm in ann.split('|') {
        let t = arm.trim();
        if t == "null" || t == "undefined" {
            continue;
        }
        if base.is_some() {
            return None;
        }
        base = Some(t);
    }
    let mut t = base?;
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
    let mut chars = t.chars();
    let head = chars.next()?;
    if !(head.is_ascii_alphabetic() || head == '_' || head == '$') {
        return None;
    }
    if chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$') {
        return Some((t, depth));
    }
    // Generic instantiation spelling `Head<...>` — the full text is
    // the nominal key (its own canonical annotation). The caller's
    // nominal-set gate decides whether it participates; built-in
    // generics (`Promise<T>`, `Map<K,V>`) never register there.
    if let Some(open) = t.find('<')
        && t.ends_with('>')
        && t[..open]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return Some((t, depth));
    }
    None
}

impl<'a> Analysis<'a> {
    /// Union an annotated slot onto its cyclic alias's nominal key
    /// (no-op for every other annotation). `Item[]` wraps the slot key
    /// in `Elem` per array layer — the *elements* are the instances.
    pub(super) fn alias_ann_union(&mut self, key: &SlotKey, ann: &str) {
        let Some((base, depth)) = named_ref(ann) else {
            return;
        };
        if !self.nominal_aliases.contains(base) {
            // Generic instantiation key — register lazily on first
            // sighting; built-ins / unknown heads fall through to the
            // D3 per-slot path.
            if !(base.contains('<') && self.register_inst_key(&base.to_string())) {
                return;
            }
        }
        let mut k = key.clone();
        for _ in 0..depth {
            k = SlotKey::Elem(Box::new(k));
        }
        let ck = SlotKey::Class(base.to_string());
        self.mark_containerish(&ck);
        self.mark_containerish(&k);
        self.uf.union(&ck, &k);
    }

    /// First sighting of a generic instantiation key (`Rec<number>`):
    /// admit it to the nominal set and union its substituted field
    /// annotations onto the key's Class projections. Lowering reserves
    /// one memo'd layout per key shared by every consuming slot, so
    /// widths must join at type granularity — per-slot widths would be
    /// lowering-order sensitive. Recursive fields re-enter
    /// `alias_ann_union`, which registers indirect keys (`Rec<number>`
    /// surfacing only inside `Wrap<number>`'s substituted fields) the
    /// same way; the nominal-set insert above the recursion makes the
    /// closure terminate. Returns false for non-generic heads and bare
    /// `__alias__` decls (those resolve to their underlying type — no
    /// struct layout, nothing to join).
    fn register_inst_key(&mut self, key: &String) -> bool {
        let Some(open) = key.find('<') else {
            return false;
        };
        let Some((tp_names, fields)) = self.generic_decls.get(&key[..open]).cloned() else {
            return false;
        };
        if fields.len() == 1 && fields[0].0 == "__alias__" {
            return false;
        }
        if self.nominal_aliases.contains(key) {
            return true;
        }
        self.nominal_aliases.insert(key.clone());
        let inner = &key[open + 1..key.len() - 1];
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
        if args.len() != tp_names.len() {
            return true; // malformed spelling; lowering rejects loudly
        }
        let subst: Vec<(String, String)> = tp_names
            .into_iter()
            .zip(args.iter().map(|a| a.to_string()))
            .collect();
        for (fname, fann) in &fields {
            let substituted = crate::ssa_lower::substitute_in_ann(fann, &subst);
            let fk = SlotKey::Field(Box::new(SlotKey::Class(key.clone())), fname.clone());
            self.alias_ann_union(&fk, &substituted);
        }
        true
    }

    /// Annotation-driven nominal hookups, the alias analog of
    /// `nominal_unions`: fn signatures and named-type fields naming a
    /// cyclic alias glue onto its Class key. The field rule is the
    /// recursion-depth collapse point.
    pub(super) fn alias_nominal_unions(&mut self) {
        let ast = self.ast;
        for stmt in &ast.stmts {
            match stmt {
                Stmt::FnDecl {
                    name,
                    params,
                    return_type,
                    ..
                } => {
                    for p in params {
                        if let Some(ann) = &p.type_ann {
                            let k = SlotKey::Param(name.clone(), p.name.clone());
                            self.alias_ann_union(&k, ann);
                        }
                    }
                    if let Some(ann) = return_type {
                        self.alias_ann_union(&SlotKey::Ret(name.clone()), ann);
                    }
                }
                Stmt::TypeDecl {
                    name,
                    type_params,
                    fields,
                } => {
                    if !type_params.is_empty() {
                        continue;
                    }
                    for (fname, ann) in fields {
                        let fk =
                            SlotKey::Field(Box::new(SlotKey::Class(name.clone())), fname.clone());
                        self.alias_ann_union(&fk, ann);
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    fn cyclic(src: &str) -> HashSet<String> {
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        cyclic_alias_names(&ast)
    }

    #[test]
    fn self_referential_alias_is_cyclic() {
        let c = cyclic("type Item = { v: number; next: Item | null };");
        assert!(c.contains("Item"));
    }

    #[test]
    fn step_alias_is_nominal_but_not_cyclic() {
        let src = "type __step_g = { value: number; done: boolean };";
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        assert!(nominal_alias_names(&ast).contains("__step_g"));
        assert!(cyclic_alias_names(&ast).is_empty());
    }

    #[test]
    fn acyclic_alias_is_not() {
        let c = cyclic("type P = { x: number };\ntype Q = { p: P | null };");
        assert!(c.is_empty());
    }

    #[test]
    fn mutual_recursion_marks_both() {
        let c =
            cyclic("type A = { x: number; b: B | null };\ntype B = { y: number; a: A | null };");
        assert!(c.contains("A") && c.contains("B"));
    }

    #[test]
    fn array_face_cycle_detected() {
        let c = cyclic("type Tree = { w: number; kids: Tree[] };");
        assert!(c.contains("Tree"));
    }

    #[test]
    fn named_ref_spellings() {
        assert_eq!(named_ref("Item"), Some(("Item", 0)));
        assert_eq!(named_ref("Item | null"), Some(("Item", 0)));
        assert_eq!(named_ref("null | Item"), Some(("Item", 0)));
        assert_eq!(named_ref("Item[]"), Some(("Item", 1)));
        assert_eq!(named_ref("Item[][]"), Some(("Item", 2)));
        // parser normal form for `T | null`
        assert_eq!(named_ref("__nullable(Item)"), Some(("Item", 0)));
        assert_eq!(named_ref("__nullable(Item[])"), Some(("Item", 1)));
        assert_eq!(named_ref("__nullable(Item)[]"), Some(("Item", 1)));
        assert_eq!(named_ref("A | B"), None);
        assert_eq!(named_ref("{ x: Item }"), None);
        assert_eq!(named_ref("__fn(A)->B"), None);
        assert_eq!(named_ref("number"), Some(("number", 0)));
    }
}
