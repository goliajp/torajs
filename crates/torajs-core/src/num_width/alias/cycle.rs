//! Cyclic alias detection, split out of the alias analysis.

use super::*;

/// Plain (non-class, non-generic, struct-shaped) alias names that sit
/// on a cycle in the named-type reference graph. Classes participate
/// as graph nodes — a `type A = { n: Node | null }` ↔ `class Node {
/// a: A | null }` mutual cycle makes `A` cyclic — but only plain
/// aliases land in the result; classes already have their nominal
/// path. Mentions are token-scanned, so exotic spellings (inline
/// nests, generics) over-approximate toward cyclic — width-only cost.
pub(super) fn cyclic_alias_names(ast: &Ast) -> HashSet<String> {
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
