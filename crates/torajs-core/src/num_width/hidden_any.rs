//! The bindings that may store a value the checker types by a
//! declaration while the lowering holds it as `any`.
//!
//! An escaped typed array lowers as an `Arr<Any>` (`escape.rs`), yet
//! the checker still reads `d[i]` by the declared element type; and
//! the element parameter of a callback over such an array, or over an
//! `any[]`, takes the elements through the callback's all-`any` twin.
//! A binding typed `number` that stores such a value converted it on
//! the way in, while §13.15.2 stores it as is: `t = t + d[1]` holds
//! "0s" when the element is "s". The check driver (`check_entry`)
//! retypes these bindings `any`; every other binding keeps its type.
//!
//! The value flows through copies, `+` (it may concatenate), `&&`,
//! `||`, `??`, the conditional, the comma and assignment expressions.
//! Every other operator answers a number or a boolean whatever its
//! operands hold, so the flow stops there.

use std::collections::{HashMap, HashSet};

use super::captures::{Body, bodies};
use super::container::canon_key_frozen;
use super::lattice::{Dep, fixpoint};
use super::{Analysis, Scope, SlotKey};
use crate::ast::{BinOp, Expr, ExprId, Stmt};
use crate::check::Type;

struct Graph<'s> {
    any_escaped: &'s HashSet<SlotKey>,
    edges: HashMap<SlotKey, Vec<Dep>>,
    seeds: Vec<SlotKey>,
    /// Retype candidates: the slot of a `let` / `const` declared
    /// `number`, `string`, `boolean` or with no annotation, whose
    /// initializer the checker types as one of those three, and that
    /// initializer, which names the declaration to the driver.
    decls: Vec<(SlotKey, ExprId)>,
}

/// The initializers of the declarations to retype `any`. `elem_params`
/// are the callback parameters handed `any` elements
/// (`hof_receivers::settle`).
pub(super) fn collect(
    a: &Analysis<'_>,
    any_escaped: &HashSet<SlotKey>,
    elem_params: &HashSet<(String, String)>,
) -> HashSet<ExprId> {
    if any_escaped.is_empty() && elem_params.is_empty() {
        return HashSet::new();
    }
    let mut g = Graph {
        any_escaped,
        edges: HashMap::new(),
        seeds: elem_params
            .iter()
            .map(|(f, p)| SlotKey::Param(f.clone(), p.clone()))
            .collect(),
        decls: Vec::new(),
    };
    for b in bodies(a.ast) {
        g.walk_body(a, &b);
    }
    let tainted = fixpoint(g.seeds, &g.edges);
    let out: Vec<(SlotKey, ExprId)> = g
        .decls
        .into_iter()
        .filter(|(k, _)| tainted.contains(k))
        .collect();
    // TORAJS_NUM_WIDTH_STATS=1 — one line per retyped declaration.
    if std::env::var_os("TORAJS_NUM_WIDTH_STATS").is_some() {
        for (k, init) in &out {
            eprintln!("[num_width] hidden-any decl: {k:?} init {}", init.0);
        }
    }
    out.into_iter().map(|(_, init)| init).collect()
}

/// The slots a write to `n` lands in: the one `n` resolves to, or in a
/// lifted body every slot of that spelling (`walk::assign_to_name`).
fn write_keys(a: &Analysis<'_>, n: &str, scope: &Scope) -> Vec<SlotKey> {
    if let Some(k) = a.resolve(n, scope) {
        return vec![k];
    }
    if scope.fn_name.starts_with("__") {
        return a.by_name.get(n).cloned().unwrap_or_default();
    }
    Vec::new()
}

impl Graph<'_> {
    /// The declarations, for-of bindings and assignments of one body.
    fn walk_body(&mut self, a: &Analysis<'_>, b: &Body<'_>) {
        let scope = Scope {
            fn_name: &b.name,
            params: b.params.clone(),
            locals: b.locals.clone(),
        };
        b.each_binding(&mut |n, decl| match decl {
            Some(Stmt::LetDecl { init, type_ann, .. }) => {
                let k = b.binding_key(n);
                self.flow(a, &[k.clone()], *init, &scope);
                if matches!(
                    type_ann.as_deref(),
                    None | Some("number" | "string" | "boolean")
                ) && matches!(
                    a.expr_types.get(init),
                    Some(Type::Number | Type::String | Type::Boolean)
                ) {
                    self.decls.push((k, *init));
                }
            }
            Some(Stmt::ForOf { elem_expr, .. }) => {
                self.flow(a, &[b.binding_key(n)], *elem_expr, &scope)
            }
            _ => {}
        });
        b.each_expr(a.ast, &mut |eid| {
            if let Expr::Assign { target, value } = a.ast.get_expr(eid)
                && let Expr::Ident(n) = a.ast.get_expr(*target)
            {
                self.flow(a, &write_keys(a, n, &scope), *value, &scope);
            }
        });
    }

    /// Edges from every source `value` carries into each of `dsts`.
    fn flow(&mut self, a: &Analysis<'_>, dsts: &[SlotKey], value: ExprId, scope: &Scope) {
        if dsts.is_empty() {
            return;
        }
        let mut srcs = Vec::new();
        self.sources(a, value, scope, &mut srcs);
        for s in srcs {
            let out = self.edges.entry(s).or_default();
            out.extend(dsts.iter().map(|d| (d.clone(), false)));
        }
    }

    fn sources(&mut self, a: &Analysis<'_>, eid: ExprId, scope: &Scope, out: &mut Vec<SlotKey>) {
        match a.ast.get_expr(eid) {
            Expr::Ident(n) => out.extend(write_keys(a, n, scope)),
            Expr::Index { obj, .. } | Expr::OptIndex { obj, .. } => {
                if let Some(k) = self.escaped_elem(a, *obj, scope) {
                    self.seeds.push(k.clone());
                    out.push(k);
                }
            }
            // a `+` the checker types `string` has a string operand and
            // answers a string whatever the other one holds
            Expr::BinOp { op: BinOp::Add, .. } if a.expr_types.get(&eid) == Some(&Type::String) => {
            }
            Expr::BinOp {
                op: BinOp::Add | BinOp::LAnd | BinOp::LOr,
                left,
                right,
            }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.sources(a, *left, scope, out);
                self.sources(a, *right, scope, out);
            }
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => {
                self.sources(a, *then_branch, scope, out);
                self.sources(a, *else_branch, scope, out);
            }
            Expr::Sequence { right: e, .. }
            | Expr::As { expr: e, .. }
            | Expr::Assign { value: e, .. } => self.sources(a, *e, scope, out),
            _ => {}
        }
    }

    /// The element point of `obj` when it is a typed array whose alias
    /// class escaped: a read of it answers whatever the `any` side
    /// stored. An `any[]` read is typed `any` by the checker already.
    fn escaped_elem(&self, a: &Analysis<'_>, obj: ExprId, scope: &Scope) -> Option<SlotKey> {
        if !matches!(a.expr_types.get(&obj), Some(Type::Array(el)) if **el != Type::Any) {
            return None;
        }
        let k = canon_key_frozen(&a.uf, &a.container_key_lookup(obj, scope)?);
        self.any_escaped
            .contains(&k)
            .then(|| SlotKey::Elem(Box::new(k)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::analyze;
    use crate::ast::Stmt;
    use crate::{lexer, parser};
    use std::collections::HashMap;

    /// The top-level `let` names the analysis would retype.
    fn retyped(src: &str) -> Vec<String> {
        let tokens = lexer::tokenize(src).expect("lex");
        let ast = parser::parse(src, &tokens).expect("parse");
        let (_, types) = crate::check::check_with_types(&ast).expect("check");
        let t = analyze(&ast, &HashMap::new(), &HashMap::new(), &types);
        let mut out: Vec<String> = ast
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::LetDecl { name, init, .. } if t.hidden_any_inits.contains(init) => {
                    Some(name.clone())
                }
                _ => None,
            })
            .collect();
        out.sort();
        out
    }

    const ESCAPED: &str = "const d: number[] = [1, 2];\nconst u: any = d;\n";

    #[test]
    fn plus_and_copies_carry_an_escaped_element() {
        let src = format!(
            "{ESCAPED}let t: number = 0;\nt = t + d[1];\nlet c = 0;\nc = t;\nlet m: number = 1;\nm = m * d[0];\nconst k = d.length > 0 ? d[0] : 0;"
        );
        assert_eq!(retyped(&src), ["c", "k", "t"]);
    }

    #[test]
    fn for_of_binding_carries_an_escaped_element() {
        let src = format!("{ESCAPED}let acc = 0;\nfor (const v of d) {{ acc = acc + v; }}");
        assert_eq!(retyped(&src), ["acc"]);
    }

    #[test]
    fn string_concatenation_and_unescaped_arrays_keep_their_type() {
        let src = format!(
            "{ESCAPED}let s: string = \"\";\ns = s + \"x\" + d[0];\nconst b: number[] = [1];\nlet n: number = 0;\nn = n + b[0];"
        );
        assert!(retyped(&src).is_empty());
    }
}
