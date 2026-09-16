//! LetDecl-init store-site helpers for
//! [`crate::ast_collect_fn_closure::FnToClosureCollector`]
//! (chunk 789 extraction — the member-chain receiver resolution
//! pushed the collector file past the 500-line limit), plus the
//! chunk-793 declared-annotation field resolution shared by every
//! wrap axis (named TypeDecl and inline `__inlobj` object types
//! resolve uniformly).

use crate::ast::PropKey;
use std::collections::HashMap;

use crate::ast::{Ast, BinOp, Expr, ExprId};

use crate::ast_collect_fn_closure::{FnToClosureCollector, is_fn_like_field_ann, strip_arr_ann};

/// Chunk 793 — decode an inline object-type annotation
/// (`__inlobj(f:ann|...)`, optionally `__nullable(...)`-wrapped)
/// into its field→annotation map; `None` for any other ann shape.
/// Splits on the checker's own `check_type_ann::split_top_pipe`, so
/// the two readers of this encoding cannot drift apart. The private
/// copy this replaced nested parens only, which cut a field spelled
/// with a multi-argument generic (`{ m: Map<string, number> }` →
/// `__inlobj(m:Map<string|number>)`) in half.
pub(crate) fn parse_inlobj_field_anns(ann: &str) -> Option<HashMap<String, String>> {
    let t = ann.trim();
    let t = t
        .strip_prefix("__nullable(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(t);
    let body = t.strip_prefix("__inlobj(")?.strip_suffix(')')?;
    let mut map = HashMap::new();
    if body.trim().is_empty() {
        return Some(map);
    }
    for seg in crate::check_type_ann::split_top_pipe(body) {
        let colon = seg.find(':')?;
        map.insert(
            seg[..colon].trim().to_string(),
            seg[colon + 1..].trim().to_string(),
        );
    }
    Some(map)
}

/// Chunk 795 — split a generic-instantiation arg list at depth-0
/// `|` (paren + angle nesting; the `>` of a fn-type return arrow is
/// not a closer — chunk-794 splitter-family mirror).
fn split_generic_args(inner: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = Vec::new();
    let mut depth: i32 = 0;
    let mut last = 0usize;
    let mut prev: u8 = 0;
    for (i, &b) in inner.as_bytes().iter().enumerate() {
        match b {
            b'(' | b'<' => depth += 1,
            b'>' if prev == b'-' => {}
            b')' | b'>' => depth -= 1,
            b'|' if depth == 0 => {
                parts.push(&inner[last..i]);
                last = i + 1;
            }
            _ => {}
        }
        prev = b;
    }
    if !inner.is_empty() {
        parts.push(&inner[last..]);
    }
    parts
}

impl<'a> FnToClosureCollector<'a> {
    /// Chunk 789 — resolve the declared struct field→ann map of a
    /// member-assign receiver chain (chunk 793: named TypeDecl and
    /// inline object annotations both resolve, via
    /// `resolve_field_anns`): an Ident hits the binding map; a
    /// Member link resolves the outer struct's field annotation
    /// (stripping an optional `__nullable(...)` wrapper); an Index
    /// (chunk 790) resolves the container's declared array
    /// annotation and answers its element shape.
    pub(crate) fn resolve_receiver_fields(&self, eid: ExprId) -> Option<HashMap<PropKey, String>> {
        match self.ast.get_expr(eid) {
            Expr::Ident(n) => {
                let ann = self.struct_bindings.get(n)?;
                self.resolve_field_anns(ann)
            }
            Expr::Member { obj, name } => {
                let outer = self.resolve_receiver_fields(*obj)?;
                let fann = outer.get(&PropKey::from(name))?;
                let inner = fann
                    .strip_prefix("__nullable(")
                    .and_then(|r| r.strip_suffix(')'))
                    .unwrap_or(fann)
                    .trim();
                self.resolve_field_anns(inner)
            }
            Expr::Index { obj, .. } => {
                let container_ann: String = match self.ast.get_expr(*obj) {
                    Expr::Ident(n) => self.struct_arr_bindings.get(n)?.clone(),
                    Expr::Member { obj: mobj, name } => {
                        let outer = self.resolve_receiver_fields(*mobj)?;
                        outer.get(&PropKey::from(name))?.clone()
                    }
                    _ => return None,
                };
                let elem = strip_arr_ann(&container_ann)?;
                self.resolve_field_anns(elem)
            }
            _ => None,
        }
    }

    /// Chunk 793 — resolve a declared annotation to its struct
    /// field→ann map: a known TypeDecl name answers its snapshot,
    /// an inline object type decodes in place, and a generic
    /// instantiation (`Box<() => number>` — chunk 795) substitutes
    /// its type args into the generic decl's fields (same dance as
    /// `fill_optional_fields::instantiate_generic`). Every wrap
    /// axis resolves through here so all spellings behave
    /// identically.
    pub(crate) fn resolve_field_anns(&self, ann: &str) -> Option<HashMap<PropKey, String>> {
        let t = ann.trim();
        if let Some(m) = self.struct_field_anns.get(t) {
            return Some(m.clone());
        }
        if let Some(m) = parse_inlobj_field_anns(t) {
            return Some(m.into_iter().map(|(k, v)| (PropKey::from(k), v)).collect());
        }
        let open_idx = t.find('<')?;
        if !t.ends_with('>') {
            return None;
        }
        let (tp_names, fields) = self.generic_field_anns.get(&t[..open_idx])?;
        let args = split_generic_args(&t[open_idx + 1..t.len() - 1]);
        if args.len() != tp_names.len() {
            return None;
        }
        let subst: Vec<(String, String)> = tp_names
            .iter()
            .cloned()
            .zip(args.iter().map(|s| s.trim().to_string()))
            .collect();
        Some(
            fields
                .iter()
                .map(|(n, a)| {
                    (
                        n.clone(),
                        crate::check_type_ann_substitute::ann_substitute(a, &subst),
                    )
                })
                .collect(),
        )
    }

    /// Mark bare top-FnDecl Ident values of `eid`'s ObjectLit fields
    /// whose declared field annotation is fn-like (Closure-repr
    /// slot). Shared by the let-init and call-arg objlit axes.
    pub(crate) fn mark_objlit_fn_fields(
        &mut self,
        eid: ExprId,
        field_anns: &HashMap<PropKey, String>,
    ) {
        if let Expr::ObjectLit { fields } = self.ast.get_expr(eid) {
            for (fname, feid) in fields.clone() {
                if field_anns
                    .get(&fname)
                    .is_some_and(|a| is_fn_like_field_ann(a))
                {
                    self.try_mark(feid);
                }
            }
        }
    }

    /// `const o: T = { k: v, ... }` where `T` resolves to a struct
    /// shape (named TypeDecl or inline object type — chunk 793)
    /// whose field `k` is fn-typed and `v` is a bare top-FnDecl
    /// Ident.
    pub(crate) fn collect_objectlit_field_sites(&mut self, init: ExprId, type_ann: Option<&str>) {
        let Some(ann) = type_ann else { return };
        let Some(field_anns) = self.resolve_field_anns(ann) else {
            return;
        };
        self.mark_objlit_fn_fields(init, &field_anns);
    }

    /// Chunk 733 — mark every bare top-FnDecl Ident element of an
    /// array literal destined for a fn-typed array slot (let-init /
    /// call-arg positions).
    pub(crate) fn mark_array_lit_elems(&mut self, eid: ExprId) {
        if let Expr::Array(els) = self.ast.get_expr(eid) {
            for e in els.clone() {
                self.try_mark(e);
            }
        }
    }

    /// `const f: any = top_fn` (chunk 518) and ObjectLit-field /
    /// Array-element positions inside the `any`-destined init
    /// (chunk 519) — see module doc for why the wrap is needed.
    pub(crate) fn collect_any_init_sites(&mut self, eid: ExprId) {
        if self.try_mark(eid) {
            return;
        }
        match self.ast.get_expr(eid) {
            Expr::ObjectLit { fields } => {
                for (_, feid) in fields.clone() {
                    self.collect_any_init_sites(feid);
                }
            }
            Expr::Array(els) => {
                for e in els.clone() {
                    self.collect_any_init_sites(e);
                }
            }
            _ => {}
        }
    }
}

/// Binding names that are an operand of `===` / `!==` / `==` / `!=`
/// somewhere in the program.
///
/// Two name censuses over the expression arena, taken in one walk —
/// both ask "is this bare name in a position that can tell the two
/// representations of a function apart", and the answer decides
/// whether the forwarder wrap happens.
///
/// A top-level `function t` used as a value has TWO representations:
/// the canonical `__fncell_*` singleton every value SITE answers
/// (`t === t`, `[t]`, `{ m: t }`, `eq(t, t)`), and the raw FnSig code
/// address an IMMUTABLE binding keeps so `h()` stays a direct call
/// (`ssa_lower`'s `fn_addr_let` lane). Both are correct in isolation
/// and the property bag is shared, so an expando write through either
/// is visible through the other — the only observer that can tell them
/// apart is `===`, and it answered false (`const h = t; h === t`)
/// while two bindings answered true to each other, because they were
/// both on the raw side.
///
/// ES has one function object per declaration (§10.2.11 declaration
/// instantiation), so the binding must answer the singleton — but
/// wrapping every immutable fn binding would take the direct-dispatch
/// lane away from every program, identity-observing or not. This is
/// the census that keeps the wrap where the difference is observable:
/// the name is compared, so it must be the cell.
pub(crate) struct IdentCensus {
    /// Names compared with `===` / `==` on either side.
    pub(crate) eq_operands: std::collections::HashSet<String>,
    /// Names appearing as the BASE of a `delete` property reference
    /// (`delete f.length`). Module-wide rather than per-site, because
    /// the observer is a read somewhere ELSE: `delete f.name` then
    /// `f.name` has to answer "". A `.name` / `.length` read off a raw
    /// base folds statically and would keep answering the declaration
    /// forever, so a function whose properties are deleted anywhere
    /// loses the static-fold face at every site — the all-or-nothing
    /// posture `proto_shadow` already takes for a patched builtin.
    pub(crate) delete_bases: std::collections::HashSet<String>,
    /// Names handed to a define family call as the RECEIVER
    /// (`Object.defineProperty(f, …)`, `Object.defineProperties`,
    /// `Reflect.defineProperty`). Same posture as `delete_bases` and
    /// for the same reason: a define can move `length` and `name`
    /// (§20.2.3 makes both configurable), and the compile-time folds
    /// read the DECLARATION, so `defineProperty(f, "length",
    /// {value: 9})` was invisible to `f.length` while `(f as any)
    /// .length` — the same cell — answered 9. All-or-nothing over the
    /// module: which define reached which function is not a question
    /// a syntactic census can answer.
    pub(crate) define_bases: std::collections::HashSet<String>,
}

pub(crate) fn ident_census(ast: &Ast) -> IdentCensus {
    let mut out = IdentCensus {
        eq_operands: std::collections::HashSet::new(),
        delete_bases: std::collections::HashSet::new(),
        define_bases: std::collections::HashSet::new(),
    };
    // `as` layers are value pass-throughs — `(h as any) === t` is
    // still a comparison of `h`, and `delete (g as any).name` is
    // still a delete off `g`.
    let strip_as = |mut e: ExprId| {
        while let Expr::As { expr, .. } = ast.get_expr(e) {
            e = *expr;
        }
        e
    };
    for e in &ast.exprs {
        match e {
            Expr::BinOp { op, left, right }
                if matches!(
                    op,
                    BinOp::Eq | BinOp::Neq | BinOp::LooseEq | BinOp::LooseNeq
                ) =>
            {
                for side in [*left, *right] {
                    if let Expr::Ident(n) = ast.get_expr(strip_as(side)) {
                        out.eq_operands.insert(n.clone());
                    }
                }
            }
            Expr::Delete { expr } => {
                let base = match ast.get_expr(*expr) {
                    Expr::Member { obj, .. } | Expr::Index { obj, .. } => *obj,
                    _ => continue,
                };
                if let Expr::Ident(n) = ast.get_expr(strip_as(base)) {
                    out.delete_bases.insert(n.clone());
                }
            }
            Expr::Call { callee, args } => {
                let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                    continue;
                };
                let Expr::Ident(ns) = ast.get_expr(*obj) else {
                    continue;
                };
                let is_define = matches!(
                    (ns.as_str(), name.as_str()),
                    ("Object", "defineProperty" | "defineProperties")
                        | ("Reflect", "defineProperty")
                );
                if !is_define {
                    continue;
                }
                if let Some(&recv) = args.first()
                    && let Expr::Ident(n) = ast.get_expr(strip_as(recv))
                {
                    out.define_bases.insert(n.clone());
                }
            }
            _ => {}
        }
    }
    out
}
