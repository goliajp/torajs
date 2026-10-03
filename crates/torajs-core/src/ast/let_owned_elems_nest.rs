//! Containers of split products for the owned-elements census
//! ([`super::let_owned_elems`]): `const rows: string[][] = []` filled
//! with `rows[i] = parts` or `rows.push(parts)`.
//!
//! Storing a product into an element of such a container used to be
//! an escape, so the product was materialized at its declaration and
//! every read of it in the loop walked owned copies. The container's
//! element is instead given a name of its own — its element class —
//! and connected to the stored names by the alias edges
//! [`super::let_owned_elems_alias`] already draws, so the container
//! is declared `Arr<Arr<Substr>>` and the store shares the block.
//!
//! A container is admitted on shape: an array-of-string-arrays
//! annotation, an empty `[]` init, one declaration, no other binder of
//! the name, not promoted to a data global and not exported. Its
//! element class is then walked like any other member, with the only
//! uses of the container that keep it: `C.length`, `C[e].length`,
//! `C.push([])`, `C.push(M)` and `C[e] = M` with `M` a connectable
//! name, and `const r = C[e]` (an alias edge). Any other use of the
//! container — a bare value, another method, a length store, a nested
//! element write — poisons the class, and so does an occurrence of
//! the container the statement walk never reached (a class body the
//! walk does not enter), so the census falls back to today's
//! verdicts.

use std::cell::RefCell;
use std::collections::HashSet;

use super::let_owned_elems_names::AliasNames;
use super::{Ast, Expr, ExprId, Stmt};

/// An array of owned-string arrays.
pub(super) fn is_nest_ann(ann: Option<&str>) -> bool {
    matches!(
        ann,
        Some("string[][]" | "Array<string[]>" | "Array<Array<string>>")
    )
}

/// Admit the containers among the declarations `collect_names` saw
/// with a nested annotation, and count each element class as one
/// declaration so the class settles like any other name.
pub(super) fn admit_containers(ast: &Ast, names: &mut AliasNames) {
    let exported = exported_names(ast);
    let admitted: Vec<String> = names
        .nest_shaped
        .iter()
        .filter(|(n, init)| {
            matches!(ast.get_expr(*init), Expr::Array(els) if els.is_empty())
                && names.let_counts.get(n) == Some(&1)
                && !names.foreign.contains(n)
                && !names.promoted.contains(n)
                && !exported.contains(n)
        })
        .map(|(n, _)| n.clone())
        .collect();
    for c in admitted {
        // a NUL never spells an identifier, so the name is the class's own
        let class = format!("{c}\u{0}[]");
        names.let_counts.insert(class.clone(), 1);
        names.containers.insert(c, class);
    }
}

fn exported_names(ast: &Ast) -> HashSet<String> {
    let mut out = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::ExportDecl { inner, named, .. } = s {
            for (n, alias) in named {
                out.insert(n.clone());
                out.extend(alias.clone());
            }
            if let Some(inner) = inner.as_deref() {
                for t in super::stmts_flat(std::slice::from_ref(inner)) {
                    if let Stmt::LetDecl { name, .. } = t {
                        out.insert(name.clone());
                    }
                }
            }
        }
    }
    out
}

fn container<'a>(ast: &Ast, names: &'a AliasNames, eid: ExprId) -> Option<&'a str> {
    match ast.get_expr(eid) {
        Expr::Ident(c) => names.containers.get(c).map(String::as_str),
        _ => None,
    }
}

/// `C[e]` with `C` an admitted container: the name of its element
/// class.
pub(super) fn element_class<'a>(ast: &Ast, names: &'a AliasNames, eid: ExprId) -> Option<&'a str> {
    match ast.get_expr(eid) {
        Expr::Index { obj, .. } => container(ast, names, *obj),
        _ => None,
    }
}

/// The slot a node stores a bare value into, by name, and the value:
/// `t = v`, `C[e] = v`, `C.push(v)`.
pub(super) fn edge_slot<'a>(
    ast: &'a Ast,
    names: &'a AliasNames,
    e: &'a Expr,
) -> Option<(&'a str, ExprId)> {
    match e {
        Expr::Assign { target, value } => match ast.get_expr(*target) {
            Expr::Ident(t) => Some((t.as_str(), *value)),
            _ => element_class(ast, names, *target).map(|c| (c, *value)),
        },
        Expr::Call { callee, args } if args.len() == 1 => match ast.get_expr(*callee) {
            Expr::Member { obj, name } if name == "push" => {
                container(ast, names, *obj).map(|c| (c, args[0]))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The walk of one container's element class: the container's name,
/// the class's, and every occurrence of the container the walk ruled
/// on.
pub(super) struct Nest<'a> {
    pub c: &'a str,
    pub class: &'a str,
    pub seen: RefCell<HashSet<ExprId>>,
}

impl Nest<'_> {
    fn is_c(&self, ast: &Ast, e: ExprId) -> bool {
        let hit = matches!(ast.get_expr(e), Expr::Ident(n) if n == self.c);
        if hit {
            self.seen.borrow_mut().insert(e);
        }
        hit
    }

    /// `C[e]`: its index.
    fn element(&self, ast: &Ast, e: ExprId) -> Option<ExprId> {
        match ast.get_expr(e) {
            Expr::Index { obj, index } if self.is_c(ast, *obj) => Some(*index),
            _ => None,
        }
    }

    /// `C.m` or `C[e].m`: a property of the container or of one element.
    fn touches(&self, ast: &Ast, e: ExprId) -> bool {
        match ast.get_expr(e) {
            Expr::Member { obj, .. } => self.is_c(ast, *obj) || self.element(ast, *obj).is_some(),
            _ => false,
        }
    }

    /// Every occurrence of the container in the program was ruled on.
    pub(super) fn all_seen(&self, ast: &Ast) -> bool {
        let seen = self.seen.borrow();
        ast.exprs.iter().enumerate().all(|(i, e)| {
            !matches!(e, Expr::Ident(n) if n == self.c) || seen.contains(&ExprId(i as u32))
        })
    }
}

/// Rule on a node that names the container, for the walk of its
/// element class; None leaves the node to the ordinary walk. `alias`
/// asks whether a bare value is an edge into a named slot, `walk`
/// rules on a child.
pub(super) fn container_use(
    ast: &Ast,
    eid: ExprId,
    n: &Nest<'_>,
    alias: &dyn Fn(&str, ExprId) -> bool,
    walk: &dyn Fn(ExprId) -> bool,
) -> Option<bool> {
    match ast.get_expr(eid) {
        Expr::Ident(x) if x == n.c => Some(true),
        Expr::Closure { captures, .. } if captures.iter().any(|x| x == n.c) => Some(true),
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } if n.touches(ast, *target) => {
            Some(true)
        }
        Expr::Delete { expr } if n.touches(ast, *expr) => Some(true),
        Expr::Assign { target, value } => {
            let index = n.element(ast, *target)?;
            Some(!alias(n.class, *value) || walk(index) || walk(*value))
        }
        Expr::Member { obj, name } if n.is_c(ast, *obj) => Some(name != "length"),
        Expr::Member { obj, name } => {
            let index = n.element(ast, *obj)?;
            Some(name != "length" || walk(index))
        }
        Expr::Call { callee, args } => match ast.get_expr(*callee) {
            Expr::Member { obj, name }
                if name == "push" && args.len() == 1 && n.is_c(ast, *obj) =>
            {
                let empty = matches!(ast.get_expr(args[0]), Expr::Array(els) if els.is_empty());
                Some(!empty && (!alias(n.class, args[0]) || walk(args[0])))
            }
            _ => None,
        },
        _ => None,
    }
}

/// `const r = C[e]` (unwrapped): an alias edge into the element class,
/// for the walk of that class.
pub(super) fn let_use(
    ast: &Ast,
    name: &str,
    init: ExprId,
    n: &Nest<'_>,
    alias: &dyn Fn(&str, ExprId) -> bool,
    walk: &dyn Fn(ExprId) -> bool,
) -> Option<bool> {
    let index = n.element(ast, init)?;
    Some(!alias(name, init) || walk(index))
}

#[cfg(test)]
mod tests {
    use crate::ast::{Expr, Stmt};
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// Run the census on `src`: does the container `rows` hold views,
    /// and how many split products are still listed as owned.
    fn census(src: &str) -> (bool, usize) {
        let tokens = tokenize(src).expect("lex");
        let mut ast = parse(src, &tokens).expect("parse");
        crate::ast::analyze_let_owned_elems(&mut ast);
        let rows = crate::ast::stmts_flat(&ast.stmts)
            .into_iter()
            .find_map(|s| match s {
                Stmt::LetDecl { name, init, .. } if name == "rows" => Some(*init),
                _ => None,
            })
            .expect("rows");
        let owned_splits = ast
            .let_owned_elem_inits
            .iter()
            .filter(|i| matches!(ast.get_expr(**i), Expr::Call { .. }))
            .count();
        (ast.view_slot_inits.contains_key(&rows), owned_splits)
    }

    const HEAD: &str = "const lines: string[] = ['a,b', 'c,d']\n\
                        const rows: string[][] = []\n\
                        for (let i = 0; i < 2; i++) rows.push([])\n";
    const STORE: &str = "for (let i = 0; i < 4; i++) {\n\
                         const parts = lines[i & 1].split(',')\n\
                         rows[i & 1] = parts\n\
                         }\n";

    fn with(tail: &str) -> (bool, usize) {
        census(&format!("{HEAD}{STORE}{tail}"))
    }

    #[test]
    fn element_store_and_aliased_read_keep_views() {
        let got = with(
            "const row = rows[1]\nconsole.log(rows.length + rows[0].length + row[0].length)\n",
        );
        assert_eq!(got, (true, 0));
    }

    #[test]
    fn push_of_a_product_keeps_views() {
        let got = census(&format!(
            "{HEAD}const p = lines[0].split(',')\nrows.push(p)\nconst r = rows[2]\nconsole.log(r.join('|'))\n"
        ));
        assert_eq!(got, (true, 0));
    }

    #[test]
    fn every_other_use_of_the_container_poisons() {
        for tail in [
            "console.log(rows)\n",
            "rows[0].push('z')\n",
            "rows[0][1] = 'z'\n",
            "rows.length = 1\n",
            "rows[0].length = 1\n",
            "for (const r of rows) console.log(r.length)\n",
            "console.log(rows[0].join('|'))\n",
            "console.log(rows[0][0])\n",
            "const q = rows.pop()\n",
            "rows.push(lines[0].split(','))\n",
            "rows[1] = []\n",
            "const r: any = rows[0]\n",
            "function f(): string[] { return rows[0] }\n",
        ] {
            assert_eq!(with(tail), (false, 1), "{tail}");
        }
    }

    #[test]
    fn a_written_alias_of_an_element_poisons() {
        assert_eq!(with("const row = rows[0]\nrow.push('z')\n"), (false, 1));
    }
}
